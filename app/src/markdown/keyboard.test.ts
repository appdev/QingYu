import * as assert from "node:assert/strict";
import {describe, it} from "node:test";
import {isMarkdownSelectAll, selectActiveEditorContent} from "./keyboard";
import {installMarkdownTestDom} from "./markraTestDom";

describe("isMarkdownSelectAll", () => {
    it("matches the platform select-all shortcut", () => {
        assert.equal(isMarkdownSelectAll({key: "a", metaKey: true, ctrlKey: false, altKey: false, shiftKey: false}), true);
        assert.equal(isMarkdownSelectAll({key: "A", metaKey: false, ctrlKey: true, altKey: false, shiftKey: false}), true);
    });

    it("does not consume modified or unrelated shortcuts", () => {
        assert.equal(isMarkdownSelectAll({key: "a", metaKey: true, ctrlKey: false, altKey: false, shiftKey: true}), false);
        assert.equal(isMarkdownSelectAll({key: "s", metaKey: true, ctrlKey: false, altKey: false, shiftKey: false}), false);
    });
});

describe("selectActiveEditorContent", () => {
    it("selects only the active Markdown title", () => {
        const cleanup = installMarkdownTestDom();
        try {
            document.body.innerHTML = `<div>before</div>
<div class="protyle-title__input" contenteditable="true" tabindex="0">Markdown title</div>
<div>after</div>`;
            const titleElement = document.querySelector(".protyle-title__input") as HTMLElement;
            titleElement.focus();

            selectActiveEditorContent();

            assert.equal(window.getSelection()?.toString(), "Markdown title");
        } finally {
            cleanup();
        }
    });
});
