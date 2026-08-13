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
