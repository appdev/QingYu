import * as assert from "node:assert/strict";
import {JSDOM} from "jsdom";
import test from "node:test";
import {
    classifyMarkdownDrop,
    flushMarkdownDocumentEditors,
    markdownFileTreeDragAttributes,
    orderedFileTreePaths,
    routeMarkdownFileTreeDrop,
} from "./documentManagement";

const ref = {kind: "markdown" as const, notebook: "20260820000000-test", path: "/a.md"};

const fakeEditor = (options: {
    notebook?: string;
    path?: string;
    revision?: string;
    readOnly?: boolean;
    flush?: () => Promise<boolean>;
}) => ({
    notebookId: options.notebook ?? ref.notebook,
    path: options.path ?? ref.path,
    readOnly: options.readOnly ?? false,
    revision: options.revision ?? "revision-1",
    flush: options.flush ?? (async () => true),
});

test("fails closed when one split Markdown editor cannot flush", async () => {
    const result = await flushMarkdownDocumentEditors(ref, [
        fakeEditor({}),
        fakeEditor({flush: async () => false}),
    ]);

    assert.equal(result, null);
});

test("returns the common revision only after every matching editor flushes", async () => {
    let unrelatedFlushes = 0;
    const result = await flushMarkdownDocumentEditors(ref, [
        fakeEditor({revision: "revision-2"}),
        fakeEditor({revision: "revision-2"}),
        fakeEditor({path: "/other.md", flush: async () => {
            unrelatedFlushes++;
            return true;
        }}),
    ]);

    assert.equal(result, "revision-2");
    assert.equal(unrelatedFlushes, 0);
});

test("fails closed for readonly, thrown, or disagreeing split editors", async () => {
    assert.equal(await flushMarkdownDocumentEditors(ref, [fakeEditor({readOnly: true})]), null);
    assert.equal(await flushMarkdownDocumentEditors(ref, [fakeEditor({flush: async () => {
        throw new Error("save failed");
    }})]), null);
    assert.equal(await flushMarkdownDocumentEditors(ref, [
        fakeEditor({revision: "revision-1"}),
        fakeEditor({revision: "revision-2"}),
    ]), null);
});

test("classifies same-directory drag as sort and cross-directory drag as move", () => {
    assert.equal(classifyMarkdownDrop(ref, {notebook: ref.notebook, directory: "/"}), "sort");
    assert.equal(classifyMarkdownDrop(ref, {notebook: ref.notebook, directory: "/folder"}), "move");
    assert.equal(classifyMarkdownDrop(ref, {notebook: "20260820000000-other", directory: "/"}), "move");
});

test("rejects Markdown as a parent and preserves mixed sibling order", () => {
    assert.equal(classifyMarkdownDrop(ref, {
        notebook: ref.notebook,
        directory: "/b.md",
        kind: "markdown",
    }), "reject");
    assert.deepEqual(orderedFileTreePaths([
        {kind: "native", path: "/20260820000000-a.sy"},
        {kind: "markdown", path: "/b.md"},
    ]), ["/20260820000000-a.sy", "/b.md"]);
});

test("makes workspace Markdown rows participate in desktop drag events", () => {
    const dom = new JSDOM(`<ul><li ${markdownFileTreeDragAttributes()}></li></ul>`);
    const row = dom.window.document.querySelector("li") as HTMLLIElement;

    assert.equal(row.draggable, true);
    assert.equal(row.dataset.docType, "markdown");
});

test("routes same-directory mixed siblings to one complete sort update", async () => {
    const calls: unknown[][] = [];
    const result = await routeMarkdownFileTreeDrop({
        sources: [
            {kind: "native", notebook: "box", path: "/native.sy"},
            {kind: "markdown", notebook: "box", path: "/notes.md"},
        ],
        target: {kind: "native", notebook: "box", directory: "/", mode: "sibling"},
        orderedPaths: ["/notes.md", "/native.sy"],
    }, {
        sort: async (...args) => { calls.push(["sort", ...args]); return true; },
        moveMarkdown: async (...args) => { calls.push(["markdown", ...args]); return true; },
        moveNative: async (...args) => { calls.push(["native", ...args]); return true; },
    });

    assert.deepEqual(result, {ok: true});
    assert.deepEqual(calls, [["sort", "box", ["/notes.md", "/native.sy"]]]);
});

test("rejects cross-directory mixed selections before any mutation and refreshes every affected directory", async () => {
    const calls: unknown[][] = [];
    const result = await routeMarkdownFileTreeDrop({
        sources: [
            {kind: "native", notebook: "source", path: "/native.sy"},
            {kind: "markdown", notebook: "source", path: "/notes.md"},
        ],
        target: {kind: "native", notebook: "target", directory: "/folder.sy", mode: "child"},
        orderedPaths: [],
    }, {
        sort: async () => true,
        moveMarkdown: async (...args) => { calls.push(["markdown", ...args]); return true; },
        moveNative: async (...args) => { calls.push(["native", ...args]); return true; },
    });

    assert.deepEqual(result, {ok: false, reason: "unsafe-mixed"});
    assert.deepEqual(calls, []);
});

test("rejects multiple Markdown cross-directory moves and refreshes every affected directory", async () => {
    const refreshed: unknown[][] = [];
    const result = await routeMarkdownFileTreeDrop({
        sources: [
            {kind: "native", notebook: "source", path: "/one/native.sy"},
            {kind: "markdown", notebook: "source", path: "/one/notes.md"},
            {kind: "markdown", notebook: "other", path: "/two/more.md"},
        ],
        target: {kind: "native", notebook: "target", directory: "/folder.sy", mode: "child"},
        orderedPaths: [],
    }, {
        sort: async () => true,
        moveMarkdown: async () => true,
        moveNative: async () => true,
        refresh: async (...args) => { refreshed.push(args); },
    });

    assert.deepEqual(result, {ok: false, reason: "unsafe-mixed"});
    assert.deepEqual(refreshed, [
        ["source", "/one"],
        ["other", "/two"],
        ["target", "/folder.sy"],
    ]);
});

test("rejects a child drop onto a Markdown file", async () => {
    let called = false;
    const result = await routeMarkdownFileTreeDrop({
        sources: [{kind: "markdown", notebook: "box", path: "/a.md"}],
        target: {kind: "markdown", notebook: "box", directory: "/b.md", mode: "child"},
        orderedPaths: [],
    }, {
        sort: async () => { called = true; return true; },
        moveMarkdown: async () => { called = true; return true; },
        moveNative: async () => { called = true; return true; },
    });

    assert.deepEqual(result, {ok: false, reason: "invalid-target"});
    assert.equal(called, false);
});

test("refreshes source and destination and returns failure when a move throws", async () => {
    const refreshed: string[] = [];
    const result = await routeMarkdownFileTreeDrop({
        sources: [{kind: "markdown", notebook: "source", path: "/one/a.md"}],
        target: {kind: "native", notebook: "target", directory: "/folder.sy", mode: "child"},
        orderedPaths: [],
    }, {
        sort: async () => true,
        moveNative: async () => true,
        moveMarkdown: async () => { throw new Error("network"); },
        refresh: async (notebook, directory) => { refreshed.push(`${notebook}:${directory}`); },
    });
    assert.deepEqual(result, {ok: false, reason: "failed"});
    assert.deepEqual(refreshed, ["source:/one", "target:/folder.sy"]);
});
