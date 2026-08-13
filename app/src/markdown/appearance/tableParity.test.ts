import assert = require("node:assert/strict");
import test from "node:test";
import {getAppearanceContract} from "./contracts";
import {findIndependentBaseThemeDeclarations} from "./testSupport";

test("table contract covers content and every visible control state", () => {
    const table = getAppearanceContract("block.table");
    const toolbar = getAppearanceContract("control.table-toolbar");
    const button = getAppearanceContract("control.table-button");
    assert.ok(table);
    assert.ok(toolbar);
    assert.ok(button);
    assert.deepEqual(toolbar.states, ["default", "hover", "focus", "selected", "disabled"]);
    assert.deepEqual(button.states, ["default", "hover", "focus", "selected", "disabled"]);
    assert.equal(toolbar.reference.selector, ".block__icons");
    assert.equal(button.reference.selector, ".block__icons .block__icon");
    assert.ok(table.geometry.includes("width"));
    assert.ok(table.geometry.includes("height"));
    assert.deepEqual(findIndependentBaseThemeDeclarations(table.markdownSelector), []);
});
