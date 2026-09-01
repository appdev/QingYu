import {EditorSelection, Prec, StateEffect, StateField, type Extension} from "@codemirror/state";
import {EditorView, keymap} from "@codemirror/view";
import {initialVisualMarkdownSelection} from "./markra-core/codemirror/frontmatter-preview";

const setMarkdownLineSelection = StateEffect.define<boolean>();

const markdownLineSelection = StateField.define<boolean>({
    create: () => false,
    update(active, transaction) {
        if (transaction.reconfigured) {
            return false;
        }
        const explicit = transaction.effects.find((effect) => effect.is(setMarkdownLineSelection));
        if (explicit) {
            return explicit.value;
        }
        return transaction.docChanged || transaction.selection ? false : active;
    },
});

const selectMarkdownLineOrDocument = (view: EditorView, mode: "source" | "visual") => {
    if (view.state.selection.ranges.length !== 1 || view.state.field(markdownLineSelection)) {
        const from = mode === "visual" ? initialVisualMarkdownSelection(view.state.doc.toString()) : 0;
        view.dispatch({
            effects: setMarkdownLineSelection.of(false),
            scrollIntoView: true,
            selection: EditorSelection.range(from, view.state.doc.length),
        });
        return true;
    }

    const line = view.state.doc.lineAt(view.state.selection.main.head);
    view.dispatch({
        effects: setMarkdownLineSelection.of(true),
        scrollIntoView: true,
        selection: EditorSelection.range(line.from, line.to),
    });
    return true;
};

export const markdownSelectAllExtension = (mode: "source" | "visual"): Extension => [
    markdownLineSelection,
    Prec.highest(keymap.of([{key: "Mod-a", run: (view) => selectMarkdownLineOrDocument(view, mode)}])),
    EditorView.domEventHandlers({
        blur(_event, view) {
            view.dispatch({effects: setMarkdownLineSelection.of(false)});
            return false;
        },
    }),
];
