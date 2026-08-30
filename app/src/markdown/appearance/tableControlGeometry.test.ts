import assert = require("node:assert/strict");
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {test} from "node:test";

const source = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_markdown.scss"), "utf8");
const blockDragSource = readFileSync(resolve(process.cwd(), "src/markdown/markra-core/codemirror/block-drag.ts"), "utf8");
const foldToggleSource = readFileSync(resolve(process.cwd(), "src/markdown/markra-core/codemirror/fold-toggle.ts"), "utf8");
const tableSource = readFileSync(resolve(process.cwd(), "src/markdown/markra-core/codemirror/table.ts"), "utf8");

test("table controls keep a 28px hit target and activate pointer events with their toolbar", () => {
    const control = source.match(/\.markra-table-control \{([\s\S]*?)\n {2}\}/u)?.[1] || "";
    const toolbarControl = source.match(/\.markra-table-align-controls \.markra-table-control \{([\s\S]*?)\n {2}\}/u)?.[1] || "";
    assert.match(control, /height: var\(--b3-editor-appearance-control-table-button-height, 28px\)/u);
    assert.match(control, /width: var\(--b3-editor-appearance-control-table-button-width, 28px\)/u);
    assert.match(control, /pointer-events: none/u);
    assert.match(toolbarControl, /position: static/u);
    assert.match(source, /\.cm-markra-table-wrap\.markra-table-controls-visible \.markra-table-align-controls \{[\s\S]*?pointer-events: auto/u);
    assert.match(source, /\.markra-table-add-column \{[\s\S]*?right: \.375em;[\s\S]*?top: 50%;/u);
    assert.match(source, /\.markra-table-add-row \{[\s\S]*?bottom: \.375em;[\s\S]*?left: 50%;/u);
    assert.match(tableSource, /align: "start",\s*gap: 2,\s*placement: "top"/u);
    assert.match(tableSource, /markra-table-controls-visible/u);
});

test("table controls reserve highlight styling for active and keyboard-focus states", () => {
    assert.match(source, /\.markra-table-control:hover:not\(:focus-visible\):not\(\[aria-pressed="true"\]\):not\(\[aria-expanded="true"\]\) \{[\s\S]*?background-color: var\(--b3-editor-appearance-control-table-button-background-color, transparent\);[\s\S]*?color: var\(--b3-editor-appearance-control-table-button-color, var\(--b3-theme-on-surface\)\);/u);
});

test("fold and block controls own separate start-of-line geometry slots", () => {
    assert.match(blockDragSource, /side: -2,[\s\S]*?new BlockToolbarWidget/u);
    assert.match(foldToggleSource, /side: -1,[\s\S]*?new FoldToggleWidget/u);
    assert.doesNotMatch(blockDragSource, /side: -1,[\s\S]*?new BlockToolbarWidget/u);
});
