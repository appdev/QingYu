import assert = require("node:assert/strict");
import test from "node:test";
import {createWorkspaceMarkdownDocumentSource} from "./documentSource";
import {
    handleMarkdownManagementCommit,
    handleMarkdownManagementPrepare,
    isMarkdownManagementPrepareActive,
    prepareMarkdownMutationAcrossRenderers,
    commitMarkdownMutationAcrossRenderers,
} from "./managementCoordinator";

const {createMarkdownManagementCoordinator} = require("../../electron/markdownManagementCoordinator.js");

test("workspace source preserves the current Markdown HTTP contract", async () => {
    const calls: Array<{url: string, body: Record<string, unknown>}> = [];
    const source = createWorkspaceMarkdownDocumentSource({
        notebookId: "box",
        path: "/note.md",
        readOnly: false,
        request: async (url, body) => {
            calls.push({url, body});
            return {code: 0, data: {name: "note.md", path: "/note.md", content: "note\n", revision: "r1", mtime: 1}};
        },
    });

    const document = await source.load();

    assert.equal(document.lineEnding, "\n");
    assert.equal(document.utf8Bom, false);
    assert.deepEqual(calls, [{url: "/api/markdown/get", body: {notebook: "box", path: "/note.md"}}]);
});

test("workspace source updates its location after moving", async () => {
    const calls: Array<{url: string, body: Record<string, unknown>}> = [];
    const source = createWorkspaceMarkdownDocumentSource({
        notebookId: "box-a",
        path: "/note.md",
        readOnly: false,
        createOperationID: () => "op-move",
        async request(url, body) {
            calls.push({url, body});
            return {code: 0, data: {operationID: body.operationID, name: "note.md", path: "/archive/note.md", content: "", revision: "r2", mtime: 2}};
        },
    });
    await source.move?.({toNotebook: "box-b", toParentPath: "/archive", revision: "r1"});
    await source.load();
    assert.deepEqual(calls, [{
        url: "/api/markdown/move",
        body: {notebook: "box-a", path: "/note.md", toNotebook: "box-b", toParentPath: "/archive", revision: "r1", operationID: "op-move"},
    }, {
        url: "/api/markdown/get",
        body: {notebook: "box-b", path: "/archive/note.md"},
    }]);
});

test("duplicating does not change the workspace source location", async () => {
    const calls: Array<{url: string, body: Record<string, unknown>}> = [];
    const source = createWorkspaceMarkdownDocumentSource({
        notebookId: "box",
        path: "/note.md",
        readOnly: false,
        createOperationID: () => "op-duplicate",
        async request(url, body) {
            calls.push({url, body});
            const duplicate = url.endsWith("/duplicate");
            return {code: 0, data: {
                name: duplicate ? "note 2.md" : "note.md",
                path: duplicate ? "/note 2.md" : "/note.md",
                content: "note\n",
                revision: "r1",
                mtime: 1,
                ...(duplicate ? {operationID: body.operationID} : {}),
            }};
        },
    });
    await source.duplicate?.("r1");
    await source.load();
    assert.deepEqual(calls, [{
        url: "/api/markdown/duplicate",
        body: {notebook: "box", path: "/note.md", revision: "r1", operationID: "op-duplicate"},
    }, {
        url: "/api/markdown/get",
        body: {notebook: "box", path: "/note.md"},
    }]);
});

test("workspace mutations prepare, echo operation IDs, and commit before changing location", async () => {
    const phases: string[] = [];
    const calls: Array<{url: string, body: Record<string, unknown>}> = [];
    let operation = 0;
    const source = createWorkspaceMarkdownDocumentSource({
        notebookId: "box-a",
        path: "/note.md",
        readOnly: false,
        createOperationID: () => `op-${++operation}`,
        prepareMutation: async (ref, operationID, revision) => {
            phases.push(`prepare:${ref.notebook}:${ref.path}:${operationID}:${revision}`);
            return {ok: true, lease: `lease-${operationID}`};
        },
        commitMutation: async (operationID, lease, mutation) => {
            phases.push(`commit:${operationID}:${lease}:${mutation.kind}:${mutation.to?.notebook}:${mutation.to?.path}`);
            return true;
        },
        async request(url, body) {
            calls.push({url, body});
            phases.push(`request:${body.operationID}`);
            const moved = url.endsWith("/move");
            return {code: 0, data: {
                operationID: body.operationID,
                name: moved ? "note.md" : "renamed.md",
                path: moved ? "/archive/note.md" : url.endsWith("/rename") ? "/renamed.md" : "/note.md",
                content: "note\n",
                revision: `r-${operation}`,
                mtime: operation,
            }};
        },
    });

    await source.save({content: "saved\n", revision: "r0"});
    await source.rename({name: "renamed.md", revision: "r-1"});
    await source.duplicate?.("r-2");
    await source.move?.({toNotebook: "box-b", toParentPath: "/archive", revision: "r-3"});
    await source.load();

    assert.deepEqual(calls.slice(0, 4).map((call) => call.body.operationID), ["op-1", "op-2", "op-3", "op-4"]);
    assert.deepEqual(phases.slice(0, -1), [
        "prepare:box-a:/note.md:op-1:r0", "request:op-1", "commit:op-1:lease-op-1:save:box-a:/note.md",
        "prepare:box-a:/note.md:op-2:r-1", "request:op-2", "commit:op-2:lease-op-2:rename:box-a:/renamed.md",
        "prepare:box-a:/renamed.md:op-3:r-2", "request:op-3", "commit:op-3:lease-op-3:duplicate:undefined:undefined",
        "prepare:box-a:/renamed.md:op-4:r-3", "request:op-4", "commit:op-4:lease-op-4:move:box-b:/archive/note.md",
    ]);
    assert.deepEqual(calls[4], {url: "/api/markdown/get", body: {notebook: "box-b", path: "/archive/note.md"}});
});

test("workspace mutation rejects a mismatched operation ID without changing source location", async () => {
    const commits: unknown[] = [];
    const calls: Array<{url: string, body: Record<string, unknown>}> = [];
    const source = createWorkspaceMarkdownDocumentSource({
        notebookId: "box-a",
        path: "/note.md",
        readOnly: false,
        createOperationID: () => "op-client",
        prepareMutation: async () => ({ok: true, lease: "lease-client"}),
        commitMutation: async (...args) => {
            commits.push(args);
            return true;
        },
        async request(url, body) {
            calls.push({url, body});
            if (url.endsWith("/move")) return {code: 0, data: {
                operationID: "op-other", name: "note.md", path: "/archive/note.md",
                content: "", revision: "r2", mtime: 2,
            }};
            return {code: 0, data: {name: "note.md", path: "/note.md", content: "", revision: "r1", mtime: 1}};
        },
    });

    const result = await source.move?.({toNotebook: "box-b", toParentPath: "/archive", revision: "r1"});
    await source.load();

    assert.deepEqual(result, {status: "error", code: "OPERATION_MISMATCH"});
    assert.equal(commits.length, 0);
    assert.deepEqual(calls[1], {url: "/api/markdown/get", body: {notebook: "box-a", path: "/note.md"}});
});

test("a dirty editor save flushes and updates the revision in every matching renderer", async () => {
    const phases: string[] = [];
    const coordinator = createMarkdownManagementCoordinator({timeout: 100});
    const initiator = {
        managementID: "editor-initiator", notebookId: "box", path: "/note.md", revision: "r1",
        flush: async () => { phases.push("initiator-save"); return true; },
        getRevision: () => "r1",
        applyWorkspaceDocumentRevision: (revision: string) => {
            initiator.revision = revision;
            phases.push("initiator-revision");
        },
    };
    const remote = {
        managementID: "editor-remote", notebookId: "box", path: "/note.md", revision: "r1",
        flush: async () => { phases.push("remote-flush"); return true; },
        getRevision: () => "r1",
        applyWorkspaceDocumentRevision: (revision: string) => {
            remote.revision = revision;
            phases.push("remote-revision");
        },
    };
    const register = (id: number, editors: typeof initiator[]) => coordinator.register(id, "workspace", async (request: any) => {
        const result = request.phase === "prepare"
            ? await handleMarkdownManagementPrepare(request, editors)
            : await handleMarkdownManagementCommit(request, editors);
        coordinator.ack(id, "workspace", result);
    });
    register(1, [initiator]);
    register(2, [remote as typeof initiator]);
    const source = createWorkspaceMarkdownDocumentSource({
        notebookId: "box",
        path: "/note.md",
        readOnly: false,
        createOperationID: () => "op-two-renderers",
        prepareMutation: (ref, operationID, revision) => coordinator.prepare(1, {
            workspace: "workspace", ref, operationID, mode: "flush", expectedRevision: revision,
            excludedEditorID: initiator.managementID,
        }),
        commitMutation: (operationID, lease, mutation) => coordinator.commit(1, {
            workspace: "workspace", operationID, lease, mutation,
        }).then((result: {ok: boolean}) => result.ok),
        request: async (_url, body) => {
            phases.push("http-save");
            return {code: 0, data: {operationID: body.operationID, name: "note.md", path: "/note.md",
                content: "saved", revision: "r2", mtime: 2}};
        },
    });

    phases.push("initiator-save");
    const result = await source.save({content: "saved", revision: "r1"});

    assert.equal(result.status, "ok");
    assert.deepEqual(phases, ["initiator-save", "remote-flush", "http-save", "initiator-revision", "remote-revision"]);
    assert.equal(initiator.revision, "r2");
    assert.equal(remote.revision, "r2");
});

test("two independently dirty renderers fail closed when the remote flush advances revision", async () => {
    const coordinator = createMarkdownManagementCoordinator({timeout: 100});
    let remoteRevision = "r1";
    let initiatorRequests = 0;
    const remoteSource = createWorkspaceMarkdownDocumentSource({
        notebookId: "box", path: "/note.md", readOnly: false,
        createOperationID: () => "op-remote-save",
        isPrepareActive: isMarkdownManagementPrepareActive,
        request: async (_url, body) => ({code: 0, data: {operationID: body.operationID, name: "note.md",
            path: "/note.md", content: "remote", revision: "r2", mtime: 2}}),
    });
    const initiator = {managementID: "initiator", notebookId: "box", path: "/note.md", revision: "r1",
        flush: async () => true, getRevision: () => "r1"};
    const remote = {managementID: "remote", notebookId: "box", path: "/note.md",
        flush: async () => {
            const saved = await remoteSource.save({content: "remote", revision: remoteRevision});
            if (saved.status === "ok") remoteRevision = saved.document.revision;
            return saved.status === "ok";
        },
        getRevision: () => remoteRevision};
    const register = (id: number, editors: any[]) => coordinator.register(id, "workspace", async (request: any) => {
        coordinator.ack(id, "workspace", request.phase === "prepare"
            ? await handleMarkdownManagementPrepare(request, editors)
            : await handleMarkdownManagementCommit(request, editors));
    });
    register(1, [initiator]);
    register(2, [remote]);
    const source = createWorkspaceMarkdownDocumentSource({
        notebookId: "box", path: "/note.md", readOnly: false,
        createOperationID: () => "op-initiator-save",
        prepareMutation: (ref, operationID, revision) => coordinator.prepare(1, {
            workspace: "workspace", ref, operationID, mode: "flush", expectedRevision: revision,
            excludedEditorID: initiator.managementID,
        }),
        request: async () => {
            initiatorRequests++;
            return {code: 0, data: {}};
        },
    });

    assert.deepEqual(await source.save({content: "initiator", revision: "r1"}),
        {status: "error", code: "HTTP_FAILED"});
    assert.equal(remoteRevision, "r2");
    assert.equal(initiatorRequests, 0);
});

test("workspace source saves a dirty initiating editor without recursive local fallback flush", async () => {
    let flushes = 0;
    let requests = 0;
    const editor = {
        managementID: "local-initiator", notebookId: "box", path: "/note.md", revision: "r1",
        flush: async () => { flushes++; throw new Error("recursive flush"); },
        getRevision: () => "r1",
        applyWorkspaceDocumentReference: (): void => undefined,
    };
    const source = createWorkspaceMarkdownDocumentSource({
        notebookId: "box", path: "/note.md", readOnly: false,
        createOperationID: () => "op-local-source",
        prepareMutation: (ref, operationID, revision) => prepareMarkdownMutationAcrossRenderers(
            undefined, "workspace", ref, [editor], operationID,
            {expectedRevision: revision, excludedEditorID: editor.managementID},
        ),
        commitMutation: (operationID, lease, mutation) => commitMarkdownMutationAcrossRenderers(
            undefined, "workspace", operationID, lease, mutation, [editor],
        ),
        request: async (_url, body) => {
            requests++;
            return {code: 0, data: {operationID: body.operationID, name: "note.md", path: "/note.md",
                content: "saved", revision: "r2", mtime: 2}};
        },
    });

    const result = await source.save({content: "saved", revision: "r1"});

    assert.equal(result.status, "ok");
    assert.equal(flushes, 0);
    assert.equal(requests, 1);
});

test("workspace source aborts its prepared lease when the HTTP client throws", async () => {
    const phases: string[] = [];
    const source = createWorkspaceMarkdownDocumentSource({
        notebookId: "box", path: "/note.md", readOnly: false,
        createOperationID: () => "op-client-throws",
        prepareMutation: async () => {
            phases.push("prepare");
            return {ok: true, lease: "lease-client-throws"};
        },
        abortMutation: async (operationID, lease) => {
            phases.push(`abort:${operationID}:${lease}`);
        },
        request: async () => {
            phases.push("request");
            throw new Error("connection closed");
        },
    });

    assert.deepEqual(await source.save({content: "saved", revision: "r1"}),
        {status: "error", code: "HTTP_FAILED"});
    assert.deepEqual(phases, ["prepare", "request", "abort:op-client-throws:lease-client-throws"]);
});
