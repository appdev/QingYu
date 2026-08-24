import * as assert from "node:assert/strict";
import test from "node:test";
import {
    openRecentDocument,
    recentDocumentTimestamp,
    renderRecentDocumentItems,
    restoreRecentlyClosedTab,
    validateClosedMarkdownLayout,
} from "./recentDocuments";

test("opens a Markdown recent item by notebook and path", async () => {
    const calls: unknown[][] = [];
    const app = {name: "app"};
    await openRecentDocument(app, {
        kind: "markdown",
        notebook: "20260820000000-test",
        path: "/notes.md",
        title: "notes.md",
    }, {
        openMarkdown: async (...args) => { calls.push(args); },
        openNative: async () => undefined,
    });

    assert.deepEqual(calls[0].slice(1), ["20260820000000-test", "/notes.md", "notes.md"]);
});

test("renders native and Markdown recent identities without a synthetic block id", () => {
    const html = renderRecentDocumentItems([
        {kind: "native", rootID: "20260820000000-a", title: "Native"},
        {kind: "markdown", notebook: "20260820000000-box", path: "/a.md", title: "a.md"},
    ]);

    assert.match(html, /data-node-id="20260820000000-a"/u);
    assert.match(html, /data-markdown-notebook="20260820000000-box"/u);
    assert.match(html, /data-markdown-path="\/a\.md"/u);
    assert.doesNotMatch(html, /data-node-id="markdown:/u);
});

test("escapes recent-document titles and Markdown paths", () => {
    const html = renderRecentDocumentItems([{
        kind: "markdown",
        notebook: "box&quot;",
        path: "/<script>.md",
        title: "<script>x</script>",
    }]);

    assert.doesNotMatch(html, /<script>/u);
    assert.match(html, /&lt;script&gt;x&lt;\/script&gt;/u);
    assert.match(html, /data-markdown-path="\/&lt;script&gt;\.md"/u);
});

for (const field of ["viewedAt", "openAt", "closedAt", "updated"] as const) {
    test(`reads the ${field} timestamp from either recent-document branch`, () => {
        assert.equal(recentDocumentTimestamp({kind: "native", rootID: "native", title: "Native", [field]: 42}, field), 42);
        assert.equal(recentDocumentTimestamp({kind: "markdown", notebook: "box", path: "/a.md", title: "a.md", [field]: 42}, field), 42);
    });
}

test("validates a closed Markdown layout without inventing a block identity", async () => {
    const refs: unknown[] = [];
    const valid = await validateClosedMarkdownLayout({
        instance: "MarkdownEditor",
        notebookId: "box",
        path: "/a.md",
    }, async (ref) => {
        refs.push(ref);
        return true;
    });

    assert.equal(valid, true);
    assert.deepEqual(refs, [{kind: "markdown", notebook: "box", path: "/a.md"}]);
    assert.equal(await validateClosedMarkdownLayout({instance: "MarkdownEditor", notebookId: "box"}, async () => true), false);
});

test("skips stale Markdown closed tabs and restores the next valid entry", async () => {
    const closed = [
        {title: "valid.md", children: {instance: "MarkdownEditor", notebookId: "box", path: "/valid.md"}},
        {title: "missing.md", children: {instance: "MarkdownEditor", notebookId: "box", path: "/missing.md"}},
    ];
    const opened: string[] = [];
    let staleMessages = 0;
    const restored = await restoreRecentlyClosedTab({name: "app"}, closed, {
        validateMarkdown: async (ref) => ref.path === "/valid.md",
        openMarkdown: async (_app, ref) => { opened.push(ref.path); },
        restoreNative: async () => false,
        stale: () => { staleMessages++; },
    });

    assert.equal(restored, true);
    assert.equal(closed.length, 0);
    assert.deepEqual(opened, ["/valid.md"]);
    assert.equal(staleMessages, 1);
});

test("preserves native recently-closed restoration", async () => {
    const native = {title: "Native", children: {instance: "Editor", rootId: "native-root"}};
    const closed = [native];
    const restored: unknown[] = [];

    assert.equal(await restoreRecentlyClosedTab({name: "app"}, closed, {
        validateMarkdown: async () => false,
        openMarkdown: async () => undefined,
        restoreNative: async (_app, value) => { restored.push(value); return true; },
        stale: () => undefined,
    }), true);
    assert.deepEqual(restored, [native]);
    assert.equal(closed.length, 0);
});
