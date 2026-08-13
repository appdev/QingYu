import { EditorView } from "@codemirror/view";

export const markraTheme = EditorView.baseTheme({
  '&[data-markra-composing="true"] .cm-selectionBackground': {
    // IME pre-edit updates temporarily expose their range as a CodeMirror
    // selection. Hiding only that drawn layer prevents it from flashing.
    backgroundColor: "transparent !important",
  },
  '&[data-markra-link-modifier="true"] .cm-markra-link, &[data-markra-link-modifier="true"] .cm-markra-link-icon': {
    cursor: "pointer",
  },
});
