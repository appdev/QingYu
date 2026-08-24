import assert = require("node:assert/strict");
import {test} from "node:test";
import {markdown} from "@codemirror/lang-markdown";
import {EditorState, Transaction} from "@codemirror/state";
import {GFM} from "@lezer/markdown";
import {
    createMarkdownTableAppearanceExtension,
    markdownTableAppearanceSnapshot,
    matchMarkdownTableAppearances,
    readMarkdownTableDescriptors,
    setMarkdownTableWidthMode,
    type PersistedMarkdownTableAppearance,
} from "./table-appearance";

const table = "| 项目 | 内容 |\n| --- | --- |\n| 版本 | v1.0 |";

const createState = (doc: string, records: readonly PersistedMarkdownTableAppearance[] = []) => {
    const appearance = createMarkdownTableAppearanceExtension({getRecords: () => records});
    const state = EditorState.create({doc, extensions: [markdown({extensions: [GFM]}), appearance.extension]});
    return {appearance, state};
};

test("restores a persisted width mode without changing Markdown", () => {
    const initial = createState(table);
    const descriptor = readMarkdownTableDescriptors(initial.state)[0];
    const record: PersistedMarkdownTableAppearance = {
        ...descriptor,
        attributes: {widthMode: "even"},
        tableId: "persisted-table",
    };
    const restored = createState(table, [record]);
    const snapshot = markdownTableAppearanceSnapshot(restored.state, restored.appearance.field);

    assert.equal(restored.state.doc.toString(), table);
    assert.equal(snapshot[0].tableId, "persisted-table");
    assert.equal(snapshot[0].attributes.widthMode, "even");
});

test("does not guess when duplicate persisted records are ambiguous", () => {
    const {state} = createState(table);
    const descriptor = readMarkdownTableDescriptors(state)[0];
    const records: PersistedMarkdownTableAppearance[] = ["first", "second"].map((tableId) => ({
        ...descriptor,
        attributes: {widthMode: "even"},
        tableId,
    }));

    assert.equal(matchMarkdownTableAppearances([descriptor], records).size, 0);
});

test("copy creates a new table identity while cut and paste preserves it", () => {
    const original = createState(`${table}\n\n尾部`);
    const originalSnapshot = markdownTableAppearanceSnapshot(original.state, original.appearance.field)[0];
    let state = original.state.update({
        effects: setMarkdownTableWidthMode.of({mode: "even", tableId: originalSnapshot.tableId}),
    }).state;
    state = state.update({
        changes: {from: state.doc.length, insert: `\n\n${table}`},
        annotations: Transaction.userEvent.of("input.paste"),
    }).state;
    const copied = markdownTableAppearanceSnapshot(state, original.appearance.field);

    assert.equal(copied.length, 2);
    assert.equal(copied[0].tableId, originalSnapshot.tableId);
    assert.notEqual(copied[1].tableId, originalSnapshot.tableId);
    assert.equal(copied[1].attributes.widthMode, "auto");

    const cutSource = createState(`${table}\n\n尾部`);
    const cutSnapshot = markdownTableAppearanceSnapshot(cutSource.state, cutSource.appearance.field)[0];
    state = cutSource.state.update({
        effects: setMarkdownTableWidthMode.of({mode: "even", tableId: cutSnapshot.tableId}),
    }).state;
    state = state.update({
        changes: {from: cutSnapshot.from, to: cutSnapshot.to},
        annotations: Transaction.userEvent.of("delete.cut"),
    }).state;
    state = state.update({
        changes: {from: state.doc.length, insert: `\n\n${table}`},
        annotations: Transaction.userEvent.of("input.paste"),
    }).state;
    const moved = markdownTableAppearanceSnapshot(state, cutSource.appearance.field);

    assert.equal(moved.length, 1);
    assert.equal(moved[0].tableId, cutSnapshot.tableId);
    assert.equal(moved[0].attributes.widthMode, "even");
});
