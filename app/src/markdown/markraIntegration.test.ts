import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorState} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import {runScopeHandlers} from "@codemirror/view";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {convertCodeMirrorClipboardHtml} from "./markra-core/codemirror";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {installMarkdownTestDom} from "./markraTestDom";

const adapter: MarkdownHostAdapter = {
    createIcon(_name, className, ownerDocument) {
        const icon = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "svg");
        icon.classList.add(className);
        return icon;
    },
    notifyError() {},
    openLink() {},
    positionPopover() {},
    renderMath(_source, _displayMode, context) {
        return context.ownerDocument.createElement("span");
    },
    async renderMermaid(_source, context) {
        return context.ownerDocument.createElement("div");
    },
    resolveImageSource(source) {
        return source;
    },
    async saveClipboardAssets() {
        return [];
    },
};

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

const createView = (doc: string) => {
    view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc,
            extensions: [
                minimalSetup,
                createSiyuanMarkraExtension({
                    adapter,
                    documentPath: () => "/test.md",
                    mode: "visual",
                }),
            ],
        }),
    });
    return view;
};

test("converts copied rich tables to complete GFM Markdown", () => {
    const result = convertCodeMirrorClipboardHtml(`
<h2>文档变更记录</h2>
<table><thead><tr><th>版本</th><th>日期</th><th>作者</th><th>变更</th></tr></thead>
<tbody><tr><td>v1.0</td><td>2026-04-20</td><td>架构组</td><td><strong>初版</strong>：建立端到端框架</td></tr></tbody></table>`);
    assert.ok(result);
    assert.equal(result.structured, true);
    assert.match(result.markdown, /^## 文档变更记录/mu);
    assert.match(result.markdown, /\| 版本 \| 日期 \| 作者 \| 变更 \|/u);
    assert.match(result.markdown, /\| --- \| --- \| --- \| --- \|/u);
    assert.match(result.markdown, /\*\*初版\*\*/u);
});

test("renders tables through the Markra visual core without changing source", async () => {
    const source = "引言\n\n| 版本 | 日期 | 作者 | 变更 |\n| --- | --- | --- | --- |\n| v1.0 | 2026-04-20 | 架构组 | 初版 |";
    const editor = createView(source);
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(editor.state.doc.toString(), source);
    assert.equal(editor.dom.getAttribute("data-markdown-mode"), "visual");
    assert.equal(editor.dom.querySelectorAll(".cm-gutter").length, 0);
    assert.ok(editor.dom.querySelector("table"));
    assert.equal(editor.dom.querySelectorAll("tbody tr").length, 1);
});

test("keeps following text outside an unfinished code fence until Enter closes it", async () => {
    const editor = createView("```\n下方正文");
    editor.dispatch({selection: {anchor: 3}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(editor.dom.querySelector(".cm-markra-code-header"), null);
    assert.equal(editor.dom.querySelector(".cm-markra-code-content-line"), null);
    assert.equal(editor.dom.querySelector(".cm-markra-code-top-gap"), null);

    const handled = runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "Enter",
    }), "editor");
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(handled, true);
    assert.equal(editor.state.doc.toString(), "```\n\n```\n下方正文");
    assert.ok(editor.dom.querySelector(".cm-markra-code-header"));
    assert.equal(editor.dom.querySelector(".cm-markra-code-content-line")?.textContent, "");
});

test("does not preview an unfinished Mermaid fence over following text", async () => {
    const source = "```mermaid\ngraph TD\n下方正文";
    const editor = createView(source);
    editor.dispatch({selection: {anchor: "```mermaid".length}});
    editor.contentDOM.dispatchEvent(new Event("blur"));
    await Promise.resolve();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(editor.state.doc.toString(), source);
    assert.equal(editor.dom.querySelector(".markra-mermaid-render"), null);
});

test("adds safe SiYuan semantic aliases to rendered Markdown", async () => {
    const editor = createView("# 标题\n\n> 引用\n\n**粗体** *斜体* ~~删除~~ ==高亮== `代码`");
    editor.dispatch({selection: {anchor: editor.state.doc.length}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.ok(editor.dom.querySelector(".cm-markra-h1.h1"));
    assert.ok(editor.dom.querySelector(".cm-markra-blockquote.cm-markra-blockquote-first.cm-markra-blockquote-last"));
    assert.equal(editor.dom.querySelector(".cm-markra-blockquote.bq"), null);
    assert.ok(editor.dom.querySelector('.cm-markra-strong[data-type~="strong"]'));
    assert.ok(editor.dom.querySelector('.cm-markra-emphasis[data-type~="em"]'));
    assert.ok(editor.dom.querySelector('.cm-markra-strikethrough[data-type~="s"]'));
    assert.ok(editor.dom.querySelector('.cm-markra-highlight[data-type~="mark"]'));
    assert.ok(editor.dom.querySelector('.cm-markra-inline-code[data-type~="code"]'));
    assert.equal(editor.dom.querySelector("[data-node-id]"), null);
});

test("uses SiYuan sprite icons for every visual table toolbar action", async () => {
    const editor = createView("引言\n\n| 版本 | 作者 |\n| --- | --- |\n| v1.0 | 架构组 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const iconHref = (selector: string) => editor.dom.querySelector(`${selector} use`)?.getAttribute("href");
    assert.equal(iconHref(".markra-table-size-button"), "#iconTable");
    assert.equal(iconHref(".markra-table-align-left"), "#iconAlignLeft");
    assert.equal(iconHref(".markra-table-align-center"), "#iconAlignCenter");
    assert.equal(iconHref(".markra-table-align-right"), "#iconAlignRight");
    assert.equal(iconHref(".markra-table-width-button"), "#iconWidth");
    assert.equal(iconHref(".markra-table-delete-table"), "#iconTrashcan");
});

test("groups visual table actions by function without changing button order", async () => {
    const editor = createView("| 项目 | 负责人 |\n| --- | --- |\n| Markdown | 架构组 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const toolbar = editor.dom.querySelector(".markra-table-align-controls");
    assert.ok(toolbar);
    assert.deepEqual(
        Array.from(toolbar.children, (element) => element.getAttribute("data-table-control-group")),
        ["size", "alignment", "width", "delete"],
    );
    assert.equal(toolbar.querySelectorAll('[data-table-control-group="size"] .markra-table-control').length, 1);
    assert.equal(toolbar.querySelectorAll('[data-table-control-group="alignment"] .markra-table-control').length, 3);
    assert.equal(toolbar.querySelectorAll('[data-table-control-group="width"] .markra-table-control').length, 1);
    assert.equal(toolbar.querySelectorAll('[data-table-control-group="delete"] .markra-table-control').length, 1);
    assert.equal(
        toolbar.querySelector('[data-table-control-group="alignment"] [aria-pressed="true"]')?.classList.contains("markra-table-align-left"),
        true,
    );
    const widthButton = toolbar.querySelector('[data-table-control-group="width"] .markra-table-width-button');
    assert.equal(widthButton?.getAttribute("data-mode"), "auto");
    assert.equal(widthButton?.getAttribute("aria-pressed"), "true");
});

test("keeps visual table actions visible after the table loses focus", async () => {
    const editor = createView("| 项目 | 内容 |\n| --- | --- |\n| 文档版本 | v1.7 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const sizeButton = editor.dom.querySelector(".markra-table-size-button") as HTMLButtonElement;
    sizeButton.blur();

    assert.equal(sizeButton.classList.contains("block__icon"), true);
    assert.equal(sizeButton.hidden, false);
    assert.equal(sizeButton.disabled, false);
});

test("keeps the table size picker inside the CodeMirror theme scope", async () => {
    const editor = createView("| 项目 | 内容 |\n| --- | --- |\n| 文档版本 | v1.7 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const sizeButton = editor.dom.querySelector(".markra-table-size-button") as HTMLButtonElement;
    sizeButton.click();

    const popover = document.querySelector(".markra-table-size-popover");
    assert.ok(popover);
    assert.equal(popover.parentElement, editor.dom);
    assert.equal(popover.querySelectorAll(".markra-table-size-cell").length, 80);
});

test("uses SiYuan native image wrapper and resize handle", async () => {
    const editor = createView("引言\n\n![架构图](assets/architecture.png)");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.ok(editor.dom.querySelector(".img.markra-image-node"));
    assert.ok(editor.dom.querySelector(".img .protyle-action__drag"));
    assert.equal(editor.dom.querySelector(".markra-image-resize-handle"), null);
    assert.equal(editor.dom.querySelector(".markra-image-viewer-button"), null);
});

test("uses the intrinsic image width until the user resizes it", async () => {
    const editor = createView("![截图](assets/screenshot.png)");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const image = editor.dom.querySelector(".markra-image-frame img") as HTMLImageElement;
    const frame = editor.dom.querySelector(".markra-image-frame") as HTMLElement;
    assert.equal(image.loading, "eager");
    assert.equal(image.classList.contains("cm-markra-image"), true);
    Object.defineProperty(image, "naturalWidth", {configurable: true, value: 506});
    image.dispatchEvent(new Event("load"));

    assert.equal(frame.style.width, "506px");
    assert.equal(frame.classList.contains("markra-image-frame-sized"), false);
});

test("keeps Mod-A available for selecting the complete Markdown document", () => {
    const editor = createView("第一段\n\n第二段");
    const handled = runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "a",
        metaKey: true,
    }), "editor");
    assert.equal(handled, true);
    assert.equal(editor.state.selection.main.from, 0);
    assert.equal(editor.state.selection.main.to, editor.state.doc.length);
});
