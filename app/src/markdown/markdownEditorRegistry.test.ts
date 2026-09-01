import assert = require("node:assert/strict");
import {test} from "node:test";
import {flushMarkdownEditors, MarkdownEditorRegistration, MarkdownEditorRegistry} from "./markdownEditorRegistry";

test("migrates source subscribers atomically when an editor is renamed", () => {
    const registry = new MarkdownEditorRegistry<object>();
    const editor = {};
    const events: Array<[object | undefined, string | undefined]> = [];
    registry.register("workspace:box:/before.md", editor);
    registry.subscribe("workspace:box:/before.md", (value, migratedKey) => events.push([value, migratedKey]));
    registry.register("workspace:box:/after.md", editor);
    assert.deepEqual(events, [
        [editor, undefined],
        [editor, "workspace:box:/after.md"],
    ]);
    assert.equal(registry.get("workspace:box:/before.md"), undefined);
    assert.equal(registry.get("workspace:box:/after.md"), editor);
});

test("invalidates subscribers when the owning editor is destroyed", () => {
    const registry = new MarkdownEditorRegistry<object>();
    const editor = {};
    registry.register("source", editor);
    const events: Array<object | undefined> = [];
    registry.subscribe("source", (value) => events.push(value));
    registry.unregister(editor);
    assert.deepEqual(events, [editor, undefined]);
});

test("keeps split editors for one source and falls back when the newest closes", () => {
    const registry = new MarkdownEditorRegistry<object>();
    const first = {id: "first"};
    const second = {id: "second"};
    const events: Array<object | undefined> = [];
    registry.register("source", first);
    registry.register("source", second);
    registry.subscribe("source", (value) => events.push(value));
    assert.equal(registry.get("source"), second);
    registry.unregister(second);
    assert.equal(registry.get("source"), first);
    assert.deepEqual(events, [second, first]);
    registry.unregister(first);
    assert.equal(registry.get("source"), undefined);
    assert.deepEqual(events, [second, first, undefined]);
});

test("migrating one split editor leaves subscribers on the remaining source instance", () => {
    const registry = new MarkdownEditorRegistry<object>();
    const first = {id: "first"};
    const second = {id: "second"};
    const events: Array<[object | undefined, string | undefined]> = [];
    registry.register("before", first);
    registry.register("before", second);
    registry.subscribe("before", (value, migratedKey) => events.push([value, migratedKey]));
    registry.register("after", second);
    assert.equal(registry.get("before"), first);
    assert.equal(registry.get("after"), second);
    assert.deepEqual(events, [[second, undefined], [first, undefined]]);
});

test("does not resurrect an editor when an async save registers after destroy", () => {
    const registry = new MarkdownEditorRegistry<object>();
    const editor = {};
    const registration = new MarkdownEditorRegistration(registry, editor);
    registration.register("before");
    registration.destroy();
    registration.register("after");
    assert.equal(registry.get("before"), undefined);
    assert.equal(registry.get("after"), undefined);
});

test("export barrier flushes every matching editor and reports a conflict", async () => {
    const calls: string[] = [];
    const result = await flushMarkdownEditors([{
        focusPosition() {},
        setOutlineOpen() {},
        subscribeOutline: () => () => undefined,
        async flushForExport() {
            calls.push("first");
            return true;
        },
    }, {
        focusPosition() {},
        setOutlineOpen() {},
        subscribeOutline: () => () => undefined,
        async flushForExport() {
            calls.push("second");
            return false;
        },
    }]);
    assert.deepEqual(calls, ["first", "second"]);
    assert.equal(result, false);
});
