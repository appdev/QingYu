import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorView, minimalSetup} from "codemirror";
import {EditorState} from "@codemirror/state";
import {
    codeBlockPreviewPlugin,
    liveMarkdown,
    type CodeBlockPreviewPluginOptions,
} from "./markra-core/codemirror";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {highlightMarkraCode} from "./markra-core/code-support";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {installMarkdownTestDom} from "./markraTestDom";
import {mountCodeLanguageMenu} from "../protyle/codeLanguageMenu";

let cleanup: () => void;
let view: EditorView | undefined;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => {
    view?.destroy();
    view = undefined;
    cleanup();
});

test("matches the native code action hierarchy without a synthetic top gap", async () => {
    view = new EditorView({
        doc: "```java\nconst value = 1;\n```",
        extensions: [minimalSetup, liveMarkdown({plugins: [codeBlockPreviewPlugin()]})],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const action = view.dom.querySelector<HTMLElement>(".protyle-action.cm-markra-code-actions");
    assert.ok(action);
    assert.deepEqual(Array.from(action.children, (child) => child.className), [
        "protyle-action--first protyle-action__language markra-code-language-control cm-markra-code-header markra-code-language-label",
        "fn__flex-1",
        "protyle-icon protyle-action__copy markra-code-copy-button",
        "protyle-icon protyle-action__menu markra-code-more-button",
    ]);
    assert.equal(view.dom.querySelector(".cm-markra-code-top-gap"), null);
    assert.ok(view.dom.querySelector(".cm-markra-code-opening-line"));
    assert.ok(view.dom.querySelector(".cm-markra-code-content-first.cm-markra-code-content-last"));
});

test("uses the SiYuan language popover and updates the Markdown fence", async () => {
    const editorHost = document.createElement("div");
    editorHost.className = "markdown-editor";
    document.body.append(editorHost);
    view = new EditorView({
        doc: "```text\nline\n```",
        extensions: [
            minimalSetup,
            liveMarkdown({plugins: [codeBlockPreviewPlugin({
                icons: {
                    check: "#iconCheck",
                    copy: "#iconCopy",
                    more: "#iconMore",
                },
                labels: {
                    clearLanguage: "Clear",
                    searchLanguage: "Search",
                },
                languages: [
                    {label: "1c", value: "1c"},
                    {label: "java", value: "java"},
                    {label: "javascript", value: "javascript"},
                ],
            })]}),
        ],
        parent: editorHost,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const header = view.dom.querySelector<HTMLElement>(".cm-markra-code-actions");
    assert.equal(header?.querySelector(".markra-code-language-label")?.textContent, "text");
    assert.equal(header?.querySelector(".markra-code-copy-icon use")?.getAttribute("href"), "#iconCopy");
    assert.equal(header?.querySelector(".markra-code-copy-check-icon use")?.getAttribute("href"), "#iconCheck");
    assert.equal(header?.querySelector(".markra-code-more-icon use")?.getAttribute("href"), "#iconMore");
    assert.equal(header?.querySelector("select"), null);

    header?.querySelector<HTMLButtonElement>(".markra-code-more-button")?.click();
    const popover = document.querySelector<HTMLElement>(".markra-code-language-popover");
    assert.equal(popover?.parentElement, editorHost);
    const search = popover?.querySelector<HTMLInputElement>(".b3-text-field");
    assert.equal(search?.placeholder, "Search");
    assert.deepEqual(
        Array.from(popover?.querySelectorAll<HTMLElement>(".b3-list-item") ?? [], (item) => item.textContent),
        ["Clear", "1c", "java", "javascript"],
    );

    if (search) {
        search.value = "java";
        search.dispatchEvent(new Event("input", {bubbles: true}));
    }
    assert.deepEqual(
        Array.from(popover?.querySelectorAll<HTMLElement>(".b3-list-item") ?? [], (item) => item.textContent),
        ["Clear", "java", "javascript"],
    );
    popover?.querySelector<HTMLElement>('[data-id="javascript"]')?.click();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    assert.equal(view.state.doc.toString(), "```javascript\nline\n```");
    assert.equal(view.dom.querySelector(".markra-code-language-label")?.textContent, "javascript");
});

test("delegates the language menu to the host adapter", async () => {
    let request: Parameters<NonNullable<CodeBlockPreviewPluginOptions["openCodeLanguageMenu"]>>[0] | undefined;
    let focused = 0;
    view = new EditorView({
        doc: "```java\nline\n```",
        extensions: [
            minimalSetup,
            liveMarkdown({plugins: [codeBlockPreviewPlugin({
                languages: [{label: "java", value: "java"}, {label: "typescript", value: "typescript"}],
                openCodeLanguageMenu: (nextRequest) => {
                    request = nextRequest;
                    return {
                        destroy: () => undefined,
                        element: nextRequest.ownerDocument.createElement("div"),
                        focus: () => focused += 1,
                    };
                },
            })]}),
        ],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    view.dom.querySelector<HTMLElement>(".markra-code-language-control")?.click();
    assert.equal(focused, 1);
    assert.equal(request?.currentLanguage, "java");
    assert.deepEqual(request?.languages, ["java", "typescript"]);
    request?.onSelect("typescript");
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    assert.equal(view.state.doc.toString(), "```typescript\nline\n```");
});

test("reopens the host language menu after it closes itself", async () => {
    view = new EditorView({
        doc: "```java\nline\n```",
        extensions: [
            minimalSetup,
            liveMarkdown({plugins: [codeBlockPreviewPlugin({
                languages: [{label: "java", value: "java"}, {label: "typescript", value: "typescript"}],
                openCodeLanguageMenu: (request) => mountCodeLanguageMenu({
                    anchor: request.anchor,
                    container: request.ownerDocument.body,
                    currentLanguage: request.currentLanguage,
                    languages: request.languages,
                    labels: {clear: "Clear", search: "Search"},
                    onDestroy: request.onDestroy,
                    onSelect: request.onSelect,
                    position: () => undefined,
                }),
            })]}),
        ],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const languageControl = view.dom.querySelector<HTMLElement>(".markra-code-language-control");
    languageControl?.click();
    assert.ok(document.querySelector(".markra-code-language-popover"));

    document.body.dispatchEvent(new MouseEvent("mousedown", {bubbles: true}));
    assert.equal(document.querySelector(".markra-code-language-popover"), null);

    languageControl?.click();
    assert.ok(document.querySelector(".markra-code-language-popover"));
});

test("offers a custom SiYuan code language when search has no exact match", async () => {
    view = new EditorView({
        doc: "```text\nline\n```",
        extensions: [
            minimalSetup,
            liveMarkdown({plugins: [codeBlockPreviewPlugin({
                labels: {clearLanguage: "Clear", searchLanguage: "Search"},
                languages: [{label: "java", value: "java"}],
            })]}),
        ],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    view.dom.querySelector<HTMLButtonElement>(".markra-code-more-button")?.click();
    const popover = document.querySelector<HTMLElement>(".markra-code-language-popover");
    const search = popover?.querySelector<HTMLInputElement>(".b3-text-field");
    if (search) {
        search.value = "my lang`";
        search.dispatchEvent(new Event("input", {bubbles: true}));
    }
    assert.equal(popover?.querySelector<HTMLElement>('[data-id="customLanguage"]')?.textContent, "my_lang_");
});

test("reads SiYuan code languages when the popover opens", async () => {
    let languages = [{label: "text", value: "text"}];
    view = new EditorView({
        doc: "```text\nline\n```",
        extensions: [
            minimalSetup,
            liveMarkdown({plugins: [codeBlockPreviewPlugin({
                languages: () => languages,
            })]}),
        ],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    languages = [{label: "java", value: "java"}];
    view.dom.querySelector<HTMLButtonElement>(".markra-code-more-button")?.click();
    assert.deepEqual(
        Array.from(document.querySelectorAll<HTMLElement>(".markra-code-language-popover .b3-list-item"),
            (item) => item.textContent),
        ["Clear", "java"],
    );
});

test("disables native-style code controls in read-only mode", async () => {
    view = new EditorView({
        doc: "```text\nline\n```",
        extensions: [
            minimalSetup,
            EditorState.readOnly.of(true),
            liveMarkdown({plugins: [codeBlockPreviewPlugin({
                icons: {check: "#iconCheck", copy: "#iconCopy", more: "#iconMore"},
            })]}),
        ],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(view.dom.querySelector<HTMLButtonElement>(".markra-code-more-button")?.disabled, true);
});

test("treats unknown fenced languages as plain text", () => {
    assert.deepEqual(highlightMarkraCode("not-a-real-language", "const value = 1;"), []);
});

test("applies disabled native line numbers with enabled wrapping and disabled ligatures", async () => {
    view = new EditorView({
        doc: "```text\nconst longValue = 'line';\n```",
        extensions: [
            minimalSetup,
            liveMarkdown({plugins: [codeBlockPreviewPlugin({
                ligatures: false,
                lineWrap: true,
                showLineNumbers: false,
            })]}),
        ],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const line = view.dom.querySelector<HTMLElement>(".cm-markra-code-content-line");
    assert.equal(line?.dataset.codeLineWrap, "true");
    assert.equal(line?.dataset.codeLigatures, "false");
    assert.equal(line?.hasAttribute("data-code-line-number"), false);
});

test("applies enabled native line numbers with disabled wrapping and enabled ligatures", async () => {
    view = new EditorView({
        doc: "```text\nconst value = 1;\n```",
        extensions: [
            minimalSetup,
            liveMarkdown({plugins: [codeBlockPreviewPlugin({
                ligatures: true,
                lineWrap: false,
                showLineNumbers: true,
            })]}),
        ],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const line = view.dom.querySelector<HTMLElement>(".cm-markra-code-content-line");
    assert.equal(line?.dataset.codeLineWrap, "false");
    assert.equal(line?.dataset.codeLigatures, "true");
    assert.equal(line?.dataset.codeLineNumber, "1");
});

test("uses the current SiYuan code settings in visual mode", async () => {
    Object.assign(window, {
        siyuan: {
            config: {
                editor: {
                    codeLigatures: true,
                    codeLineWrap: false,
                    codeSyntaxHighlightLineNum: false,
                },
            },
        },
    });
    const adapter: MarkdownHostAdapter = {
        createIcon: () => document.createElementNS("http://www.w3.org/2000/svg", "svg"),
        notifyError: () => undefined,
        openLink: () => undefined,
        positionPopover: () => undefined,
        renderMath: () => document.createElement("span"),
        renderMermaid: async () => document.createElement("div"),
        resolveImageSource: (source) => source,
        saveClipboardAssets: async () => [],
    };
    view = new EditorView({
        doc: "```text\nconst value = 1;\n```",
        extensions: [
            minimalSetup,
            createSiyuanMarkraExtension({
                adapter,
                documentPath: () => "/test.md",
                mode: "visual",
            }),
        ],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const line = view.dom.querySelector<HTMLElement>(".cm-markra-code-content-line");
    assert.equal(line?.dataset.codeLineWrap, "false");
    assert.equal(line?.dataset.codeLigatures, "true");
    assert.equal(line?.hasAttribute("data-code-line-number"), false);
});
