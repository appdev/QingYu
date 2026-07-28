import {
  StateEffect,
  StateField,
  type Extension,
} from "@codemirror/state";
import { Decoration, EditorView } from "@codemirror/view";
import type { EditorTextSelection } from "../text-selection.ts";

interface HeldSelection {
  readonly from: number;
  readonly to: number;
}

const showSelectionHoldEffect = StateEffect.define<HeldSelection>({
  map(value, changes) {
    return {
      from: changes.mapPos(value.from, 1),
      to: changes.mapPos(value.to, -1),
    };
  },
});
const clearSelectionHoldEffect = StateEffect.define<null>();

const selectionHoldField = StateField.define<HeldSelection | null>({
  create() {
    return null;
  },
  update(value, transaction) {
    let next = value
      ? {
          from: transaction.changes.mapPos(value.from, 1),
          to: transaction.changes.mapPos(value.to, -1),
        }
      : null;
    for (const effect of transaction.effects) {
      if (effect.is(showSelectionHoldEffect)) next = effect.value;
      if (effect.is(clearSelectionHoldEffect)) next = null;
    }
    return next;
  },
  provide: (field) => EditorView.decorations.compute([field], (state) => {
    const selection = state.field(field);
    if (
      !selection ||
      selection.from < 0 ||
      selection.to > state.doc.length ||
      selection.from >= selection.to
    ) {
      return Decoration.none;
    }
    return Decoration.set([
      Decoration.mark({ class: "markra-selection-hold" }).range(
        selection.from,
        selection.to,
      ),
    ]);
  }),
});

const selectionHoldTheme = EditorView.baseTheme({
  ".markra-selection-hold": {
    background: "color-mix(in srgb, var(--accent, #3b82f6) 18%, transparent)",
    borderRadius: "0.2em",
  },
});

export function codeMirrorSelectionHoldPlugin(): Extension {
  return [selectionHoldField, selectionHoldTheme];
}

export function showCodeMirrorSelectionHold(
  view: EditorView,
  selection: EditorTextSelection,
) {
  const from = Math.max(0, Math.min(view.state.doc.length, selection.from));
  const to = Math.max(from, Math.min(view.state.doc.length, selection.to));
  if (from >= to || !selection.text.trim()) {
    clearCodeMirrorSelectionHold(view);
    return false;
  }
  view.dispatch({ effects: showSelectionHoldEffect.of({ from, to }) });
  return true;
}

export function clearCodeMirrorSelectionHold(view: EditorView) {
  if (!view.state.field(selectionHoldField, false)) return false;
  view.dispatch({ effects: clearSelectionHoldEffect.of(null) });
  return true;
}
