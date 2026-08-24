import assert = require("node:assert/strict");
import {test} from "node:test";
import {ActiveMarkdownOutlines} from "./activeMarkdownOutlines";

test("falls back to the older outline when the newest instance for a source closes", () => {
    const outlines = new ActiveMarkdownOutlines<object>();
    const first = {id: "first"};
    const second = {id: "second"};
    outlines.register("source", first);
    outlines.register("source", second);
    assert.equal(outlines.get("source"), second);
    outlines.unregister("source", second);
    assert.equal(outlines.get("source"), first);
    outlines.unregister("source", first);
    assert.equal(outlines.get("source"), undefined);
});

test("moves only the migrating outline to its new source", () => {
    const outlines = new ActiveMarkdownOutlines<object>();
    const first = {id: "first"};
    const second = {id: "second"};
    outlines.register("before", first);
    outlines.register("before", second);
    outlines.migrate("before", "after", second);
    assert.equal(outlines.get("before"), first);
    assert.equal(outlines.get("after"), second);
});
