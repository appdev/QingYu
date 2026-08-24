import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorSelection, EditorState} from "@codemirror/state";
import {EditorView} from "@codemirror/view";
import {installMarkdownTestDom} from "../../markraTestDom";
import {formattingPlugin, liveMarkdown, runMarkraCommand} from "./index";

let cleanup: () => void;
const views: EditorView[] = [];
beforeEach(() => cleanup = installMarkdownTestDom());
afterEach(() => { views.splice(0).forEach((view) => view.destroy()); cleanup(); });

const createView = (doc: string, from: number, to: number) => {
    const view = new EditorView({
        parent: document.body,
        state: EditorState.create({doc, selection: EditorSelection.range(from, to), extensions: [
            liveMarkdown({plugins: [formattingPlugin()]}),
        ]}),
    });
    views.push(view);
    return view;
};

test("normalizes two bold spans into one wrapper", () => {
    const doc = "Before **start** plain **end** after";
    const view = createView(doc, doc.indexOf("start"), doc.indexOf("end") + 3);
    assert.equal(runMarkraCommand(view, "format.bold"), true);
    assert.equal(view.state.doc.toString(), "Before **start plain end** after");
    assert.equal(runMarkraCommand(view, "format.bold"), true);
    assert.equal(view.state.doc.toString(), "Before start plain end after");
});

test("preserves nested italic while normalizing bold", () => {
    const selected = "plain ***marked*** tail";
    const view = createView(`Before ${selected} after`, 7, 7 + selected.length);
    assert.equal(runMarkraCommand(view, "format.bold"), true);
    assert.equal(view.state.doc.toString(), "Before **plain *marked* tail** after");
});

test("formats every cursor selection independently", () => {
    const view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc: "one two",
            selection: EditorSelection.create([EditorSelection.range(0, 3), EditorSelection.range(4, 7)], 0),
            extensions: [EditorState.allowMultipleSelections.of(true), liveMarkdown({plugins: [formattingPlugin()]})],
        }),
    });
    views.push(view);
    assert.equal(runMarkraCommand(view, "format.bold"), true);
    assert.equal(view.state.doc.toString(), "**one** **two**");
});

test("does not treat escaped or inline-code markers as an existing bold wrapper", () => {
    const escaped = "\\*literal\\*";
    const escapedView = createView(escaped, 0, escaped.length);
    assert.equal(runMarkraCommand(escapedView, "format.bold"), true);
    assert.equal(escapedView.state.doc.toString(), `**${escaped}**`);
    const code = "`**literal**`";
    const codeView = createView(code, 0, code.length);
    assert.equal(runMarkraCommand(codeView, "format.bold"), true);
    assert.equal(codeView.state.doc.toString(), `**${code}**`);
});

test("toggles strikethrough and highlight wrappers", () => {
    for (const [command, marker] of [["format.strikethrough", "~~"], ["format.highlight", "=="]] as const) {
        const view = createView("marked", 0, 6);
        assert.equal(runMarkraCommand(view, command), true);
        assert.equal(view.state.doc.toString(), `${marker}marked${marker}`);
        assert.equal(runMarkraCommand(view, command), true);
        assert.equal(view.state.doc.toString(), "marked");
    }
});
