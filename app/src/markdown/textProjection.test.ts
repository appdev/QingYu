import assert = require("node:assert/strict");
import {test} from "node:test";
import {EditorState} from "@codemirror/state";
import {markraLanguage} from "./markra-core/codemirror";
import {projectMarkdownRange} from "./textProjection";

for (const [source, text] of [["甲乙**丙**丁", "甲乙丙丁"], ["[示例](https://example.com)", "示例"],
    ["`**字面**`", "**字面**"], ["==高亮==", "高亮"], ["$a*b*$", "a*b*"], ["\\*字面\\*", "*字面*"],
    ["![图片](a.png)", "图片"], ["# 标题", "标题"], ["`` `a` ``", "`a`"],
    ["| A | B |\n| --- | --- |\n| one | two |", "A\tB\none\ttwo"]]) {
    test(`projects visible text for ${source}`, () => {
        const state = EditorState.create({doc: source, extensions: markraLanguage});
        assert.equal(projectMarkdownRange(state, 0, source.length).text, text);
    });
}
test("uses full syntax context for a partially wrapped annotation", () => {
    const state = EditorState.create({doc: "甲乙**丙**丁", extensions: markraLanguage});
    assert.equal(projectMarkdownRange(state, 1, 5).text, "乙丙");
    assert.equal(projectMarkdownRange(state, 4, 5).text, "丙");
});
