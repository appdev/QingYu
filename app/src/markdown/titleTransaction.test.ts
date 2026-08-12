import assert = require("node:assert/strict");
import test from "node:test";
import {runMarkdownTitleTransaction} from "./titleTransaction";

test("saves Front Matter before renaming the Markdown file", async () => {
    const calls: string[] = [];
    const result = await runMarkdownTitleTransaction({
        applyTitle(title) {
            calls.push(`apply:${title}`);
            return true;
        },
        async flush() {
            calls.push("flush");
            return true;
        },
        metadataTitle: "New",
        previousTitle: "Old",
        async rename() {
            calls.push("rename");
            return true;
        },
        renameRequired: true,
    });
    assert.equal(result, true);
    assert.deepEqual(calls, ["apply:New", "flush", "rename"]);
});

test("does not rename after a failed save", async () => {
    let renamed = false;
    const result = await runMarkdownTitleTransaction({
        applyTitle: () => true,
        flush: async () => false,
        metadataTitle: "New",
        previousTitle: "Old",
        rename: async () => {
            renamed = true;
            return true;
        },
        renameRequired: true,
    });
    assert.equal(result, false);
    assert.equal(renamed, false);
});

test("restores and saves the previous title after rename failure", async () => {
    const calls: string[] = [];
    const result = await runMarkdownTitleTransaction({
        applyTitle(title) {
            calls.push(`apply:${title}`);
            return true;
        },
        async flush() {
            calls.push("flush");
            return true;
        },
        metadataTitle: "New",
        previousTitle: "Old",
        async rename() {
            calls.push("rename");
            return false;
        },
        renameRequired: true,
    });
    assert.equal(result, false);
    assert.deepEqual(calls, ["apply:New", "flush", "rename", "apply:Old", "flush"]);
});

