import assert = require("node:assert/strict");
import {existsSync, readFileSync} from "node:fs";
import {resolve} from "node:path";
import {test} from "node:test";
import {ensureSyntaxTree} from "@codemirror/language";
import {EditorState} from "@codemirror/state";
import {findCodeMirrorMathRanges, markraLanguage} from "./markra-core/codemirror";
import {parseMarkdownCalloutMarker} from "./markra-core/shared";

const fixtureDirectory = resolve(process.cwd(), "testdata/markdown");
const fixturePath = resolve(fixtureDirectory, "format-showcase.md");
const source = readFileSync(fixturePath, "utf8");

const expectedCaseIds = [
    "D01", "D02", "D03", "D04", "D05",
    "H01", "H02", "H03", "H04", "H05", "H06", "H07", "H08", "H09", "H10",
    "I01", "I02", "I03", "I04", "I05", "I06", "I07", "I08", "I09", "I10",
    "A01", "A02", "A03", "A04", "A05", "A06",
    "L01", "L02", "L03", "L04", "L05", "L06", "L07", "L08",
    "Q01", "Q02", "Q03", "Q04", "Q05", "Q06", "Q07", "Q08",
    "C01", "C02", "C03", "C04", "C05",
    "R01", "R02", "R03", "R04",
    "T01", "T02", "T03", "T04", "T05", "T06",
    "K01", "K02", "K03", "K04", "K05", "K06", "K07", "K08",
    "M01", "M02", "M03", "M04",
    "P01", "P02", "P03", "P04", "P05",
    "X01", "X02", "X03", "X04",
];

const requiredNodeNames = new Set([
    "ATXHeading1", "ATXHeading2", "ATXHeading3", "ATXHeading4", "ATXHeading5", "ATXHeading6",
    "StrongEmphasis", "Emphasis", "Strikethrough", "Highlight",
    "InlineCode", "Link", "URL", "BulletList", "OrderedList", "Task", "Blockquote", "HorizontalRule",
    "Table", "FencedCode", "CodeBlock", "Image", "HTMLBlock", "HTMLTag",
]);

test("keeps the Markdown format showcase inventory stable and unique", () => {
    const actualCaseIds = Array.from(source.matchAll(/\b([A-Z]\d{2})-/gu), (match) => match[1]);
    assert.equal(new Set(actualCaseIds).size, actualCaseIds.length);
    assert.deepEqual(actualCaseIds, expectedCaseIds);
});

test("covers the supported Markdown syntax inventory", () => {
    const state = EditorState.create({doc: source, extensions: [markraLanguage]});
    const actualNodeNames = new Set<string>();
    const tree = ensureSyntaxTree(state, source.length, 1_000);
    assert.ok(tree, "format showcase syntax tree did not finish within one second");
    tree.iterate({enter: (node) => {
        actualNodeNames.add(node.name);
    }});

    for (const nodeName of requiredNodeNames) {
        assert.ok(actualNodeNames.has(nodeName), `missing syntax node: ${nodeName}`);
    }
    assert.equal(actualNodeNames.has("SetextHeading1"), false);
    assert.equal(actualNodeNames.has("SetextHeading2"), false);
    assert.equal(findCodeMirrorMathRanges(state).length, 4);
});

test("keeps callout and local-image probes deterministic", () => {
    const calloutTypes = source.split("\n")
        .map((line) => /^>\s*(.*)$/u.exec(line)?.[1] ?? "")
        .map((line) => parseMarkdownCalloutMarker(line)?.type)
        .filter((type): type is NonNullable<typeof type> => type !== undefined);

    assert.deepEqual(calloutTypes, ["note", "tip", "important", "warning", "caution"]);
    assert.equal(/!\[[^\]]*\]\(https?:/u.test(source), false);
    assert.equal(existsSync(resolve(fixtureDirectory, "format-showcase.svg")), true);
});
