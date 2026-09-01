import * as assert from "node:assert/strict";
import test from "node:test";
import {
    createMarkdownDocument,
    duplicateMarkdownDocument,
    moveMarkdownDocument,
    renameMarkdownDocument,
    renameMarkdownDocumentTitle,
    recycleMarkdownDocument,
} from "./documentManagement";
import {readMarkdownFrontmatter} from "./markra-core/markdown/frontmatter";

const ref = {kind: "markdown" as const, notebook: "20260820000000-source", path: "/a.md"};

test("creates through a barrier with a client operation ID before commit", async () => {
    const phases: unknown[][] = [];
    const created = await createMarkdownDocument({notebook: ref.notebook, parentPath: "/", name: "Untitled.md"}, {
        createOperationID: () => "op-create",
        prepareOperation: async (operationID) => {
            phases.push(["prepare", operationID]);
            return {ok: true, lease: "lease-create"};
        },
        request: async (url, body) => {
            phases.push(["request", url, body]);
            return {code: 0, data: {operationID: body.operationID, path: "/Untitled.md", name: "Untitled.md"}};
        },
        commitMutation: async (...args) => {
            phases.push(["commit", ...args]);
            return true;
        },
    });

    assert.deepEqual(created, {operationID: "op-create", path: "/Untitled.md", name: "Untitled.md"});
    assert.deepEqual(phases, [
        ["prepare", "op-create"],
        ["request", "/api/markdown/create", {
            notebook: ref.notebook, parentPath: "/", name: "Untitled.md", operationID: "op-create",
        }],
        ["commit", "op-create", "lease-create", {kind: "create"}],
    ]);
});

test("moves with the flushed revision and migrates editors only after success", async () => {
    const requests: {url: string; body: Record<string, unknown>}[] = [];
    const migrations: unknown[][] = [];
    const result = await moveMarkdownDocument(ref, {
        notebook: "20260820000000-target",
        directory: "/notes",
    }, {
        editors: [{notebookId: ref.notebook, path: ref.path, revision: "revision-1", readOnly: false, flush: async () => true}],
        request: async (url, body) => {
            requests.push({url, body});
            return {code: 0, data: {notebook: "20260820000000-target", path: "/notes/a.md", revision: "revision-2", operationID: "op-test"}};
        },
        createOperationID: () => "op-test",
        migrate: async (...args) => { migrations.push(args); },
    });

    assert.equal(result, true);
    assert.deepEqual(requests, [{
        url: "/api/markdown/move",
        body: {
            notebook: ref.notebook,
            path: ref.path,
            revision: "revision-1",
            toNotebook: "20260820000000-target",
            toParentPath: "/notes",
            operationID: "op-test",
        },
    }]);
    assert.deepEqual(migrations, [[ref, {
        kind: "markdown",
        notebook: "20260820000000-target",
        path: "/notes/a.md",
    }, "revision-2"]]);
});

test("does not migrate or close editors when the mutation fails", async () => {
    let sideEffects = 0;
    const dependencies = {
        editors: [{notebookId: ref.notebook, path: ref.path, revision: "revision-1", readOnly: false, flush: async () => true}],
        request: async () => ({code: 409, data: null as Record<string, unknown> | null}),
        migrate: async () => { sideEffects++; },
        close: async () => { sideEffects++; },
    };

    assert.equal(await moveMarkdownDocument(ref, {notebook: ref.notebook, directory: "/notes"}, dependencies), false);
    assert.equal(await recycleMarkdownDocument(ref, dependencies), false);
    assert.equal(sideEffects, 0);
});

test("recycles with the flushed revision and closes matching editors after success", async () => {
    const requests: {url: string; body: Record<string, unknown>}[] = [];
    const closed: unknown[] = [];
    const result = await recycleMarkdownDocument(ref, {
        editors: [{notebookId: ref.notebook, path: ref.path, revision: "revision-3", readOnly: false, flush: async () => true}],
        request: async (url, body) => {
            requests.push({url, body});
            return {code: 0, data: {id: "trash-1", operationID: "op-test"}};
        },
        createOperationID: () => "op-test",
        close: async (closedRef) => { closed.push(closedRef); },
    });

    assert.equal(result, true);
    assert.deepEqual(requests, [{
        url: "/api/markdown/remove",
        body: {notebook: ref.notebook, path: ref.path, revision: "revision-3", operationID: "op-test"},
    }]);
    assert.deepEqual(closed, [ref]);
});

test("loads the current revision when the document has no open editor", async () => {
    const requests: {url: string; body: Record<string, unknown>}[] = [];
    const result = await recycleMarkdownDocument(ref, {
        editors: [],
        loadRevision: async () => "revision-on-disk",
        request: async (url, body) => {
            requests.push({url, body});
            return {code: 0, data: {id: "trash-2", operationID: "op-test"}};
        },
        createOperationID: () => "op-test",
    });

    assert.equal(result, true);
    assert.equal(requests[0].body.revision, "revision-on-disk");
});

test("duplicates only after every split editor flushes", async () => {
    let requests = 0;
    const result = await duplicateMarkdownDocument(ref, {
        editors: [
            {notebookId: ref.notebook, path: ref.path, revision: "revision-1", flush: async () => true},
            {notebookId: ref.notebook, path: ref.path, revision: "revision-1", flush: async () => false},
        ],
        request: async () => {
            requests++;
            return {code: 0, data: {path: "/a-copy.md"}};
        },
    });

    assert.equal(result, false);
    assert.equal(requests, 0);
});

test("duplicates with a client operation ID and completes the prepared lease", async () => {
    const calls: unknown[][] = [];
    const result = await duplicateMarkdownDocument(ref, {
        editors: [],
        createOperationID: () => "op-duplicate",
        prepareRevision: async () => ({ok: true, revision: "revision-7", matches: 1, lease: "lease-7"}),
        request: async (url, body) => {
            calls.push(["request", url, body]);
            return {code: 0, data: {path: "/a-copy.md", operationID: body.operationID}};
        },
        commitMutation: async (...args) => {
            calls.push(["commit", ...args]);
            return true;
        },
    });

    assert.equal(result, true);
    assert.deepEqual(calls, [["request", "/api/markdown/duplicate", {
        notebook: ref.notebook,
        path: ref.path,
        revision: "revision-7",
        operationID: "op-duplicate",
    }], ["commit", "op-duplicate", "lease-7", {kind: "duplicate"}]]);
});

test("rejects a duplicate response with a different operation ID", async () => {
    let commits = 0;
    const result = await duplicateMarkdownDocument(ref, {
        editors: [],
        createOperationID: () => "op-client",
        loadRevision: async () => "revision-8",
        request: async () => ({code: 0, data: {path: "/a-copy.md", operationID: "op-server"}}),
        commitMutation: async () => {
            commits++;
            return true;
        },
    });
    assert.equal(result, false);
    assert.equal(commits, 0);
});

test("renames once and migrates every split editor through the shared callback", async () => {
    const migrated: unknown[][] = [];
    const result = await renameMarkdownDocument(ref, "renamed.md", {
        editors: [
            {notebookId: ref.notebook, path: ref.path, revision: "revision-4", flush: async () => true},
            {notebookId: ref.notebook, path: ref.path, revision: "revision-4", flush: async () => true},
        ],
        request: async () => ({code: 0, data: {path: "/renamed.md", revision: "revision-5", operationID: "op-test"}}),
        createOperationID: () => "op-test",
        migrate: async (...args) => { migrated.push(args); },
    });

    assert.equal(result, true);
    assert.deepEqual(migrated, [[ref, {
        kind: "markdown",
        notebook: ref.notebook,
        path: "/renamed.md",
    }, "revision-5"]]);
});

test("renames a Markdown title only after saving the matching Front Matter title", async () => {
    let path = ref.path;
    let content = "Body";
    let revision = "revision-1";
    let operation = 0;
    const calls: string[] = [];
    const result = await renameMarkdownDocumentTitle(ref, "renamed.markdown", {
        editors: [],
        createOperationID: () => `operation-${++operation}`,
        loadRevision: async () => revision,
        request: async (url, body) => {
            calls.push(url);
            if (url === "/api/markdown/get") {
                return {code: 0, data: {path, content, revision}};
            }
            if (url === "/api/markdown/save") {
                assert.equal(body.revision, revision);
                content = body.content as string;
                revision = `revision-${operation + 1}`;
                return {code: 0, data: {path, content, revision, operationID: body.operationID}};
            }
            assert.equal(url, "/api/markdown/rename");
            assert.equal(body.revision, revision);
            path = "/renamed.markdown";
            return {code: 0, data: {path, content, revision, operationID: body.operationID}};
        },
    });

    assert.equal(result, true);
    assert.deepEqual(calls, ["/api/markdown/get", "/api/markdown/save", "/api/markdown/rename"]);
    assert.equal(path, "/renamed.markdown");
    const metadata = readMarkdownFrontmatter(content);
    assert.equal(metadata.status === "valid" && metadata.title, "renamed");
});

test("restores the original Front Matter when the file rename fails", async () => {
    const originalContent = "---\ntitle: Old\n---\n\nBody";
    let content = originalContent;
    let revision = "revision-1";
    let operation = 0;
    let saves = 0;
    const result = await renameMarkdownDocumentTitle(ref, "occupied.md", {
        editors: [],
        createOperationID: () => `operation-${++operation}`,
        loadRevision: async () => revision,
        request: async (url, body) => {
            if (url === "/api/markdown/get") return {code: 0, data: {content, revision}};
            if (url === "/api/markdown/save") {
                saves++;
                content = body.content as string;
                revision = `revision-${saves + 1}`;
                return {code: 0, data: {content, revision, operationID: body.operationID}};
            }
            return {code: 409, data: {revision}};
        },
    });

    assert.equal(result, false);
    assert.equal(saves, 2);
    assert.equal(content, originalContent);
});

test("does not rename a file with malformed Front Matter", async () => {
    const requests: string[] = [];
    const result = await renameMarkdownDocumentTitle(ref, "renamed.md", {
        editors: [],
        createOperationID: () => "operation-malformed",
        loadRevision: async () => "revision-1",
        request: async (url) => {
            requests.push(url);
            return {code: 0, data: {content: "---\ntitle: [\n---\nBody", revision: "revision-1"}};
        },
    });

    assert.equal(result, false);
    assert.deepEqual(requests, ["/api/markdown/get"]);
});

test("does not rename when saving the Front Matter title conflicts", async () => {
    const requests: string[] = [];
    const result = await renameMarkdownDocumentTitle(ref, "renamed.md", {
        editors: [],
        createOperationID: () => "operation-conflict",
        loadRevision: async () => "revision-1",
        request: async (url) => {
            requests.push(url);
            if (url === "/api/markdown/get") {
                return {code: 0, data: {content: "---\ntitle: Old\n---\nBody", revision: "revision-1"}};
            }
            return {code: 409, data: {revision: "revision-2"}};
        },
    });

    assert.equal(result, false);
    assert.deepEqual(requests, ["/api/markdown/get", "/api/markdown/save"]);
});

test("does not migrate split editors when rename is rejected", async () => {
    let migrations = 0;
    const result = await renameMarkdownDocument(ref, "renamed.md", {
        editors: [{notebookId: ref.notebook, path: ref.path, revision: "revision-4", flush: async () => true}],
        request: async () => ({code: 409, data: {path: "/renamed.md", revision: "revision-5"}}),
        migrate: async () => { migrations++; },
    });

    assert.equal(result, false);
    assert.equal(migrations, 0);
});

test("cancels a pending operation when the HTTP request throws", async () => {
    const result = await renameMarkdownDocument(ref, "renamed.md", {
        editors: [{notebookId: ref.notebook, path: ref.path, revision: "revision-4", flush: async () => true}],
        createOperationID: () => "op-thrown",
        request: async () => { throw new Error("network"); },
    });
    assert.equal(result, false);
});

test("aborts a prepared cross-renderer lease when a file action request throws", async () => {
    const aborted: unknown[][] = [];
    const result = await renameMarkdownDocument(ref, "renamed.md", {
        editors: [],
        createOperationID: () => "op-file-action-throws",
        prepareRevision: async () => ({ok: true, revision: "revision-9", matches: 1, lease: "lease-9"}),
        request: async () => { throw new Error("network"); },
        abortMutation: async (...args) => { aborted.push(args); },
    });

    assert.equal(result, false);
    assert.deepEqual(aborted, [["op-file-action-throws", "lease-9"]]);
});
