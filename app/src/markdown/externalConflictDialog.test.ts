import assert = require("node:assert/strict");
import test from "node:test";
import {handleExternalMarkdownConflictChoice} from "./externalConflictDialog";

test("cancel preserves local content and overwrite is scoped to the reported revision", async () => {
    const calls: string[] = [];
    const callbacks = {
        cancel: async () => { calls.push("cancel"); },
        overwrite: async (revision: string) => { calls.push(`overwrite:${revision}`); },
        reload: async () => { calls.push("reload"); },
    };

    await handleExternalMarkdownConflictChoice("cancel", "disk-r2", callbacks);
    await handleExternalMarkdownConflictChoice("overwrite", "disk-r2", callbacks);

    assert.deepEqual(calls, ["cancel", "overwrite:disk-r2"]);
});
