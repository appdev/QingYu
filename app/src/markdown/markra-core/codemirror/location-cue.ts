import {StateEffect, StateField, type Extension} from "@codemirror/state";
import {Decoration, EditorView, ViewPlugin, type DecorationSet, type ViewUpdate} from "@codemirror/view";

export const locationCueDurationMs = 1200;

interface LocationCueState {
    decorations: DecorationSet;
    sequence: number;
}

const showLocationCueEffect = StateEffect.define<{position: number}>();
const clearLocationCueEffect = StateEffect.define<null>();

const locationCueField = StateField.define<LocationCueState>({
    create: () => ({decorations: Decoration.none, sequence: 0}),
    provide: (field) => EditorView.decorations.from(field, (cue) => cue.decorations),
    update(cue, transaction) {
        const showEffect = [...transaction.effects].reverse().find((effect) => effect.is(showLocationCueEffect));
        if (showEffect?.is(showLocationCueEffect)) {
            const position = Math.max(0, Math.min(transaction.state.doc.length, showEffect.value.position));
            const sequence = cue.sequence + 1;
            return {
                decorations: Decoration.set([Decoration.line({
                    class: `cm-markra-location-cue cm-markra-location-cue-${sequence % 2 ? "odd" : "even"}`,
                }).range(transaction.state.doc.lineAt(position).from)]),
                sequence,
            };
        }
        if (transaction.docChanged || transaction.selection !== undefined ||
            transaction.effects.some((effect) => effect.is(clearLocationCueEffect))) {
            return {decorations: Decoration.none, sequence: cue.sequence};
        }
        return {decorations: cue.decorations.map(transaction.changes), sequence: cue.sequence};
    },
});

class LocationCueTimer {
    private timer: number | null = null;

    constructor(private readonly view: EditorView) {
    }

    update(update: ViewUpdate) {
        const shown = update.transactions.some((transaction) =>
            transaction.effects.some((effect) => effect.is(showLocationCueEffect)));
        if (!shown) return;
        this.clear();
        this.timer = window.setTimeout(() => {
            this.timer = null;
            clearCodeMirrorLocationCue(this.view);
        }, locationCueDurationMs);
    }

    destroy() {
        this.clear();
    }

    private clear() {
        if (this.timer === null) return;
        window.clearTimeout(this.timer);
        this.timer = null;
    }
}

export const codeMirrorLocationCue = (): Extension => [
    locationCueField,
    ViewPlugin.fromClass(LocationCueTimer),
    EditorView.domEventHandlers({
        pointerdown(_event, view) {
            clearCodeMirrorLocationCue(view);
            return false;
        },
    }),
];

export const showCodeMirrorLocationCue = (view: EditorView, position: number) => {
    if (Number.isFinite(position)) view.dispatch({effects: showLocationCueEffect.of({position})});
};

export const clearCodeMirrorLocationCue = (view: EditorView) => {
    const cue = view.state.field(locationCueField, false);
    if (cue?.decorations.size) view.dispatch({effects: clearLocationCueEffect.of(null)});
};
