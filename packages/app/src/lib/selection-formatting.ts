export const selectionFormattingShortcutActions = [
  "bold",
  "italic",
  "strikethrough",
  "inlineCode",
  "heading1",
  "quote",
  "bulletList",
  "orderedList",
] as const;

export const selectionFormattingToolbarActions = [
  "bold",
  "italic",
  "strikethrough",
  "inlineCode",
  "highlight",
  "clearFormatting",
  "paragraph",
  "heading1",
  "quote",
  "bulletList",
  "orderedList",
] as const;

export const selectionHeadingLevels = [1, 2, 3, 4, 5, 6] as const;

export type SelectionFormattingShortcutAction = typeof selectionFormattingShortcutActions[number];
export type SelectionFormattingToolbarAction = typeof selectionFormattingToolbarActions[number];
export type SelectionFormattingAction = SelectionFormattingToolbarAction | "link" | "taskList";
export type SelectionHeadingLevel = typeof selectionHeadingLevels[number];

export type SelectionFormattingState = {
  actions: SelectionFormattingAction[];
  headingLevel: SelectionHeadingLevel | null;
};
