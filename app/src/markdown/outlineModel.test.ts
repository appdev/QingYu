import assert = require("node:assert/strict");
import {test} from "node:test";
import {getMarkdownOutlineWithPositions} from "./markra-core/markdown/markdown";
import {
    buildMarkdownOutlineTree,
    buildMarkdownOutlineTreeData,
    getMarkdownOutlinePosition,
} from "./outlineModel";

test("returns heading positions while ignoring fenced headings", () => {
    assert.deepEqual(getMarkdownOutlineWithPositions("# A\n```md\n# hidden\n```\n## B"), [
        {level: 1, title: "A", from: 0, to: 3},
        {level: 2, title: "B", from: 23, to: 27},
    ]);
});

test("builds heading hierarchy", () => {
    assert.equal(buildMarkdownOutlineTree("# A\n## B\n# C")[0].children[0].title, "B");
});

test("closes fences only with the same marker and sufficient length", () => {
    const source = "````md\n```\n# hidden one\n~~~\n# hidden two\n````\n# shown";
    assert.deepEqual(getMarkdownOutlineWithPositions(source).map((item) => item.title), ["shown"]);
});

test("preserves CRLF source offsets", () => {
    assert.deepEqual(getMarkdownOutlineWithPositions("# A\r\n## B"), [
        {level: 1, title: "A", from: 0, to: 3},
        {level: 2, title: "B", from: 5, to: 9},
    ]);
});

test("adapts Markdown headings to native outline tree data", () => {
    const data = buildMarkdownOutlineTreeData([
        {from: 0, level: 1, title: "Parent <unsafe>", to: 16},
        {from: 18, level: 3, title: "Skipped level", to: 34},
        {from: 36, level: 2, title: "Sibling", to: 45},
    ]);
    assert.equal(data.length, 1);
    assert.equal(data[0].name, "Parent &lt;unsafe>");
    assert.equal(data[0].subType, "h1");
    assert.equal(data[0].depth, 0);
    assert.equal(data[0].count, 2);
    assert.equal(data[0].children?.[0].subType, "h3");
    assert.equal(data[0].children?.[0].depth, 1);
    assert.equal(data[0].children?.[1].subType, "h2");
    assert.equal(getMarkdownOutlinePosition(data[0].children?.[1].id), 36);
    assert.equal(getMarkdownOutlinePosition("native-block-id"), undefined);
});
