import assert = require("node:assert/strict");
import {test} from "node:test";
import {stackAnnotationCards} from "./sidebar";
import {installMarkdownTestDom} from "../markraTestDom";
import {EditorState} from "@codemirror/state";
import {EditorView} from "@codemirror/view";
import {annotationExtension, annotationField} from "./extension";
import {MarkdownAnnotationSidebar, type AnnotationLabels} from "./sidebar";

test("dense annotation cards do not overlap or extend above the panel", () => {
    assert.deepEqual(stackAnnotationCards([{id: "a", top: -30, height: 90}, {id: "b", top: 20, height: 110},
        {id: "c", top: 500, height: 60}]).map((item) => item.top), [0, 98, 500]);
});

test("read-only documents cannot create annotations", () => {
    const cleanup = installMarkdownTestDom();
    const root = document.body.appendChild(document.createElement("div"));
    const view = new EditorView({doc: "甲乙丙", parent: root, selection: {anchor: 0, head: 2},
        extensions: [annotationExtension(), EditorState.readOnly.of(true)]});
    const labels = Object.fromEntries(["title", "add", "orphan", "reattach", "select", "all", "margin", "edit", "remove", "save", "cancel", "close"].map((key) => [key, key])) as unknown as AnnotationLabels;
    const sidebar = new MarkdownAnnotationSidebar(view, root, root, labels, () => assert.fail("read-only source changed"));
    try {
        assert.equal(sidebar.add(), false);
        assert.equal(view.state.field(annotationField).length, 0);
    } finally { sidebar.destroy(); view.destroy(); cleanup(); }
});

test("rendered widget selection never attaches a note to a stale CodeMirror selection", () => {
    const cleanup = installMarkdownTestDom();
    const root = document.body.appendChild(document.createElement("div"));
    const view = new EditorView({doc: "甲乙丙", parent: root, selection: {anchor: 0, head: 2}, extensions: annotationExtension()});
    const widget = document.createElement("div");
    widget.contentEditable = "false";
    widget.setAttribute("contenteditable", "false");
    widget.textContent = "表格中的另外一句话";
    view.dom.append(widget);
    const range = document.createRange();
    range.selectNodeContents(widget);
    let sourceRequests = 0;
    const labels = Object.fromEntries(["title", "add", "orphan", "reattach", "select", "all", "margin", "edit", "remove", "save", "cancel", "close"].map((key) => [key, key])) as unknown as AnnotationLabels;
    const sidebar = new MarkdownAnnotationSidebar(view, root, root, labels, () => { sourceRequests++; });
    document.getSelection().removeAllRanges();
    document.getSelection().addRange(range);
    try {
        assert.equal(sidebar.add(), false);
        assert.equal(sourceRequests, 1);
        assert.equal(view.state.field(annotationField).length, 0);
    } finally { sidebar.destroy(); view.destroy(); cleanup(); }
});
