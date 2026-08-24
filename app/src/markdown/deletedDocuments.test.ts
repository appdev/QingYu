import * as assert from "node:assert/strict";
import {JSDOM} from "jsdom";
import test from "node:test";
import {
    buildPurgeDeletedMarkdownRequest,
    buildRestoreDeletedMarkdownRequest,
    deletedMarkdownActionAllowed,
    loadDeletedMarkdown,
    previewDeletedMarkdown,
    purgeDeletedMarkdown,
    renderDeletedMarkdownList,
    setDeletedMarkdownPreview,
    resolveDeletedMarkdownTarget,
    restoreDeletedMarkdown,
} from "./deletedDocuments";

const entry = {
    id: "trash-1",
    notebook: "20260820000000-source",
    originalPath: "/notes/a.md",
    historyPath: "20260820-delete/source/notes/a.md",
    deletedAt: 1,
    size: 7,
    revision: "revision-1",
};

test("restores to the original path without overwrite", () => {
    const request = buildRestoreDeletedMarkdownRequest(entry, undefined);
    assert.deepEqual(request, {
        id: entry.id,
        toNotebook: entry.notebook,
        toParentPath: "/notes",
        name: "a.md",
    });
});

test("writes raw preview to the real readonly textarea sink and hides actions in readonly mode", () => {
    const dom = new JSDOM('<ul id="list"></ul><textarea readonly id="preview"></textarea>');
    const list = dom.window.document.querySelector("#list") as HTMLUListElement;
    const preview = dom.window.document.querySelector("#preview") as HTMLTextAreaElement;
    renderDeletedMarkdownList(list, [entry], {readonly: true, emptyText: "Empty", restoreText: "Restore", purgeText: "Purge"});
    setDeletedMarkdownPreview(preview, "<script>x</script>");
    assert.equal(preview.value, "<script>x</script>");
    assert.equal(dom.window.document.querySelector("script"), null);
    assert.equal(list.querySelector("[data-type='restoreDeletedMarkdown']"), null);
    assert.equal(list.querySelector("[data-type='purgeDeletedMarkdown']"), null);
    const action = dom.window.document.createElement("button");
    action.dataset.type = "restoreDeletedMarkdown";
    assert.equal(deletedMarkdownActionAllowed(action, true), false);
    assert.equal(deletedMarkdownActionAllowed(action, false), true);
});

test("requires a new target when the original notebook is missing", () => {
    assert.equal(resolveDeletedMarkdownTarget(entry, new Set()), null);
    assert.deepEqual(resolveDeletedMarkdownTarget(entry, new Set([entry.notebook])), {
        notebook: entry.notebook,
        parentPath: "/notes",
        name: "a.md",
    });
});

test("builds renamed restore and purge requests", () => {
    assert.deepEqual(buildRestoreDeletedMarkdownRequest(entry, {
        notebook: "20260820000000-target",
        parentPath: "/restored",
        name: "renamed.md",
    }), {id: entry.id, toNotebook: "20260820000000-target", toParentPath: "/restored", name: "renamed.md"});
    assert.deepEqual(buildPurgeDeletedMarkdownRequest(entry), {id: entry.id});
});

test("loads deleted Markdown newest first and previews raw content through the Markdown endpoint", async () => {
    const calls: unknown[][] = [];
    const request = async (url: string, body: Record<string, unknown>) => {
        calls.push([url, body]);
        if (url.endsWith("listDeleted")) return {code: 0, data: [{...entry, id: "older", deletedAt: 1}, {...entry, id: "newer", deletedAt: 2}]};
        return {code: 0, data: {content: "# raw\n<script>"}};
    };

    assert.deepEqual((await loadDeletedMarkdown(request)).map((item) => item.id), ["newer", "older"]);
    assert.equal(await previewDeletedMarkdown("newer", request), "# raw\n<script>");
    assert.deepEqual(calls, [
        ["/api/markdown/listDeleted", {}],
        ["/api/markdown/getDeleted", {id: "newer"}],
    ]);
});

test("distinguishes restore conflict from general purge failure and sends client operation IDs", async () => {
    const calls: unknown[][] = [];
    const request = async (url: string, body: Record<string, unknown>) => {
        calls.push([url, body]);
        return url.endsWith("restore") ? {code: 409, msg: "conflict", data: null as unknown} :
            {code: -1, msg: "purge failed", data: null as unknown};
    };

    assert.deepEqual(await restoreDeletedMarkdown(entry, undefined, request), {ok: false, conflict: true, message: "conflict"});
    assert.deepEqual(await purgeDeletedMarkdown(entry, request), {ok: false, conflict: false, message: "purge failed"});
    assert.equal((calls[0][1] as Record<string, unknown>).operationID === undefined, false);
    assert.equal((calls[1][1] as Record<string, unknown>).operationID === undefined, false);
});

test("returns a general failure when a history mutation request throws", async () => {
    assert.deepEqual(await purgeDeletedMarkdown(entry, async () => { throw new Error("network"); }), {
        ok: false,
        conflict: false,
        message: "network",
    });
});
