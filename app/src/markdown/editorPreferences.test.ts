import assert = require("node:assert/strict");
import {test} from "node:test";
import {applyMarkdownEditorShellPreferences, getMarkdownFontZoomSize, readMarkdownEditorPreferences} from "./editorPreferences";
import {JSDOM} from "jsdom";

test("normalizes QingYu editor settings for Markdown", () => {
    assert.deepEqual(readMarkdownEditorPreferences({spellcheck: true, rtl: true, fullWidth: true, codeTabSpaces: 0}), {
        codeIndentation: "\t",
        fullWidth: true,
        justify: false,
        rtl: true,
        spellcheck: true,
    });
});

test("uses the platform modifier and clamps Markdown wheel zoom", () => {
    assert.equal(getMarkdownFontZoomSize({ctrlKey: true, deltaX: 0, deltaY: -1, metaKey: false}, 16, true, false), 17);
    assert.equal(getMarkdownFontZoomSize({ctrlKey: false, deltaX: 0, deltaY: 1, metaKey: true}, 16, true, true), 15);
    assert.equal(getMarkdownFontZoomSize({ctrlKey: true, deltaX: 1, deltaY: -1, metaKey: false}, 16, true, false), null);
    assert.equal(getMarkdownFontZoomSize({ctrlKey: true, deltaX: 0, deltaY: -1, metaKey: false}, 72, true, false), null);
    assert.equal(getMarkdownFontZoomSize({ctrlKey: true, deltaX: 0, deltaY: 1, metaKey: false}, 9, true, false), null);
});

test("applies full-width, direction, alignment, and spellcheck together", () => {
    const dom = new JSDOM('<div id="editor"><button data-type="markdown-mode"></button><button data-type="markdown-more"></button><div id="title"></div></div>');
    const element = dom.window.document.getElementById("editor") as HTMLElement;
    const title = dom.window.document.getElementById("title") as HTMLElement;
    applyMarkdownEditorShellPreferences(element, title, {
        codeIndentation: "    ", fullWidth: true, justify: true, rtl: true, spellcheck: true,
    });
    assert.equal(element.classList.contains("markdown-editor--full-width"), true);
    assert.equal(element.classList.contains("markdown-editor--rtl"), true);
    assert.equal(element.classList.contains("markdown-editor--justify"), true);
    assert.equal(title.spellcheck, true);
    assert.equal(element.querySelector('[data-type="markdown-more"]')?.classList.contains("block__icon--active"), false);
});
