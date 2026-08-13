import assert = require("node:assert/strict");
import test from "node:test";
import {getAppearanceContract} from "./contracts";
import {findIndependentBaseThemeDeclarations} from "./testSupport";

test("table contract covers content and every visible control state", () => {
    const table = getAppearanceContract("block.table");
    const toolbar = getAppearanceContract("control.table-toolbar");
    assert.ok(table);
    assert.ok(toolbar);
    assert.deepEqual(toolbar.states, ["default", "hover", "focus", "selected", "disabled"]);
    assert.ok(table.geometry.includes("width"));
    assert.ok(table.geometry.includes("height"));
    assert.deepEqual(findIndependentBaseThemeDeclarations(table.markdownSelector), []);
});
