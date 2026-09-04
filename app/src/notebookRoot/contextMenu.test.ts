import assert = require("node:assert/strict");
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import test from "node:test";

const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/contextMenu.ts"), "utf8");

const functionBody = (name: string, nextName: string) => source.slice(
    source.indexOf(`const ${name}`),
    source.indexOf(`const ${nextName}`),
);

const assertOrder = (body: string, patterns: string[]) => {
    let previous = -1;
    patterns.forEach((pattern) => {
        const current = body.indexOf(pattern);
        assert.ok(current > previous, `${pattern} should follow the previous menu action`);
        previous = current;
    });
};

test("notebook root context menu reuses native and Markdown operations", () => {
    assert.match(source, /renameMarkdownFile\(document\.notebook, document\.path\)/);
    assert.match(source, /moveMarkdownFile\(document\.notebook, document\.path\)/);
    assert.match(source, /removeMarkdownFile\(document\.notebook, document\.path\)/);
    assert.match(source, /createMarkdownExportMenu/);
    assert.match(source, /copySubMenu\(\[document\.documentID\]/);
    assert.match(source, /movePathToMenu\(\[document\.path\]\)/);
    assert.match(source, /exportMd\(document\.documentID\)/);
    assert.match(source, /deleteFile\(document\.notebook, document\.path\)/);
});

test("notebook root context menu keeps the common first-level action order", () => {
    assertOrder(functionBody("appendMarkdownItems", "appendNativeItems"), [
        "renameMarkdownFile(",
        'id: "copy"',
        "moveMarkdownFile(",
        "createMarkdownExportMenu(",
        'id: "separator_delete"',
        'id: "deleteMarkdown"',
    ]);
    assertOrder(functionBody("appendNativeItems", "openNotebookRootContextMenu"), [
        "renameMenu({",
        'id: "copy"',
        "movePathToMenu([document.path])",
        "exportMd(document.documentID)",
        'id: "separator_delete"',
        'id: "delete"',
    ]);
});

test("notebook root context menu filters write actions in read-only mode", () => {
    assert.equal(source.match(/const readonly = window\.siyuan\.config\.readonly/g)?.length, 2);
    assert.match(source, /if \(!readonly\) \{[\s\S]*?renameMarkdownFile/);
    assert.match(source, /if \(!readonly\) \{[\s\S]*?renameMenu/);
    assert.match(source, /if \(!readonly\) \{[\s\S]*?separator_delete[\s\S]*?deleteMarkdown/);
    assert.match(source, /if \(!readonly\) \{[\s\S]*?separator_delete[\s\S]*?deleteFile/);
});
