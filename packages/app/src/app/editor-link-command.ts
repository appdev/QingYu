type EditorLinkCommandOptions = {
  insertMarkdownLink: () => unknown;
  readOnlyMode: boolean;
  syncSelectionToolbarFormattingState: () => unknown;
  syncVisualMarkdownAfterEditorCommand: () => unknown;
};

export function runEditorLinkCommand({
  insertMarkdownLink,
  readOnlyMode,
  syncSelectionToolbarFormattingState,
  syncVisualMarkdownAfterEditorCommand
}: EditorLinkCommandOptions) {
  if (readOnlyMode) return false;

  insertMarkdownLink();
  syncVisualMarkdownAfterEditorCommand();
  syncSelectionToolbarFormattingState();
  return true;
}
