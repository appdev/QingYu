import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markraTestDom";
import {getAppearanceContract} from "./contracts";
import {mountTestEditor} from "./testSupport";

let cleanup: () => void;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => cleanup());

test("source mode exposes themed gutters while visual mode keeps Protyle geometry", () => {
    const source = mountTestEditor("source");
    const visual = mountTestEditor("visual");
    try {
        assert.equal(source.view.dom.dataset.markdownMode, "source");
        assert.ok(source.view.dom.querySelector(".cm-gutters"));
        assert.equal(visual.view.dom.dataset.markdownMode, "visual");
        assert.equal(visual.view.dom.querySelector(".cm-gutters"), null);
    } finally {
        source.destroy();
        visual.destroy();
    }
});

test("covers shell and editor foundation states", () => {
    for (const id of [
        "shell.document",
        "shell.metadata",
        "shell.title",
        "editor.visual",
        "editor.source",
        "editor.cursor",
        "editor.selection",
        "editor.active-line",
        "editor.gutter",
        "editor.placeholder",
        "editor.scroller",
        "editor.drag-indicator",
        "editor.error",
    ]) {
        assert.ok(getAppearanceContract(id), id);
    }
});

test("measures visual editor text and padding from their owning elements", () => {
    const visual = getAppearanceContract("editor.visual");
    assert.equal(visual?.markdownPropertyReferences?.fontFamily.selector, ".cm-content");
    assert.equal(visual?.markdownPropertyReferences?.lineHeight.selector, ".cm-content");
    assert.equal(visual?.markdownPropertyReferences?.paddingLeft.selector, ".markdown-editor__body");
    assert.equal(visual?.markdownPropertyReferences?.paddingTop.selector, ".markdown-editor__body");

    const scroller = getAppearanceContract("editor.scroller");
    assert.equal(scroller?.styleProperties.includes("fontFamily"), false);
    assert.equal(scroller?.styleProperties.includes("lineHeight"), false);
});

test("renders multi-line callout shadows from their outer edges", () => {
    const source = require("node:fs").readFileSync(
        require("node:path").resolve(process.cwd(), "src/assets/scss/business/_markdown.scss"),
        "utf8",
    );

    assert.match(source, /box-shadow:\s*var\(--markra-callout-shadow-inline/u);
    assert.match(source, /\.cm-line\.cm-markra-callout\s*\{[\s\S]*margin-block:\s*0;/u);
    assert.match(source, /markra-callout-first\s*\{[\s\S]*margin-top:\s*2px;/u);
    assert.match(source, /markra-callout-last\s*\{[\s\S]*margin-bottom:\s*2px;/u);
    assert.match(source, /markra-callout-first[\s\S]*box-shadow:\s*var\(--markra-callout-shadow-first/u);
    assert.match(source, /markra-callout-last[\s\S]*box-shadow:\s*var\(--markra-callout-shadow-last/u);
    assert.match(
        source,
        /markra-callout-first\.markra-callout-last[\s\S]*box-shadow:\s*var\(--markra-callout-shadow/u,
    );
});
