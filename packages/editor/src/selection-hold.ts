import type { Node as ProseNode } from "@milkdown/kit/prose/model";
import { Plugin, PluginKey } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import { $prose } from "@milkdown/kit/utils";
import { clampNumber } from "@markra/shared";

export type EditorTextSelection = {
  from: number;
  text: string;
  to: number;
};

type SelectionHoldMeta =
  | {
      kind: "clear";
    }
  | {
      kind: "show";
      selection: EditorTextSelection;
    };

const selectionHoldKey = new PluginKey<DecorationSet>("markra-selection-hold");
const emptySelectionHold = DecorationSet.empty;

export function showSelectionHold(view: EditorView, selection: EditorTextSelection) {
  view.dispatch(view.state.tr.setMeta(selectionHoldKey, { kind: "show", selection } satisfies SelectionHoldMeta));
}

export function clearSelectionHold(view: EditorView) {
  view.dispatch(view.state.tr.setMeta(selectionHoldKey, { kind: "clear" } satisfies SelectionHoldMeta));
}

export const markraSelectionHoldPlugin = $prose(() => {
  return new Plugin<DecorationSet>({
    key: selectionHoldKey,
    props: {
      decorations(state) {
        return selectionHoldKey.getState(state) ?? emptySelectionHold;
      }
    },
    state: {
      apply(transaction, decorations) {
        const meta = transaction.getMeta(selectionHoldKey) as SelectionHoldMeta | undefined;

        if (meta?.kind === "clear") return emptySelectionHold;

        if (meta?.kind === "show") {
          return buildSelectionHoldDecorations(transaction.doc, meta.selection);
        }

        if (transaction.docChanged) return decorations.map(transaction.mapping, transaction.doc);

        return decorations;
      },
      init() {
        return emptySelectionHold;
      }
    }
  });
});

function buildSelectionHoldDecorations(doc: ProseNode, selection: EditorTextSelection) {
  const docSize = doc.content.size;
  const from = clampNumber(selection.from, 0, docSize);
  const to = clampNumber(selection.to, 0, docSize);
  if (from === null || to === null || from >= to || !selection.text.trim()) return emptySelectionHold;

  return DecorationSet.create(doc, [
    Decoration.inline(from, to, {
      class: "markra-selection-hold"
    })
  ]);
}
