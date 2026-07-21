import type { EditorState } from "@codemirror/state";
import type { EditorView, ViewUpdate } from "@codemirror/view";

export type RevealScope = "line" | "node";

export interface RevealContext {
  view: EditorView;
  state: EditorState;
  from: number;
  to: number;
  nodeName: string;
  scope: RevealScope;
}

export type RevealPolicy = (context: RevealContext) => boolean;

function revealCursorKey(state: EditorState) {
  return state.selection.ranges
    .filter((selection) => selection.empty)
    .map((selection) => selection.head)
    .join(":");
}

export function selectionChangeAffectsReveal(update: ViewUpdate) {
  return (
    update.selectionSet &&
    revealCursorKey(update.startState) !== revealCursorKey(update.state)
  );
}

export function cursorInsideRange(
  view: EditorView,
  from: number,
  to: number,
) {
  return (
    view.hasFocus &&
    view.state.selection.ranges.some(
      (selection) =>
        selection.empty && selection.head > from && selection.head < to,
    )
  );
}

export const revealActiveLine: RevealPolicy = ({
  view,
  state,
  from,
  to,
  scope,
}) => {
  if (!view.hasFocus) return false;

  // A range selection is a visual operation, not an intent to edit Markdown
  // source. Revealing delimiters while its endpoint moves can rewrap lines and
  // change the document height underneath the pointer.
  const cursors = state.selection.ranges.filter((selection) => selection.empty);

  if (scope === "node") {
    return cursors.some(
      (selection) => selection.head > from && selection.head < to,
    );
  }

  // Keep the live preview intact while editing rendered text. Markdown source
  // only needs to reappear when the selection actually reaches its marker.
  return cursors.some((selection) =>
    from === to
      ? selection.head === from
      : selection.head >= from && selection.head < to,
  );
};
