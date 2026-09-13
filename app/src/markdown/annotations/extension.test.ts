import assert = require("node:assert/strict");
import {test} from "node:test";
import {EditorState, Text, Transaction} from "@codemirror/state";
import {history, undo, redo} from "@codemirror/commands";
import {anchorAt} from "./anchors";
import {annotationExtension, annotationField, changeAnnotation} from "./extension";

test("maps inside edits but excludes insertion at either boundary", () => {
    let state = EditorState.create({doc: "甲乙丙丁", extensions: annotationExtension([
        {...anchorAt("甲乙丙丁", 1, 3), id: "a", note: "note", createdAt: 1, updatedAt: 1},
    ])});
    state = state.update({changes: [{from: 1, insert: "前"}, {from: 3, insert: "后"}]}).state;
    assert.equal(state.field(annotationField)[0].quote, "乙丙");
    state = state.update({changes: {from: 3, insert: "中"}}).state;
    assert.equal(state.field(annotationField)[0].quote, "乙中丙");
});

test("whole-document title updates preserve anchors in the unchanged suffix", () => {
    const text = "---\ntitle: old\n---\n甲乙丙";
    const from = text.indexOf("乙");
    const state = EditorState.create({doc: text, extensions: annotationExtension([
        {...anchorAt(text, from, from + 1), id: "a", note: "note", createdAt: 1, updatedAt: 1},
    ])});
    const after = text.replace("old", "new title");
    const next = state.update({changes: {from: 0, to: text.length, insert: after}}).state;
    assert.equal(next.field(annotationField)[0].from, after.indexOf("乙"));
    assert.equal(next.field(annotationField)[0].status, "attached");
});

test("normalized replacement keeps CRLF document lines and annotation offsets", () => {
    const state = EditorState.create({doc: "甲\r\n乙丙", extensions: [EditorState.lineSeparator.of("\r\n"), annotationExtension([
        {...anchorAt("甲\n乙丙", 2, 4), id: "a", note: "note", createdAt: 1, updatedAt: 1},
    ])]});
    const next = state.update({changes: {from: 0, to: state.doc.length, insert: Text.of(["标题", "甲", "乙丙"])}}).state;
    assert.equal(next.doc.lines, 3);
    assert.equal(next.sliceDoc(), "标题\r\n甲\r\n乙丙");
    assert.equal(next.field(annotationField)[0].from, 5);
    assert.equal(next.field(annotationField)[0].quote, "乙丙");
});
test("undo and redo restore deleted anchors without reverting independently edited notes", () => {
    let state = EditorState.create({doc: "甲乙丙丁", extensions: [history(), annotationExtension([
        {...anchorAt("甲乙丙丁", 1, 3), id: "a", note: "note", createdAt: 1, updatedAt: 1},
    ])]});
    const target = {get state() { return state; }, dispatch: (tr: {state: EditorState}) => { state = tr.state; }};
    state = state.update({changes: {from: 1, to: 3}}).state;
    assert.equal(state.field(annotationField)[0].status, "orphaned");
    state = state.update({effects: changeAnnotation.of({...state.field(annotationField)[0], note: "new"})}).state;
    assert.ok(undo(target));
    assert.equal(state.field(annotationField)[0].quote, "乙丙");
    assert.equal(state.field(annotationField)[0].status, "attached");
    assert.equal(state.field(annotationField)[0].note, "new");
    assert.ok(redo(target));
    assert.equal(state.field(annotationField)[0].status, "orphaned");
});

test("history anchors follow a non-history change before the quoted range", () => {
    let state = EditorState.create({doc: "甲乙丙丁", extensions: [history(), annotationExtension([
        {...anchorAt("甲乙丙丁", 1, 3), id: "a", note: "note", createdAt: 1, updatedAt: 1},
    ])]});
    const target = {get state() { return state; }, dispatch: (tr: {state: EditorState}) => { state = tr.state; }};
    state = state.update({changes: {from: 2, insert: "字"}}).state;
    state = state.update({changes: {from: 0, insert: "外"}, annotations: Transaction.addToHistory.of(false)}).state;
    assert.ok(undo(target));
    const record = state.field(annotationField)[0];
    assert.equal(state.doc.toString(), "外甲乙丙丁");
    assert.equal(record.from, 2);
    assert.equal(record.to, 4);
    assert.equal(record.quote, "乙丙");
});
