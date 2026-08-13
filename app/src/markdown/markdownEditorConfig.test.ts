import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {Compartment} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {reconfigureSiyuanMarkraExtension} from "./markdownEditorExtension";
import {installMarkdownTestDom} from "./markraTestDom";

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

test("refreshes native code settings without changing Markdown content or selection", async () => {
    Object.assign(window, {
        siyuan: {
            config: {
                editor: {
                    codeLigatures: false,
                    codeLineWrap: true,
                    codeSyntaxHighlightLineNum: true,
                },
            },
        },
    });
    const modeCompartment = new Compartment();
    const source = "```text\nconst value = 1;\n```";
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
        doc: source,
        extensions: [
            minimalSetup,
            modeCompartment.of(createSiyuanMarkraExtension({
                adapter,
                documentPath: () => "/test.md",
                mode: "visual",
            })),
        ],
        parent: document.body,
        selection: {anchor: 8},
    });
    window.siyuan.config.editor.codeLigatures = true;
    window.siyuan.config.editor.codeLineWrap = false;
    window.siyuan.config.editor.codeSyntaxHighlightLineNum = false;
    reconfigureSiyuanMarkraExtension(view, modeCompartment, {
        adapter,
        documentPath: () => "/test.md",
        mode: "visual",
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const line = view.dom.querySelector<HTMLElement>(".cm-markra-code-content-line");
    assert.equal(view.state.doc.toString(), source);
    assert.equal(view.state.selection.main.anchor, 8);
    assert.equal(line?.dataset.codeLineWrap, "false");
    assert.equal(line?.dataset.codeLigatures, "true");
    assert.equal(line?.hasAttribute("data-code-line-number"), false);
});
