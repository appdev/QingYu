import assert = require("node:assert/strict");
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import test from "node:test";
import {runInNewContext} from "node:vm";
import * as ts from "typescript";
import type {NotebookRootListing} from "./types";

const listing = (ids: string[]): NotebookRootListing => ({
    notebook: "box",
    name: "Notebook",
    icon: "",
    sortMode: 0,
    documents: ids.map((documentID) => ({
        kind: "sy", notebook: "box", path: `/${documentID}.sy`, documentID,
        identityState: "valid", identityConflict: false, revision: "", cardRatio: 1,
        title: documentID, previewText: "", icon: "", created: 0, updated: 0, size: 0, sort: 0, subFileCount: 0,
    })),
});

const compile = (source: string) => ts.transpileModule(source, {
    compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2021},
}).outputText;

const createHarness = (ids = ["deleted", "kept"]) => {
    const requests: Array<(response: {code: number, data?: NotebookRootListing}) => void> = [];
    const rendered: string[][] = [];
    const exports = {} as {NotebookRoot: {prototype: object}};
    // 保留实际类的方法，只替换网络和界面依赖，以便控制响应顺序。
    runInNewContext(compile(readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8")), {
        exports,
        require: (name: string) => {
            if (name === "../layout/Model") return {Model: class {}};
            if (name === "../util/fetch") {
                return {fetchPost: (_url: string, _data: unknown, callback: typeof requests[number]) => requests.push(callback)};
            }
            return {};
        },
    });
    const root = Object.assign(Object.create(exports.NotebookRoot.prototype), {
        notebookId: "box", listing: listing(ids), destroyed: false, reloadSequence: 0, reloadPending: false,
        renderShell() {
            rendered.push(root.listing.documents.map((document: {documentID: string}) => document.documentID));
        },
    });
    return {root, requests, rendered};
};

test("native deletion removes cards immediately and reloads the listing", async () => {
    const {root, requests, rendered} = createHarness();
    root.handleEvent({cmd: "removeDoc", data: {ids: ["deleted", "child"]}});
    assert.deepEqual(rendered, [["kept"]]);
    assert.equal(requests.length, 1);
    requests[0]({code: 0, data: listing(["kept"])});
    assert.deepEqual(root.listing.documents.map((document: {documentID: string}) => document.documentID), ["kept"]);
});

test("sync deletions share card removal and leave Markdown cards intact", () => {
    const {root, requests, rendered} = createHarness();
    root.listing.documents.push({...listing(["deleted"]).documents[0], kind: "markdown", path: "/kept.md"});
    root.handleEvent({cmd: "syncMergeResult", data: {removeRootIDs: ["deleted"]}});
    assert.deepEqual(rendered, [["kept", "deleted"]]);
    assert.equal(requests.length, 1);
});

test("unrelated deletions do not reload an idle notebook", () => {
    const {root, requests, rendered} = createHarness();
    root.handleEvent({cmd: "removeDoc", data: {ids: ["other"]}});
    root.handleEvent({cmd: "syncMergeResult", data: {removeRootIDs: []}});
    assert.equal(requests.length, 0);
    assert.equal(rendered.length, 0);
});

test("a deletion invalidates the initial request even before cards are loaded", async () => {
    const {root, requests, rendered} = createHarness([]);
    const initial = root.reload();
    root.handleEvent({cmd: "removeDoc", data: {ids: ["deleted"]}});
    assert.equal(requests.length, 2);
    requests[1]({code: 0, data: listing(["kept"])});
    requests[0]({code: 0, data: listing(["deleted", "kept"])});
    await initial;
    assert.deepEqual(rendered, [["kept"]]);
});

test("consecutive deletions reject older responses and keep cards removed after reload failure", () => {
    const {root, requests, rendered} = createHarness(["first", "second", "kept"]);
    root.handleEvent({cmd: "removeDoc", data: {ids: ["first"]}});
    root.handleEvent({cmd: "removeDoc", data: {ids: ["second"]}});
    requests[1]({code: -1});
    requests[0]({code: 0, data: listing(["second", "kept"])});
    assert.deepEqual(rendered, [["second", "kept"], ["kept"]]);
    assert.equal(root.reloadPending, false);
});

test("Markdown events still reload only the affected notebook", () => {
    const {root, requests} = createHarness();
    root.handleEvent({cmd: "removeMarkdown", data: {box: "other"}});
    assert.equal(requests.length, 0);
    root.handleEvent({cmd: "removeMarkdown", data: {box: "box"}});
    assert.equal(requests.length, 1);
});

test("a closed card page ignores pending responses and deletion notifications", async () => {
    const {root, requests, rendered} = createHarness();
    const pending = root.reload();
    root.destroyed = true;
    root.handleEvent({cmd: "removeDoc", data: {ids: ["deleted"]}});
    requests[0]({code: 0, data: listing(["kept"])});
    await pending;
    assert.equal(requests.length, 1);
    assert.equal(rendered.length, 0);
});

for (const file of ["src/index.ts", "src/window/index.ts", "src/mobile/util/onMessage.ts"]) {
    test(`${file} forwards local and sync deletions to the card page`, () => {
        const source = readFileSync(resolve(process.cwd(), file), "utf8");
        const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
        const branches = new Map<string, string>();
        const visit = (node: ts.Node) => {
            if (ts.isCaseBlock(node)) {
                node.clauses.forEach((clause, index) => {
                    if (!ts.isCaseClause(clause) || !ts.isStringLiteral(clause.expression)) return;
                    const command = clause.expression.text;
                    if (!["removeDoc", "syncMergeResult"].includes(command)) return;
                    const statements: string[] = [];
                    for (let i = index; i < node.clauses.length; i++) {
                        statements.push(...node.clauses[i].statements.map((statement) => statement.getText(ast)));
                        if (node.clauses[i].statements.length) break;
                    }
                    branches.set(command, statements.join("\n"));
                });
            }
            ts.forEachChild(node, visit);
        };
        visit(ast);
        for (const cmd of ["removeDoc", "syncMergeResult"]) {
            const data = {cmd, data: {ids: ["deleted"], removeRootIDs: ["deleted"]}};
            const received: unknown[] = [];
            const root = {notebookRoot: true, handleEvent: (event: unknown) => received.push(event)};
            assert.ok(branches.has(cmd));
            runInNewContext(compile(`switch (data.cmd) {case "${cmd}": ${branches.get(cmd)}}`), {
                data, app: {}, window: {siyuan: {config: {}}},
                getAllModels: () => ({notebookRoot: [root], markdown: [] as unknown[]}), getAllTabs: (): unknown[] => [],
                getMobileMarkdownEditor: () => root, reloadSync: (): void => undefined,
            });
            assert.deepEqual(received, [data]);
        }
    });
}
