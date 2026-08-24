import assert = require("node:assert/strict");
import test from "node:test";
import {prepareExternalMarkdownEditorTransfer, prepareExternalMarkdownEditorsForExit} from "./externalMarkdownClose";

test("exit preparation stops at the first editor that cannot close", async () => {
    const calls: string[] = [];
    const editors = [
        {async flushForExit() { calls.push("first"); return true; }},
        {async flushForExit() { calls.push("second"); return false; }},
        {async flushForExit() { calls.push("third"); return true; }},
    ];

    const result = await prepareExternalMarkdownEditorsForExit(editors);

    assert.equal(result, false);
    assert.deepEqual(calls, ["first", "second"]);
});

test("window transfer releases the capability only after close preparation succeeds", async () => {
    const calls: string[] = [];
    const transferable = {
        async prepareClose() { calls.push("prepare"); return true; },
        async releaseExternalCapability() { calls.push("release"); },
    };

    assert.equal(await prepareExternalMarkdownEditorTransfer(transferable), true);
    assert.deepEqual(calls, ["prepare", "release"]);

    calls.length = 0;
    assert.equal(await prepareExternalMarkdownEditorTransfer({
        async prepareClose() { calls.push("blocked"); return false; },
        async releaseExternalCapability() { calls.push("unexpected-release"); },
    }), false);
    assert.deepEqual(calls, ["blocked"]);
});
