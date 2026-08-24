import * as assert from "node:assert/strict";
import test from "node:test";
import {
    closeMobileMarkdownEditor,
    closeMobileMarkdownEditorForNotebook,
    getMobileMarkdownEditor,
    refreshMobileMarkdownReadOnly,
    setMobileMarkdownEditor,
} from "./markdownState";

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

test("refreshes readonly state and closes only the matching notebook", () => {
    let refreshed = 0;
    let destroyed = 0;
    setMobileMarkdownEditor({
        notebookId: "notebook",
        path: "/document.md",
        async rename() { return true; },
        async flush() { return true; },
        destroy() { destroyed += 1; },
        element: {remove() { return; }},
        refreshEditorConfig() { refreshed += 1; },
    }, []);

    refreshMobileMarkdownReadOnly();
    assert.equal(refreshed, 1);
    assert.equal(closeMobileMarkdownEditorForNotebook("other"), false);
    assert.equal(getMobileMarkdownEditor()?.notebookId, "notebook");
    assert.equal(closeMobileMarkdownEditorForNotebook("notebook"), true);
    assert.equal(destroyed, 1);
    assert.equal(getMobileMarkdownEditor(), undefined);
});
