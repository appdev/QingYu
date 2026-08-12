import assert = require("node:assert/strict");
import {test} from "node:test";
import {flushMarkdownEditors, handleMarkdownSaveBarrier, trackMarkdownFlush} from "./saveBarrier";

test("reports success only after every Markdown editor is flushed", async () => {
    const calls: string[] = [];
    const success = await flushMarkdownEditors([
        {async flush() {
            calls.push("first");
            return true;
        }},
        {async flush() {
            calls.push("second");
            return true;
        }},
    ]);

    assert.equal(success, true);
    assert.deepEqual(calls.sort(), ["first", "second"]);
});

test("fails closed when an editor cannot save or throws", async () => {
    assert.equal(await flushMarkdownEditors([
        {async flush() {
            return true;
        }},
        {async flush() {
            return false;
        }},
    ]), false);

    assert.equal(await flushMarkdownEditors([
        {async flush() {
            throw new Error("save failed");
        }},
    ]), false);
});

test("deduplicates a barrier and acknowledges only after the editor flushes", async () => {
    const originalFetch = globalThis.fetch;
    const calls: string[] = [];
    globalThis.fetch = (async (_input: string | URL | Request, init?: RequestInit) => {
        calls.push(`ack:${init?.body}`);
        return new Response("{}");
    }) as typeof fetch;
    try {
        const editor = {async flush() {
            calls.push("flush");
            return true;
        }};
        const first = handleMarkdownSaveBarrier({id: "barrier"}, "desktop", [editor]);
        const second = handleMarkdownSaveBarrier({id: "barrier"}, "desktop", [editor]);
        assert.equal(first, second);
        await first;
        assert.deepEqual(calls, [
            "flush",
            'ack:{"id":"barrier","sessionId":"desktop","success":true}',
        ]);
    } finally {
        globalThis.fetch = originalFetch;
    }
});

test("waits for a save that remains in flight after its editor closes", async () => {
    let finish: (success: boolean) => void;
    const save = new Promise<boolean>((resolve) => {
        finish = resolve;
    });
    trackMarkdownFlush(save);

    let settled = false;
    const barrier = flushMarkdownEditors([]).then((success) => {
        settled = true;
        return success;
    });
    await Promise.resolve();
    assert.equal(settled, false);
    finish(true);
    assert.equal(await barrier, true);
});
