import assert = require("node:assert/strict");
import test from "node:test";
import {readFileSync} from "node:fs";
import {resolve} from "node:path";

test("notebook root drag reorders only under custom sort", async () => {
    (globalThis as typeof globalThis & {SIYUAN_VERSION: string}).SIYUAN_VERSION = "test";
    (globalThis as typeof globalThis & {NODE_ENV: string}).NODE_ENV = "test";
    const {classifyNotebookRootDrop, isNotebookRootMoveTarget} = await import("./rules");
    const source = {notebook: "source"};
    assert.equal(classifyNotebookRootDrop(source, {notebook: "source", root: true}, 6), "reorder");
    assert.equal(classifyNotebookRootDrop(source, {notebook: "source", root: true}, 0), "spring-back");
    assert.equal(classifyNotebookRootDrop(source, {notebook: "target", root: true}, 0), "move");
    assert.equal(classifyNotebookRootDrop(source, {notebook: "target", root: false}, 0), "move");
    assert.equal(isNotebookRootMoveTarget("source", "target"), true);
    assert.equal(isNotebookRootMoveTarget("source", "source"), false);
    assert.equal(isNotebookRootMoveTarget("", "target"), false);
});

test("notebook root cards do not present document merge targets", () => {
    const dragSource = readFileSync(resolve(process.cwd(), "src/notebookRoot/drag.ts"), "utf8");
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const filesSource = readFileSync(resolve(process.cwd(), "src/layout/dock/Files.ts"), "utf8");
    assert.doesNotMatch(dragSource, /notebook-root__document--drop-target/);
    assert.doesNotMatch(styles, /&--drop-target/);
    assert.match(dragSource, /querySelectorAll\("\.file-tree__notebook-drop-target"\)/);
    assert.match(filesSource, /file-tree__notebook-drop-target/);
    assert.match(filesSource, /NOTEBOOK_ROOT_DOCUMENT_MIME/);
});
