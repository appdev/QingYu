import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {Compartment} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import {runScopeHandlers} from "@codemirror/view";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {MarkdownSlashMenuController} from "./markdownSlashMenu";
import {installMarkdownTestDom} from "./markraTestDom";

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

let cleanup: () => void;
let controller: MarkdownSlashMenuController | undefined;
let view: EditorView | undefined;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
    Object.assign(window, {
        siyuan: {
            config: {editor: {}},
            languages: {
                emptyContent: "No matching commands",
                heading1: "Heading 1",
                heading2: "Heading 2",
                heading3: "Heading 3",
                heading4: "Heading 4",
                heading5: "Heading 5",
                heading6: "Heading 6",
            },
        },
    });
});

afterEach(() => {
    controller?.destroy();
    controller = undefined;
    view?.destroy();
    view = undefined;
    cleanup();
});

const createMenu = (doc: string) => {
    const scrollElement = document.body.appendChild(document.createElement("div"));
    view = new EditorView({
        doc,
        extensions: [minimalSetup, createSiyuanMarkraExtension({
            adapter,
            documentPath: () => "/test.md",
            mode: "visual",
        })],
        parent: scrollElement,
        selection: {anchor: doc.length},
    });
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: () => ({bottom: 60, height: 20, left: 40, right: 41, top: 40, width: 1, x: 40, y: 40}),
    });
    controller = new MarkdownSlashMenuController(view, scrollElement);
    return {controller, scrollElement, view};
};

test("renders the selected slash command at the caret and executes it", () => {
    const editor = createMenu("/h2");

    editor.controller.update();

    const menu = document.querySelector<HTMLElement>('[data-markdown-slash-menu="true"]');
    assert.equal(menu?.getAttribute("role"), "menu");
    assert.equal(menu?.style.left, "40px");
    assert.equal(menu?.style.top, "68px");
    assert.equal(menu?.querySelector(".b3-menu__item--current .b3-menu__label")?.textContent, "Heading 2");
    const option = menu?.querySelector<HTMLButtonElement>('[data-command="block.heading.2"]');
    option?.dispatchEvent(new MouseEvent("mousedown", {bubbles: true}));
    option?.click();
    assert.equal(editor.view.state.doc.toString(), "## ");
    assert.equal(document.querySelector('[data-markdown-slash-menu="true"]'), null);
});

test("tracks keyboard selection and renders an empty result", () => {
    const editor = createMenu("/");
    editor.controller.update();
    assert.equal(runScopeHandlers(editor.view, new KeyboardEvent("keydown", {key: "ArrowDown"}), "editor"), true);
    editor.controller.update();
    assert.equal(document.querySelectorAll(".b3-menu__item--current").length, 1);
    assert.equal(document.querySelector(".b3-menu__item--current")?.getAttribute("data-command"), "block.heading.1");

    editor.controller.destroy();
    editor.view.destroy();
    const empty = createMenu("/not-a-command");
    empty.controller.update();
    assert.equal(document.querySelector(".markdown-editor__slash-menu-empty")?.textContent, "No matching commands");
});

test("keeps the mounted menu and its scroll position during keyboard and editor scrolling", () => {
    const editor = createMenu("/");
    editor.controller.update();
    const menu = document.querySelector<HTMLElement>('[data-markdown-slash-menu="true"]');
    const items = menu?.querySelector<HTMLElement>(".b3-menu__items");
    assert.ok(menu);
    assert.ok(items);
    items.scrollTop = 12;

    assert.equal(runScopeHandlers(editor.view, new KeyboardEvent("keydown", {key: "ArrowDown"}), "editor"), true);
    editor.controller.update();

    assert.equal(document.querySelector('[data-markdown-slash-menu="true"]'), menu);
    assert.equal(menu.querySelector(".b3-menu__item--current")?.getAttribute("data-command"), "block.heading.1");
    assert.equal(items.scrollTop, 12);

    editor.scrollElement.dispatchEvent(new Event("scroll"));
    assert.equal(document.querySelector('[data-markdown-slash-menu="true"]'), menu);
    assert.equal(items.scrollTop, 12);
});

test("closes on outside pointer input without reopening the unchanged query", () => {
    const editor = createMenu("/h2");
    editor.controller.update();

    document.body.dispatchEvent(new Event("pointerdown", {bubbles: true}));
    editor.view.dispatch({});
    editor.controller.update();

    assert.equal(document.querySelector('[data-markdown-slash-menu="true"]'), null);
    assert.equal(editor.view.state.doc.toString(), "/h2");
});

test("keeps query text when coordinates are unavailable and removes listeners on destroy", () => {
    const editor = createMenu("/h2");
    Object.defineProperty(editor.view, "coordsAtPos", {configurable: true, value: (): null => null});
    editor.controller.update();
    assert.equal(document.querySelector('[data-markdown-slash-menu="true"]'), null);
    assert.equal(editor.view.state.doc.toString(), "/h2");

    editor.controller.destroy();
    editor.view.destroy();
    assert.doesNotThrow(() => {
        window.dispatchEvent(new Event("resize"));
        document.body.dispatchEvent(new Event("pointerdown", {bubbles: true}));
    });
});

test("removes the menu when the editor switches to source mode", () => {
    const scrollElement = document.body.appendChild(document.createElement("div"));
    const mode = new Compartment();
    view = new EditorView({
        doc: "/h2",
        extensions: [minimalSetup, mode.of(createSiyuanMarkraExtension({
            adapter,
            documentPath: () => "/test.md",
            mode: "visual",
        }))],
        parent: scrollElement,
        selection: {anchor: 3},
    });
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: () => ({bottom: 60, height: 20, left: 40, right: 41, top: 40, width: 1, x: 40, y: 40}),
    });
    controller = new MarkdownSlashMenuController(view, scrollElement);
    controller.update();
    assert.notEqual(document.querySelector('[data-markdown-slash-menu="true"]'), null);

    view.dispatch({effects: mode.reconfigure(createSiyuanMarkraExtension({
        adapter,
        documentPath: () => "/test.md",
        mode: "source",
    }))});
    controller.update();

    assert.equal(document.querySelector('[data-markdown-slash-menu="true"]'), null);
    assert.equal(view.state.doc.toString(), "/h2");
});
