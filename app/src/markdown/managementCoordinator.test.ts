import * as assert from "node:assert/strict";
import test from "node:test";
import {
    handleMarkdownManagementCommit,
    handleMarkdownManagementPrepare,
    installMarkdownManagementRendererCoordinator,
    prepareMarkdownMutationAcrossRenderers,
} from "./managementCoordinator";

const ref = {kind: "markdown" as const, notebook: "box", path: "/a.md"};

test("renderer prepare flushes all matching split editors and reports their common revision", async () => {
    let flushes = 0;
    const result = await handleMarkdownManagementPrepare({phase: "prepare", generation: 1, operationID: "op", ref, mode: "flush"}, [
        {notebookId: "box", path: "/a.md", revision: "r1", flush: async () => { flushes++; return true; }},
        {notebookId: "box", path: "/a.md", revision: "r1", flush: async () => { flushes++; return true; }},
        {notebookId: "box", path: "/other.md", revision: "r2", flush: async () => { throw new Error("must not flush"); }},
    ]);
    assert.deepEqual(result, {phase: "prepare", mode: "flush", generation: 1, operationID: "op", ok: true, matched: true, matches: 2, revision: "r1"});
    assert.equal(flushes, 2);
});

test("renderer prepare fails closed for readonly and reports no-match independently", async () => {
    assert.deepEqual(await handleMarkdownManagementPrepare({phase: "prepare", generation: 1, operationID: "readonly", ref, mode: "flush"}, [
        {notebookId: "box", path: "/a.md", readOnly: true, revision: "r1", flush: async () => true},
    ]), {phase: "prepare", mode: "flush", generation: 1, operationID: "readonly", ok: false, matched: true, matches: 1});
    assert.deepEqual(await handleMarkdownManagementPrepare({phase: "prepare", generation: 1, operationID: "none", ref, mode: "flush"}, []), {
        phase: "prepare", mode: "flush", generation: 1, operationID: "none", ok: true, matched: false, matches: 0,
    });
});

test("presence reports matching editor instances without flushing", async () => {
    const result = await handleMarkdownManagementPrepare({phase: "prepare", generation: 1, operationID: "presence", ref, mode: "presence"}, [
        {notebookId: "box", path: "/a.md", readOnly: true, flush: async () => { throw new Error("must not flush"); }},
        {notebookId: "box", path: "/a.md", flush: async () => false},
    ]);
    assert.deepEqual(result, {phase: "prepare", mode: "presence", generation: 1, operationID: "presence", ok: true, matched: true, matches: 2});
});

test("prepare excludes the initiating editor while flushing another dirty renderer instance", async () => {
    let initiatorFlushes = 0;
    let splitFlushes = 0;
    const result = await handleMarkdownManagementPrepare({
        phase: "prepare", generation: 1, operationID: "save", ref, mode: "flush", excludedEditorID: "initiator",
    }, [
        {managementID: "initiator", notebookId: "box", path: "/a.md", revision: "r1", flush: async () => { initiatorFlushes++; return true; }},
        {managementID: "split", notebookId: "box", path: "/a.md", revision: "r1", flush: async () => { splitFlushes++; return true; }},
    ]);
    assert.equal(result.ok, true);
    assert.equal(initiatorFlushes, 0);
    assert.equal(splitFlushes, 1);
});

test("browser fallback excludes the initiating editor before flushing local splits", async () => {
    let initiatorFlushes = 0;
    let splitFlushes = 0;
    const result = await prepareMarkdownMutationAcrossRenderers(undefined, "workspace", ref, [{
        managementID: "initiator", notebookId: "box", path: "/a.md", revision: "r1",
        flush: async () => { initiatorFlushes++; return true; },
    }, {
        managementID: "split", notebookId: "box", path: "/a.md", revision: "r1",
        flush: async () => { splitFlushes++; return true; },
    }], "op-local", {expectedRevision: "r1", excludedEditorID: "initiator"});

    assert.deepEqual(result, {ok: true, revision: "r1", matches: 1, lease: "op-local"});
    assert.equal(initiatorFlushes, 0);
    assert.equal(splitFlushes, 1);
});

test("commit migrates every matching editor and acknowledges only after asynchronous closes", async () => {
    const calls: unknown[][] = [];
    const editor = {
        managementID: "split", notebookId: "box", path: "/a.md", revision: "r1", flush: async () => true,
        applyWorkspaceDocumentReference(notebook: string, path: string, revision: string) {
            calls.push([notebook, path, revision]);
            this.notebookId = notebook;
            this.path = path;
        },
    };
    assert.deepEqual(await handleMarkdownManagementCommit({
        phase: "commit", generation: 2, operationID: "move",
        mutation: {kind: "move", from: ref, to: {...ref, notebook: "target", path: "/b.md"}, revision: "r2"},
    }, [editor]), {phase: "commit", generation: 2, operationID: "move", ok: true});
    assert.deepEqual(calls, [["target", "/b.md", "r2"]]);
});

test("commit waits until every matching editor closes after a remove", async () => {
    const closed: string[] = [];
    const result = await handleMarkdownManagementCommit({
        phase: "commit",
        generation: 2,
        operationID: "remove",
        mutation: {kind: "remove", from: ref},
    }, [{
        notebookId: "box",
        path: "/a.md",
        flush: async () => true,
        async close() {
            await Promise.resolve();
            closed.push("/a.md");
        },
    }]);

    assert.deepEqual(result, {phase: "commit", generation: 2, operationID: "remove", ok: true});
    assert.deepEqual(closed, ["/a.md"]);
});

test("renderer coordinator registers again when main signals load completion", () => {
    const sent: Array<[string, unknown]> = [];
    const listeners = new Map<string, (_event: unknown, payload: any) => void>();
    const ipc = {
        invoke: async () => ({ok: true, matches: 0}),
        send: (channel: string, payload: unknown) => sent.push([channel, payload]),
        on: (channel: string, listener: (_event: unknown, payload: any) => void) => listeners.set(channel, listener),
    };
    installMarkdownManagementRendererCoordinator(ipc, "workspace", () => []);

    listeners.get("siyuan-markdown-management-ready")?.({}, undefined);

    assert.deepEqual(sent, [
        ["siyuan-markdown-management-register", {workspace: "workspace"}],
        ["siyuan-markdown-management-register", {workspace: "workspace"}],
    ]);
});
