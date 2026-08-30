import {EditorSelection, type EditorState} from "@codemirror/state";
import type {EditorView} from "@codemirror/view";

export type VisualBlockDirection = "backward" | "forward";

export function focusAdjacentVisualBlockBoundary(
  view: EditorView,
  from: number,
  to: number,
  direction: VisualBlockDirection,
) {
  const document = view.state.doc;
  const firstLine = document.lineAt(from);
  const lastLine = document.lineAt(to);
  let anchor: number;

  if (direction === "forward") {
    const previousLine = firstLine.number > 1
      ? document.line(firstLine.number - 1)
      : null;
    anchor = previousLine?.length === 0 ? previousLine.from : firstLine.from;
  } else {
    const nextLine = lastLine.number < document.lines
      ? document.line(lastLine.number + 1)
      : null;
    anchor = nextLine?.length === 0 ? nextLine.from : lastLine.to;
  }

  view.dispatch({
    selection: EditorSelection.cursor(anchor),
    scrollIntoView: true,
    userEvent: "select",
  });
  view.focus();
  return true;
}

function markdownContentOffset(line: string) {
  const quote = /^(?:[ \t]{0,3}>[ \t]*)+/u.exec(line)?.[0].length ?? 0;
  const list = /^(?:[-+*]|\d+[.)])[ \t]+/u.exec(line.slice(quote))?.[0].length ?? 0;
  return quote + list;
}

export function adjacentMarkdownContentPosition(
  state: EditorState,
  position: number,
  direction: VisualBlockDirection,
) {
  const currentLine = state.doc.lineAt(position);
  let lineNumber = currentLine.number + (direction === "forward" ? 1 : -1);

  while (lineNumber >= 1 && lineNumber <= state.doc.lines) {
    const line = state.doc.line(lineNumber);
    if (line.length > 0) {
      const offset = markdownContentOffset(line.text);
      return direction === "forward"
        ? Math.min(line.to, line.from + offset)
        : line.to;
    }
    lineNumber += direction === "forward" ? 1 : -1;
  }
  return null;
}
