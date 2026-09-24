import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorState} from "@codemirror/state";
import {EditorView} from "@codemirror/view";
import {liveMarkdown} from "./index";
import {installMarkdownTestDom} from "../../markraTestDom";

let cleanup: (() => void) | undefined;
let views: EditorView[] = [];

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => {
    views.forEach((view) => view.destroy());
    views = [];
    cleanup?.();
    cleanup = undefined;
});

function createView(doc: string, paragraphSpacing = 0) {
    const view = new EditorView({
        parent: document.body,
        state: EditorState.create({doc, extensions: [liveMarkdown({paragraphSpacing})]}),
    });
    views.push(view);
    return view;
}

test("uses measured spacers instead of enlarging editable heading lines", () => {
    const view = createView("# Title\n\n## Section\n\nBody");
    const spacers = [...view.dom.querySelectorAll<HTMLElement>(".cm-markra-heading-spacer")];
    assert.deepEqual(spacers.map((spacer) => [spacer.dataset.headingEdge, spacer.style.height]), [
        ["after", "16px"],
        ["before", "28px"],
        ["after", "12px"],
    ]);
    assert.equal(spacers.every((spacer) => spacer.getAttribute("aria-hidden") === "true"), true);
});

test("adds paragraph and separated blockquote spacers", () => {
    const view = createView("First\n\nSecond\n\n> Quote", 14);
    assert.equal(view.dom.querySelector<HTMLElement>(".cm-markra-paragraph-spacer")?.style.height, "14px");
    assert.equal(view.dom.querySelector<HTMLElement>(".cm-markra-blockquote-spacer")?.style.height, "10px");
});

test("rebuilds spacing when an inserted paragraph changes block structure", () => {
    const doc = "First\n\n\n\nLast";
    const view = createView(doc, 14);
    assert.equal(view.dom.querySelectorAll(".cm-markra-paragraph-spacer").length, 1);
    const insertion = view.state.doc.line(3).from;
    view.dispatch({changes: {from: insertion, insert: "Middle"}, userEvent: "input.type"});
    assert.equal(view.dom.querySelectorAll(".cm-markra-paragraph-spacer").length, 2);
});
