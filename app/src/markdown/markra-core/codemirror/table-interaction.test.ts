import assert = require("node:assert/strict");
import {test} from "node:test";
import {EditorState} from "@codemirror/state";
import {
    createMarkdownTableInteractionController,
    MarkdownTableInteractionController,
    markdownTableInteraction,
    setActiveMarkdownTable,
    setHoveredMarkdownTable,
} from "./table-interaction";

test("keeps table activation independent from hover transitions and document changes", () => {
    const interaction = createMarkdownTableInteractionController();
    let state = EditorState.create({doc: "table", extensions: interaction.extension});

    state = state.update({effects: setHoveredMarkdownTable.of("table-1")}).state;
    assert.deepEqual(markdownTableInteraction(state, interaction.field), {
        activeTableId: null,
        hoverTableId: "table-1",
    });

    state = state.update({effects: setActiveMarkdownTable.of("table-1")}).state;
    state = state.update({effects: setHoveredMarkdownTable.of(null)}).state;
    state = state.update({changes: {from: 0, to: state.doc.length, insert: "rebuilt table"}}).state;
    assert.deepEqual(markdownTableInteraction(state, interaction.field), {
        activeTableId: "table-1",
        hoverTableId: null,
    });
});

test("switches and dismisses the active table only through explicit effects", () => {
    const interaction = createMarkdownTableInteractionController();
    let state = EditorState.create({extensions: interaction.extension});

    state = state.update({effects: setActiveMarkdownTable.of("table-1")}).state;
    state = state.update({effects: setActiveMarkdownTable.of("table-2")}).state;
    assert.equal(markdownTableInteraction(state, interaction.field).activeTableId, "table-2");

    state = state.update({effects: [
        setActiveMarkdownTable.of(null),
        setHoveredMarkdownTable.of(null),
    ]}).state;
    assert.deepEqual(markdownTableInteraction(state, interaction.field), {
        activeTableId: null,
        hoverTableId: null,
    });
});

test("restores interaction state when the CodeMirror bridge is reconfigured", () => {
    const controller = new MarkdownTableInteractionController();
    const first = createMarkdownTableInteractionController(controller);
    let state = EditorState.create({extensions: first.extension});
    state = state.update({effects: [
        setActiveMarkdownTable.of("table-1"),
        setHoveredMarkdownTable.of("table-1"),
    ]}).state;

    const restored = createMarkdownTableInteractionController(controller);
    state = EditorState.create({extensions: restored.extension});
    assert.deepEqual(markdownTableInteraction(state, restored.field), {
        activeTableId: "table-1",
        hoverTableId: "table-1",
    });
});
