import assert = require("node:assert/strict");
import {test} from "node:test";
import {refreshMarkdownEditorsForConfigMessage} from "./configMessageRefresh";

test("refreshes every Markdown editor for readonly and setConf messages only", () => {
    const calls: string[] = [];
    const editors = [
        {refreshEditorConfig: () => calls.push("first")},
        {refreshEditorConfig: () => calls.push("second")},
    ];
    assert.equal(refreshMarkdownEditorsForConfigMessage("readonly", editors), true);
    assert.equal(refreshMarkdownEditorsForConfigMessage("setConf", editors), true);
    assert.equal(refreshMarkdownEditorsForConfigMessage("progress", editors), false);
    assert.deepEqual(calls, ["first", "second", "first", "second"]);
});
