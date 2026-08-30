import assert = require("node:assert/strict");
import {test} from "node:test";
import {markdown} from "@codemirror/lang-markdown";
import {forceParsing} from "@codemirror/language";
import {EditorState, Transaction} from "@codemirror/state";
import {EditorView} from "@codemirror/view";
import {GFM} from "@lezer/markdown";
import {installMarkdownTestDom} from "../../markraTestDom";
import {
    createMarkdownTableAppearanceExtension,
    deleteMarkdownTableAppearance,
    ensureMarkdownTableAppearance,
    markdownTableAppearanceAt,
    markdownTableAppearanceSnapshot,
    matchMarkdownTableAppearances,
    readMarkdownTableDescriptors,
    resolveMarkdownTableAppearance,
    setMarkdownTableWidthMode,
    type PersistedMarkdownTableAppearance,
} from "./table-appearance";
import {
    createMarkdownTableInteractionController,
    markdownActiveTableId,
    setActiveMarkdownTable,
} from "./table-interaction";

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

test("resolves table appearance from a position inside the current table range", () => {
    const initial = createState(table);
    const original = markdownTableAppearanceSnapshot(initial.state, initial.appearance.field)[0];
    const state = initial.state.update({
        effects: setMarkdownTableWidthMode.of({
            from: original.from,
            mode: "even",
            tableId: original.tableId,
        }),
    }).state;

    const current = markdownTableAppearanceAt(state, initial.appearance.field, original.from + 1);
    const restored = resolveMarkdownTableAppearance(state, original.from + 1, [original], "auto");

    assert.equal(current?.tableId, original.tableId);
    assert.equal(current?.attributes.widthMode, "even");
    assert.equal(restored?.tableId, original.tableId);
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

test("recovers an unambiguous custom width record that a partial snapshot deleted", () => {
    const {state} = createState(table);
    const descriptor = readMarkdownTableDescriptors(state)[0];
    const record: PersistedMarkdownTableAppearance = {
        ...descriptor,
        attributes: {widthMode: "even"},
        deletedAt: Date.now(),
        tableId: "deleted-by-partial-snapshot",
    };

    const restored = createState(table, [record]);
    const current = markdownTableAppearanceSnapshot(restored.state, restored.appearance.field)[0];

    assert.equal(current.tableId, record.tableId);
    assert.equal(current.attributes.widthMode, "even");
});

test("keeps table identity and width mode when alignment rewrites the complete table", () => {
    const initial = createState(table);
    const original = markdownTableAppearanceSnapshot(initial.state, initial.appearance.field)[0];
    let state = initial.state.update({
        effects: setMarkdownTableWidthMode.of({
            from: original.from,
            mode: "even",
            tableId: original.tableId,
        }),
    }).state;
    const centered = table.replace("| --- | --- |", "| :---: | :---: |");
    state = state.update({changes: {from: original.from, to: original.to, insert: centered}}).state;
    const rewritten = markdownTableAppearanceSnapshot(state, initial.appearance.field)[0];

    assert.equal(rewritten.tableId, original.tableId);
    assert.equal(rewritten.attributes.widthMode, "even");
});

test("keeps the active table through reconstruction until it is explicitly deleted", () => {
    const initial = createState(table);
    const interaction = createMarkdownTableInteractionController();
    let state = EditorState.create({
        doc: table,
        extensions: [markdown({extensions: [GFM]}), initial.appearance.extension, interaction.extension],
    });
    const original = markdownTableAppearanceSnapshot(state, initial.appearance.field)[0];
    state = state.update({effects: setActiveMarkdownTable.of(original.tableId)}).state;
    const centered = table.replace("| --- | --- |", "| :---: | :---: |");

    state = state.update({changes: {from: original.from, to: original.to, insert: centered}}).state;
    assert.equal(markdownActiveTableId(state, interaction.field), original.tableId);

    const current = markdownTableAppearanceSnapshot(state, initial.appearance.field)[0];
    state = state.update({changes: {from: current.from, to: current.to, insert: "temporary replacement"}}).state;
    assert.equal(markdownActiveTableId(state, interaction.field), original.tableId);

    state = state.update({
        effects: [
            deleteMarkdownTableAppearance.of({tableId: original.tableId}),
            setActiveMarkdownTable.of(null),
        ],
    }).state;
    assert.equal(markdownActiveTableId(state, interaction.field), null);
});

test("keeps table appearance through a transient parser gap during complete replacement", () => {
    const initial = createState(table);
    const original = markdownTableAppearanceSnapshot(initial.state, initial.appearance.field)[0];
    let state = initial.state.update({
        effects: setMarkdownTableWidthMode.of({
            from: original.from,
            mode: "even",
            tableId: original.tableId,
        }),
    }).state;
    state = state.update({changes: {from: original.from, to: original.to, insert: "temporary replacement"}}).state;
    const retained = markdownTableAppearanceSnapshot(state, initial.appearance.field)[0];
    assert.equal(retained.tableId, original.tableId);
    assert.equal(retained.attributes.widthMode, "even");

    const centered = table.replace("| --- | --- |", "| :---: | :---: |");
    state = state.update({changes: {from: retained.from, to: retained.to, insert: centered}}).state;
    const restored = markdownTableAppearanceSnapshot(state, initial.appearance.field)[0];
    assert.equal(restored.tableId, original.tableId);
    assert.equal(restored.attributes.widthMode, "even");
});

test("reports only an explicitly removed table as deleted", () => {
    const cleanup = installMarkdownTestDom();
    const deleted: string[][] = [];
    const appearance = createMarkdownTableAppearanceExtension({
        onDelete: (tableIds) => deleted.push([...tableIds]),
    });
    const view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc: `${table}\n\n尾部`,
            extensions: [markdown({extensions: [GFM]}), appearance.extension],
        }),
    });

    try {
        const original = markdownTableAppearanceSnapshot(view.state, appearance.field)[0];
        const centered = table.replace("| --- | --- |", "| :---: | :---: |");
        view.dispatch({changes: {from: original.from, to: original.to, insert: centered}});
        assert.deepEqual(deleted, []);

        const current = markdownTableAppearanceSnapshot(view.state, appearance.field)[0];
        view.dispatch({
            changes: {from: current.from, to: current.to},
            effects: deleteMarkdownTableAppearance.of({tableId: current.tableId}),
        });
        assert.deepEqual(deleted, [[original.tableId]]);
    } finally {
        view.destroy();
        cleanup();
    }
});

test("registers and restores a table parsed after the initial long-document viewport", () => {
    const cleanup = installMarkdownTestDom();
    const changes: PersistedMarkdownTableAppearance[] = [];
    const prefix = Array.from({length: 5000}, (_, index) => `paragraph ${index}`).join("\n\n");
    const appearance = createMarkdownTableAppearanceExtension({
        getRecords: () => changes,
        onChange: (record) => changes.push(record),
    });
    const view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc: `${prefix}\n\n${table}`,
            extensions: [markdown({extensions: [GFM]}), appearance.extension],
        }),
    });

    try {
        assert.equal(markdownTableAppearanceSnapshot(view.state, appearance.field).length, 0);
        assert.equal(forceParsing(view, view.state.doc.length, 5000), true);
        const descriptor = readMarkdownTableDescriptors(view.state)[0];
        const persisted: PersistedMarkdownTableAppearance = {
            ...descriptor,
            attributes: {widthMode: "even"},
            tableId: "persisted-late-table",
        };
        changes.push(persisted);

        const restored = resolveMarkdownTableAppearance(view.state, descriptor.from, changes, "auto");
        assert.equal(restored?.tableId, "persisted-late-table");
        assert.equal(restored?.attributes.widthMode, "even");

        view.dispatch({
            effects: setMarkdownTableWidthMode.of({
                from: descriptor.from,
                mode: "auto",
                tableId: restored?.tableId ?? "persisted-late-table",
            }),
        });
        const snapshot = markdownTableAppearanceSnapshot(view.state, appearance.field);
        assert.equal(snapshot.length, 1);
        assert.equal(snapshot[0].tableId, "persisted-late-table");
        assert.equal(snapshot[0].attributes.widthMode, "auto");
        assert.equal(changes.at(-1)?.attributes.widthMode, "auto");
    } finally {
        view.destroy();
        cleanup();
    }
});

test("keeps a late-parsed table identity stable through its first document-changing action", () => {
    const cleanup = installMarkdownTestDom();
    const prefix = Array.from({length: 5000}, (_, index) => `paragraph ${index}`).join("\n\n");
    const appearance = createMarkdownTableAppearanceExtension();
    const view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc: `${prefix}\n\n${table}`,
            extensions: [markdown({extensions: [GFM]}), appearance.extension],
        }),
    });

    try {
        assert.equal(markdownTableAppearanceSnapshot(view.state, appearance.field).length, 0);
        assert.equal(forceParsing(view, view.state.doc.length, 5000), true);
        const descriptor = readMarkdownTableDescriptors(view.state)[0];
        const provisionalTableId = `table-${descriptor.from}`;
        const centered = table.replace("| --- | --- |", "| :---: | :---: |");

        view.dispatch({
            changes: {from: descriptor.from, to: descriptor.to, insert: centered},
            effects: [
                ensureMarkdownTableAppearance.of({
                    from: descriptor.from,
                    tableId: provisionalTableId,
                }),
                setMarkdownTableWidthMode.of({
                    from: descriptor.from,
                    mode: "even",
                    tableId: provisionalTableId,
                }),
            ],
        });

        const registered = markdownTableAppearanceSnapshot(view.state, appearance.field);
        assert.equal(registered.length, 1);
        assert.equal(registered[0].tableId, provisionalTableId);
        assert.equal(registered[0].attributes.widthMode, "even");
    } finally {
        view.destroy();
        cleanup();
    }
});

test("keeps persisted appearance when a newly parsed table is registered by a later document change", () => {
    const cleanup = installMarkdownTestDom();
    const records: PersistedMarkdownTableAppearance[] = [];
    const prefix = Array.from({length: 5000}, (_, index) => `paragraph ${index}`).join("\n\n");
    const appearance = createMarkdownTableAppearanceExtension({getRecords: () => records});
    const view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc: `${prefix}\n\n${table}`,
            extensions: [markdown({extensions: [GFM]}), appearance.extension],
        }),
    });

    try {
        assert.equal(markdownTableAppearanceSnapshot(view.state, appearance.field).length, 0);
        assert.equal(forceParsing(view, view.state.doc.length, 5000), true);
        const descriptor = readMarkdownTableDescriptors(view.state)[0];
        records.push({
            ...descriptor,
            attributes: {widthMode: "even"},
            tableId: "persisted-late-table",
        });

        view.dispatch({changes: {from: 0, insert: "前缀\n\n"}});
        const registered = markdownTableAppearanceSnapshot(view.state, appearance.field).at(-1);

        assert.equal(registered?.tableId, "persisted-late-table");
        assert.equal(registered?.attributes.widthMode, "even");
    } finally {
        view.destroy();
        cleanup();
    }
});

test("uses the current table position when a rendered widget has a stale identity", () => {
    const cleanup = installMarkdownTestDom();
    const changes: PersistedMarkdownTableAppearance[] = [];
    const appearance = createMarkdownTableAppearanceExtension({
        onChange: (record) => changes.push(record),
    });
    const view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc: table,
            extensions: [markdown({extensions: [GFM]}), appearance.extension],
        }),
    });

    try {
        const original = markdownTableAppearanceSnapshot(view.state, appearance.field)[0];
        view.dispatch({
            effects: setMarkdownTableWidthMode.of({
                from: original.from,
                mode: "even",
                tableId: "stale-rendered-widget",
            }),
        });
        const updated = markdownTableAppearanceSnapshot(view.state, appearance.field)[0];

        assert.equal(updated.tableId, original.tableId);
        assert.equal(updated.attributes.widthMode, "even");
        assert.equal(changes.at(-1)?.tableId, original.tableId);
        assert.equal(changes.at(-1)?.attributes.widthMode, "even");
    } finally {
        view.destroy();
        cleanup();
    }
});

test("copy creates a new table identity while cut and paste preserves it", () => {
    const original = createState(`${table}\n\n尾部`);
    const originalSnapshot = markdownTableAppearanceSnapshot(original.state, original.appearance.field)[0];
    let state = original.state.update({
        effects: setMarkdownTableWidthMode.of({
            from: originalSnapshot.from,
            mode: "even",
            tableId: originalSnapshot.tableId,
        }),
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
        effects: setMarkdownTableWidthMode.of({
            from: cutSnapshot.from,
            mode: "even",
            tableId: cutSnapshot.tableId,
        }),
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
