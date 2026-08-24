import assert = require("node:assert/strict");
import {test} from "node:test";
import {
    canRestoreMarkdownOutline,
    collectMarkdownLayoutSourceKeys,
    markdownLayoutSourceKey,
    serializeMarkdownOutline,
} from "./outlineLayout";

test("derives stable workspace and external source keys", () => {
    assert.equal(markdownLayoutSourceKey({instance: "MarkdownEditor", notebookId: "box", path: "/doc.md"}),
        "workspace:box:/doc.md");
    assert.equal(markdownLayoutSourceKey({instance: "MarkdownEditor", externalCapabilityId: "capability"}),
        "external:capability");
});

test("serializes an outline with its source relationship", () => {
    assert.deepEqual(serializeMarkdownOutline("workspace:box:/doc.md"), {
        instance: "MarkdownOutline",
        sourceKey: "workspace:box:/doc.md",
    });
});

test("restores an outline only when its source is active or serialized", () => {
    const source = JSON.stringify({instance: "MarkdownEditor", notebookId: "box", path: "/doc.md"});
    assert.equal(canRestoreMarkdownOutline("workspace:box:/doc.md", false, [source]), true);
    assert.equal(canRestoreMarkdownOutline("workspace:box:/missing.md", false, [source]), false);
    assert.equal(canRestoreMarkdownOutline("workspace:box:/missing.md", false, ["not-json"]), false);
    assert.equal(canRestoreMarkdownOutline("workspace:box:/active.md", true, []), true);
});

test("precollects Markdown sources independently of outline/editor traversal order", () => {
    const editor = {instance: "MarkdownEditor", notebookId: "box", path: "/doc.md"};
    const outline = {instance: "MarkdownOutline", sourceKey: "workspace:box:/doc.md"};
    for (const children of [[outline, editor], [editor, outline]]) {
        assert.deepEqual([...collectMarkdownLayoutSourceKeys({instance: "Layout", children})], ["workspace:box:/doc.md"]);
    }
});
