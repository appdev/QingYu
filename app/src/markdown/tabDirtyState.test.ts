import assert = require("node:assert/strict");
import {test} from "node:test";
import {isTabModelUnmodified} from "./tabDirtyState";

const protyle = (updated: boolean) => ({kind: "protyle", editor: {protyle: {updated}}});
const markdown = (updated: boolean) => ({kind: "markdown", hasUnsavedChanges: () => updated});
const isProtyle = (value: unknown): value is ReturnType<typeof protyle> => (value as {kind?: string})?.kind === "protyle";
const isMarkdown = (value: unknown): value is ReturnType<typeof markdown> => (value as {kind?: string})?.kind === "markdown";

test("reports native and Markdown tab dirty state without treating other models as modified", () => {
    assert.equal(isTabModelUnmodified(protyle(true), isProtyle, isMarkdown), false);
    assert.equal(isTabModelUnmodified(protyle(false), isProtyle, isMarkdown), true);
    assert.equal(isTabModelUnmodified(markdown(true), isProtyle, isMarkdown), false);
    assert.equal(isTabModelUnmodified(markdown(false), isProtyle, isMarkdown), true);
    assert.equal(isTabModelUnmodified({}, isProtyle, isMarkdown), true);
});
