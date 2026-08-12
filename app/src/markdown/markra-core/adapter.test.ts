import assert = require("node:assert/strict");
import test from "node:test";
import {EditorState} from "@codemirror/state";
import {markdownHostAdapter, readMarkdownHostAdapter, type MarkdownHostAdapter} from "./adapter";

const createAdapter = (): MarkdownHostAdapter => ({
    createIcon: () => document.createElementNS("http://www.w3.org/2000/svg", "svg"),
    notifyError: () => undefined,
    openLink: () => undefined,
    positionPopover: () => undefined,
    renderMath: () => document.createElement("span"),
    renderMermaid: async () => document.createElement("div"),
    resolveImageSource: (source) => source,
    saveClipboardAssets: async () => [],
});

test("stores one Markdown host adapter in EditorState", () => {
    const adapter = createAdapter();
    const state = EditorState.create({extensions: [markdownHostAdapter(adapter)]});
    assert.equal(readMarkdownHostAdapter(state), adapter);
});
