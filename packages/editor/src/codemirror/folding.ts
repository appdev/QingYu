import { foldAll, foldedRanges, unfoldAll } from "@codemirror/language";
import type { EditorView } from "@codemirror/view";

export function toggleAllCodeMirrorFolds(view: EditorView) {
  return foldedRanges(view.state).size > 0
    ? unfoldAll(view)
    : foldAll(view);
}
