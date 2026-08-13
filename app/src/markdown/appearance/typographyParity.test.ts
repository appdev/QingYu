import assert = require("node:assert/strict");
import test from "node:test";
import {getAppearanceContract} from "./contracts";
import {findIndependentBaseThemeDeclarations} from "./testSupport";

for (const id of [
    "block.paragraph",
    "block.heading-1",
    "block.heading-2",
    "block.heading-3",
    "block.heading-4",
    "block.heading-5",
    "block.heading-6",
    "block.list",
    "block.task",
    "block.blockquote",
    "block.horizontal-rule",
    "inline.strong",
    "inline.emphasis",
    "inline.strikethrough",
    "inline.highlight",
    "inline.code",
    "inline.link",
    "inline.math",
]) {
    test(`${id} has no independent CodeMirror visual source`, () => {
        const contract = getAppearanceContract(id);
        assert.ok(contract);
        assert.deepEqual(findIndependentBaseThemeDeclarations(contract.markdownSelector), []);
    });
}
