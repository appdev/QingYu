import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {Compartment} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {reconfigureSiyuanMarkraExtension} from "./markdownEditorExtension";
import {MarkdownDocumentScrollController} from "./documentScroll";
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

test("restores the semantic document anchor after reconfiguration", async () => {
    const modeCompartment = new Compartment();
    const container = document.body.appendChild(document.createElement("div"));
    container.scrollTop = 100;
    container.getBoundingClientRect = () => ({
        bottom: 220,
        height: 200,
        left: 0,
        right: 400,
        top: 20,
        width: 400,
        x: 0,
        y: 20,
        toJSON: () => ({}),
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
        doc: "line one\nline two",
        extensions: [modeCompartment.of(createSiyuanMarkraExtension({
            adapter,
            documentPath: () => "/test.md",
            mode: "visual",
        }))],
        parent: container,
        selection: {anchor: 5},
    });
    let coordinateRead = 0;
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: () => {
            coordinateRead++;
            const top = coordinateRead === 1 ? 80 : 120;
            return {bottom: top + 20, height: 20, left: 0, right: 10, top, width: 10, x: 0, y: top, toJSON: () => ({})};
        },
    });
    const scroll = new MarkdownDocumentScrollController(() => view, container);

    reconfigureSiyuanMarkraExtension(view, modeCompartment, {
        adapter,
        documentPath: () => "/test.md",
        mode: "source",
    }, scroll);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(view.state.doc.toString(), "line one\nline two");
    assert.equal(view.state.selection.main.anchor, 5);
    assert.equal(container.scrollTop, 140);
    scroll.destroy();
});
