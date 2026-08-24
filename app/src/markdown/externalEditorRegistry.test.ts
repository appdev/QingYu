import assert = require("node:assert/strict");
import test from "node:test";
import {ExternalEditorRegistry} from "./externalEditorRegistry";

test("only one editor can claim an external capability in the same renderer", () => {
    const registry = new ExternalEditorRegistry<object>();
    const first = {};
    const second = {};

    assert.equal(registry.claim("cap-1", first), undefined);
    assert.equal(registry.claim("cap-1", second), first);
    assert.equal(registry.release("cap-1", second), false);
    assert.equal(registry.claim("cap-1", second), first);
    assert.equal(registry.release("cap-1", first), true);
    assert.equal(registry.claim("cap-1", second), undefined);
});
