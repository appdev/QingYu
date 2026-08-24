import * as assert from "node:assert/strict";
import test from "node:test";
import {
    applyMarkdownManagementEvent,
    applyMarkdownManagementEventToRuntime,
    beginMarkdownManagementOperation,
    completeMarkdownManagementOperation,
    createMarkdownManagementEventState,
    createMarkdownManagementRuntimeEventController,
    failMarkdownManagementOperation,
    isMarkdownManagementOperationHandled,
    markdownReferenceFromInitData,
    markdownReferenceFromLayout,
    MarkdownManagementEventState,
    renameMarkdownDocument,
    subscribeMarkdownManagementOperationReplay,
} from "./documentManagement";

test("reads only workspace Markdown references from restored tab data", () => {
    assert.deepEqual(markdownReferenceFromLayout({
        instance: "MarkdownEditor",
        notebookId: "box",
        path: "/a.md",
    }), {kind: "markdown", notebook: "box", path: "/a.md"});
    assert.equal(markdownReferenceFromLayout({
        instance: "MarkdownEditor",
        externalCapabilityId: "external",
    }), null);
    assert.equal(markdownReferenceFromInitData("{invalid"), null);
    assert.equal(markdownReferenceFromInitData(JSON.stringify({instance: "Editor", notebookId: "box", path: "/a.md"})), null);
});

const initialState = (): MarkdownManagementEventState => ({
    operationIDs: [],
    openDocuments: [{kind: "markdown", notebook: "source", path: "/a.md"}],
    recentDocuments: [{kind: "markdown", notebook: "source", path: "/a.md"}],
    closedDocuments: [
        {kind: "markdown", notebook: "source", path: "/a.md"},
        {kind: "native", rootID: "20260820000000-native"},
    ],
});

test("applies a Markdown move event exactly once with old and new references", () => {
    const event = {
        cmd: "renameMarkdown",
        kind: "markdown" as const,
        box: "target",
        path: "/b.md",
        oldBox: "source",
        oldPath: "/a.md",
        operationID: "op-1",
        time: 1,
    };

    const result = applyMarkdownManagementEvent(initialState(), event);

    assert.equal(result.duplicate, false);
    assert.deepEqual(result.refreshDirectories, ["source:/", "target:/"]);
    assert.deepEqual(result.state.openDocuments, [{kind: "markdown", notebook: "target", path: "/b.md"}]);
    assert.deepEqual(result.state.recentDocuments, [{kind: "markdown", notebook: "target", path: "/b.md"}]);
    assert.deepEqual(result.state.closedDocuments, [
        {kind: "markdown", notebook: "target", path: "/b.md"},
        {kind: "native", rootID: "20260820000000-native"},
    ]);

    const duplicate = applyMarkdownManagementEvent(result.state, event);
    assert.equal(duplicate.duplicate, true);
    assert.deepEqual(duplicate.refreshDirectories, []);
    assert.deepEqual(duplicate.state, result.state);
});

test("suppresses a broadcast that races ahead of the initiating HTTP success", () => {
    beginMarkdownManagementOperation("op-http-race");
    assert.equal(isMarkdownManagementOperationHandled("op-http-race"), true);
    completeMarkdownManagementOperation("op-http-race");
    assert.equal(isMarkdownManagementOperationHandled("op-http-race"), true);
});

test("an initiating broadcast refreshes a tree once while reference consumers stay deduplicated", () => {
    const event = {
        cmd: "renameMarkdown", kind: "markdown", box: "target", path: "/renamed.md",
        oldBox: "source", oldPath: "/a.md", operationID: "op-split-consumers", time: 11,
    };
    beginMarkdownManagementOperation(event.operationID);

    const referenceResult = applyMarkdownManagementEvent(initialState(), event);
    const firstTreeResult = applyMarkdownManagementEvent(createMarkdownManagementEventState(), event, {
        consumeInitiatingOperation: true,
    });
    completeMarkdownManagementOperation(event.operationID);
    const repeatedTreeResult = applyMarkdownManagementEvent(firstTreeResult.state, event, {
        consumeInitiatingOperation: true,
    });
    const lateReferenceResult = applyMarkdownManagementEvent(initialState(), event);

    assert.equal(referenceResult.applied, false);
    assert.equal(firstTreeResult.applied, true);
    assert.deepEqual(firstTreeResult.refreshDirectories, ["source:/", "target:/"]);
    assert.equal(repeatedTreeResult.duplicate, true);
    assert.equal(lateReferenceResult.applied, false);
});

test("replays cached server truth when HTTP or response parsing fails", () => {
    const replayed: string[] = [];
    const unsubscribe = subscribeMarkdownManagementOperationReplay((event) => replayed.push(event.operationID));
    beginMarkdownManagementOperation("op-failed-http");
    const event = {
        cmd: "renameMarkdown", kind: "markdown", box: "target", path: "/b.md",
        oldBox: "source", oldPath: "/a.md", operationID: "op-failed-http", time: 12,
    };
    const pending = applyMarkdownManagementEvent(initialState(), event);
    assert.equal(pending.duplicate, true);
    failMarkdownManagementOperation("op-failed-http");
    unsubscribe();
    assert.deepEqual(replayed, ["op-failed-http"]);
});

test("the renderer event controller replays pending server truth through its real runtime sink", async () => {
    const migrated: string[] = [];
    const controller = createMarkdownManagementRuntimeEventController(() => ({
        editors: [{
            notebookId: "source",
            path: "/a.md",
            applyWorkspaceDocumentReference: (notebook: string, path: string) => migrated.push(`${notebook}:${path}`),
        }],
        closedTabs: [],
    }));
    beginMarkdownManagementOperation("op-controller-failure");
    controller.handle({
        cmd: "renameMarkdown", kind: "markdown", box: "target", path: "/b.md",
        oldBox: "source", oldPath: "/a.md", operationID: "op-controller-failure", time: 13,
    });
    assert.deepEqual(migrated, []);

    failMarkdownManagementOperation("op-controller-failure");
    await controller.settled();
    controller.destroy();

    assert.deepEqual(migrated, ["target:/b.md"]);
    assert.deepEqual(controller.state().openDocuments, []);
});

test("applies an initiating HTTP rename once and skips the matching broadcast", async () => {
    let migrations = 0;
    let racedDuplicate = false;
    const renamed = await renameMarkdownDocument(
        {kind: "markdown", notebook: "source", path: "/http.md"},
        "renamed.md",
        {
            editors: [{notebookId: "source", path: "/http.md", revision: "r1", flush: async () => true}],
            createOperationID: () => "op-http-success",
            request: async (_url, body) => {
                racedDuplicate = applyMarkdownManagementEvent(initialState(), {
                    cmd: "renameMarkdown", kind: "markdown", box: "source", path: "/renamed.md",
                    oldBox: "source", oldPath: "/http.md", operationID: body.operationID as string, time: 10,
                }).duplicate;
                return {code: 0, data: {path: "/renamed.md", revision: "r2", operationID: body.operationID}};
            },
            migrate: () => { migrations++; },
        },
    );
    const laterBroadcast = applyMarkdownManagementEvent(initialState(), {
        cmd: "renameMarkdown", kind: "markdown", box: "source", path: "/renamed.md",
        oldBox: "source", oldPath: "/http.md", operationID: "op-http-success", time: 10,
    });
    assert.equal(renamed, true);
    assert.equal(migrations, 1);
    assert.equal(racedDuplicate, true);
    assert.equal(laterBroadcast.duplicate, true);
});

test("removes recycled Markdown references without changing the following native closed item", () => {
    const result = applyMarkdownManagementEvent(initialState(), {
        cmd: "removeMarkdown",
        kind: "markdown",
        box: "source",
        path: "/a.md",
        oldBox: "",
        oldPath: "",
        operationID: "op-recycle",
        time: 2,
    });

    assert.deepEqual(result.refreshDirectories, ["source:/"]);
    assert.deepEqual(result.state.openDocuments, []);
    assert.deepEqual(result.state.recentDocuments, []);
    assert.deepEqual(result.state.closedDocuments, [{kind: "native", rootID: "20260820000000-native"}]);
});

test("refreshes only the affected directory for restore and sort events", () => {
    const restored = applyMarkdownManagementEvent(initialState(), {
        cmd: "createMarkdown",
        kind: "markdown",
        box: "target",
        path: "/restored/a.md",
        oldBox: "",
        oldPath: "",
        operationID: "op-restore",
        time: 3,
    });
    assert.deepEqual(restored.refreshDirectories, ["target:/restored"]);

    const sorted = applyMarkdownManagementEvent(restored.state, {
        cmd: "sortMarkdown",
        kind: "markdown",
        box: "target",
        path: "/restored/a.md",
        oldBox: "",
        oldPath: "",
        operationID: "op-sort",
        time: 4,
    });
    assert.deepEqual(sorted.refreshDirectories, ["target:/restored"]);

    const purged = applyMarkdownManagementEvent(sorted.state, {
        cmd: "purgeMarkdown",
        kind: "markdown",
        box: "source",
        path: "/deleted.md",
        oldBox: "",
        oldPath: "",
        operationID: "op-purge",
        time: 5,
    });
    assert.equal(purged.applied, true);
    assert.deepEqual(purged.refreshDirectories, []);
});

test("ignores malformed and non-Markdown file events", () => {
    const state = initialState();
    const malformed = applyMarkdownManagementEvent(state, {
        cmd: "renameMarkdown",
        kind: "markdown",
        box: "target",
        path: "/b.md",
        oldBox: "source",
        oldPath: "/a.md",
        operationID: "",
        time: 1,
    });
    const native = applyMarkdownManagementEvent(state, {
        cmd: "renameMarkdown",
        kind: "native",
        box: "target",
        path: "/b.md",
        oldBox: "source",
        oldPath: "/a.md",
        operationID: "op-native",
        time: 1,
    });

    assert.equal(malformed.applied, false);
    assert.equal(native.applied, false);
    assert.deepEqual(malformed.state, state);
    assert.deepEqual(native.state, state);
});

test("migrates an open editor and closed layout once when response and broadcast race", () => {
    const editor = {
        notebookId: "source",
        path: "/a.md",
        revision: "revision-1",
        migrations: 0,
        applyWorkspaceDocumentReference(notebook: string, path: string, revision: string) {
            this.notebookId = notebook;
            this.path = path;
            this.revision = revision;
            this.migrations++;
        },
    };
    const closedTabs: unknown[] = [
        {children: {instance: "MarkdownEditor", notebookId: "source", path: "/a.md"}},
        {children: {instance: "Editor", rootId: "20260820000000-native"}},
    ];
    const event = {
        cmd: "renameMarkdown",
        kind: "markdown" as const,
        box: "target",
        path: "/b.md",
        oldBox: "source",
        oldPath: "/a.md",
        operationID: "op-runtime-move",
        time: 5,
    };

    const first = applyMarkdownManagementEventToRuntime(initialState(), event, {editors: [editor], closedTabs});
    const second = applyMarkdownManagementEventToRuntime(first.state, event, {editors: [editor], closedTabs});

    assert.equal(editor.migrations, 1);
    assert.equal(editor.notebookId, "target");
    assert.equal(editor.path, "/b.md");
    assert.deepEqual(closedTabs, [
        {children: {instance: "MarkdownEditor", notebookId: "target", path: "/b.md"}},
        {children: {instance: "Editor", rootId: "20260820000000-native"}},
    ]);
    assert.equal(second.duplicate, true);
});

test("closes a recycled open editor and removes only its stale closed layouts", () => {
    const editor = {
        notebookId: "source",
        path: "/a.md",
        closed: false,
        close() {
            this.closed = true;
        },
    };
    const closedTabs: unknown[] = [
        {children: {instance: "MarkdownEditor", notebookId: "source", path: "/a.md"}},
        {children: {instance: "Editor", rootId: "20260820000000-native"}},
    ];

    applyMarkdownManagementEventToRuntime(initialState(), {
        cmd: "removeMarkdown",
        kind: "markdown",
        box: "source",
        path: "/a.md",
        oldBox: "",
        oldPath: "",
        operationID: "op-runtime-remove",
        time: 6,
    }, {editors: [editor], closedTabs});

    assert.equal(editor.closed, true);
    assert.deepEqual(closedTabs, [{children: {instance: "Editor", rootId: "20260820000000-native"}}]);
});

test("removes a closed layout written asynchronously while a recycled tab closes", async () => {
    const closedTabs: unknown[] = [{children: {instance: "Editor", rootId: "20260820000000-native"}}];
    const editor = {
        notebookId: "source",
        path: "/a.md",
        async close() {
            await Promise.resolve();
            closedTabs.push({children: {instance: "MarkdownEditor", notebookId: "source", path: "/a.md"}});
        },
    };

    const result = applyMarkdownManagementEventToRuntime(initialState(), {
        cmd: "removeMarkdown",
        kind: "markdown",
        box: "source",
        path: "/a.md",
        oldBox: "",
        oldPath: "",
        operationID: "op-runtime-async-remove",
        time: 7,
    }, {editors: [editor], closedTabs});
    await result.settled;

    assert.deepEqual(closedTabs, [{children: {instance: "Editor", rootId: "20260820000000-native"}}]);
});
