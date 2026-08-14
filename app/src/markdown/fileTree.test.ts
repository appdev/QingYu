import * as assert from "node:assert/strict";
import {JSDOM} from "jsdom";
import test from "node:test";
import {
    createMarkdownFromFileTreeAction,
    getMarkdownFileTreeDisplayName,
    getMarkdownFileTreeNames,
} from "./fileTree";

test("hides only a terminal Markdown suffix in the file tree", () => {
    const cases = [
        ["未命名.md", "未命名"],
        ["notes.markdown", "notes"],
        ["README.MD", "README"],
        ["archive.MarkDown", "archive"],
        ["draft.md.backup", "draft.md.backup"],
        ["plain", "plain"],
    ] as const;

    for (const [name, expected] of cases) {
        assert.equal(getMarkdownFileTreeDisplayName(name), expected, name);
    }
});

test("keeps the complete Markdown name separate from its file tree display name", () => {
    assert.deepEqual(getMarkdownFileTreeNames("项目计划.markdown"), {
        dataName: "项目计划.markdown",
        displayName: "项目计划",
    });
});

test("routes a file tree add action to Markdown creation with its notebook and folder", () => {
    const dom = new JSDOM("<ul data-url=\"notebook-id\"><li data-path=\"/folder\"><span id=\"add\"></span></li></ul>");
    const actionElement = dom.window.document.querySelector("#add");
    const app = {name: "test-app"};
    let actual: unknown[] = [];

    const handled = createMarkdownFromFileTreeAction(app, actionElement, async (...args) => {
        actual = args;
        return true;
    });

    assert.equal(handled, true);
    assert.deepEqual(actual, [app, "notebook-id", "/folder"]);
});
