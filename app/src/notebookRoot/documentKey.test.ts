import assert = require("node:assert/strict");
import test from "node:test";

test("notebook root document keys prefer IDs and normalize Markdown path fallbacks", async () => {
    const {notebookRootDocumentKey} = await import("./documentKey");
    assert.equal(notebookRootDocumentKey({
        kind: "sy",
        notebook: "box",
        id: "20260901120000-native",
        path: "/native.sy",
    }), "sy\u001fbox\u001f20260901120000-native");
    assert.equal(notebookRootDocumentKey({
        kind: "markdown",
        notebook: "box",
        id: "",
        path: "//folder///draft.md",
    }), "markdown\u001fbox\u001f/folder/draft.md");
});

test("notebook root element keys use the same identity contract", async () => {
    const {JSDOM} = await import("jsdom");
    const {notebookRootDocumentKey, notebookRootElementKey} = await import("./documentKey");
    const dom = new JSDOM("<article></article>");
    const element = dom.window.document.querySelector("article") as unknown as HTMLElement;
    element.dataset.kind = "markdown";
    element.dataset.notebook = "box";
    element.dataset.id = "";
    element.dataset.path = "//folder///draft.md";
    assert.equal(notebookRootElementKey(element), notebookRootDocumentKey({
        kind: "markdown",
        notebook: "box",
        id: "",
        path: "//folder///draft.md",
    }));
    dom.window.close();
});
