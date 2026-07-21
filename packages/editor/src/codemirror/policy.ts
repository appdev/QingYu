import type { EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

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

export const revealActiveLine: RevealPolicy = ({
  view,
  state,
  from,
  to,
  scope,
}) => {
  if (!view.hasFocus) return false;

  if (scope === "node") {
    return state.selection.ranges.some(
      (selection) => selection.empty
        ? selection.head > from && selection.head < to
        : selection.from < to && selection.to > from,
    );
  }

  const firstElementLine = state.doc.lineAt(from).number;
  const lastElementLine = state.doc.lineAt(to).number;

  return state.selection.ranges.some((selection) => {
    const firstSelectionLine = state.doc.lineAt(selection.from).number;
    const lastSelectionLine = state.doc.lineAt(selection.to).number;

    return (
      firstSelectionLine <= lastElementLine &&
      lastSelectionLine >= firstElementLine
    );
  });
};
