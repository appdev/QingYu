import * as assert from "node:assert/strict";
import test from "node:test";
import {closeMobileMarkdownEditor, getMobileMarkdownEditor, setMobileMarkdownEditor} from "./markdownState";

test("closing the mobile Markdown editor releases it and restores the hidden editor", () => {
    let destroyed = false;
    let removed = false;
    let restored = false;
    const editor = {
        notebookId: "notebook",
        path: "/document.md",
        async rename() {
            return true;
        },
        async flush() {
            return true;
        },
        destroy() {
            destroyed = true;
        },
        element: {
            remove() {
                removed = true;
            },
        },
    };
    const hiddenElement = {
        classList: {
            remove(className: string) {
                restored = className === "fn__none";
            },
        },
    };

    setMobileMarkdownEditor(editor, [hiddenElement]);
    closeMobileMarkdownEditor();

    assert.equal(destroyed, true);
    assert.equal(removed, true);
    assert.equal(restored, true);
    assert.equal(getMobileMarkdownEditor(), undefined);
});
