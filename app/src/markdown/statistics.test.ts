import assert = require("node:assert/strict");
import {test} from "node:test";
import {countMarkdownStatistics} from "./statistics";

test("counts Unicode text, Markdown links, and images", () => {
    const source = "中文 words [link](a.md) ![img](a.png)";
    assert.deepEqual(countMarkdownStatistics(source), {
        runeCount: Array.from(source).length,
        wordCount: 4,
        linkCount: 1,
        imageCount: 1,
    });
});

test("ignores escaped links and fenced examples", () => {
    assert.deepEqual(countMarkdownStatistics("\\[x](a)\n```\n[y](b)\n```"), {
        runeCount: 22, wordCount: 3, linkCount: 0, imageCount: 0,
    });
});

test("counts links and images from Markdown AST nodes only", () => {
    const source = [
        "`[inline](no)` and [real](yes)",
        "",
        "    ![indented](no.png)",
        "",
        "~~~~markdown",
        "[fenced](no)",
        "~~~~",
        "",
        "[reference][target] and ![image][asset]",
        "",
        "[target]: /target",
        "[asset]: /asset.png",
    ].join("\n");
    const result = countMarkdownStatistics(source);
    assert.equal(result.linkCount, 2);
    assert.equal(result.imageCount, 1);
});
