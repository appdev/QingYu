import { keymap, type EditorView as CodeMirrorView } from "@codemirror/view";
import {
  keyboardShortcutActions,
  normalizeKeyboardShortcuts,
  type KeyboardShortcutAction,
  type KeyboardShortcutMap,
} from "@markra/shared";
import {
  insertCodeMirrorMarkdownImage,
  insertCodeMirrorMarkdownLink,
  insertCodeMirrorMarkdownTable,
} from "./controller.ts";
import { toggleAllCodeMirrorFolds } from "./folding.ts";
import { defineMarkraPlugin, runMarkraCommand } from "./plugin.ts";

export interface MarkdownShortcutsPluginOptions {
  openSpellcheckSuggestions?: (view: CodeMirrorView) => boolean;
  shortcuts?: KeyboardShortcutMap;
  toggleAllFolds?: (view: CodeMirrorView) => boolean;
}

const commandByAction: Partial<Record<KeyboardShortcutAction, string>> = {
  bold: "format.bold",
  bulletList: "block.bullet-list",
  codeBlock: "block.code",
  heading1: "block.heading.1",
  heading2: "block.heading.2",
  heading3: "block.heading.3",
  inlineCode: "format.code",
  italic: "format.italic",
  orderedList: "block.ordered-list",
  paragraph: "block.paragraph",
  quote: "block.quote",
  strikethrough: "format.strikethrough",
};

function runShortcutAction(
  view: CodeMirrorView,
  action: KeyboardShortcutAction,
  options: MarkdownShortcutsPluginOptions,
) {
  const command = commandByAction[action];
  if (command) return runMarkraCommand(view, command);

  switch (action) {
    case "image":
      return insertCodeMirrorMarkdownImage(view);
    case "link":
      return insertCodeMirrorMarkdownLink(view);
    case "table":
      return insertCodeMirrorMarkdownTable(view);
    case "toggleAllFolds":
      return options.toggleAllFolds?.(view) ?? toggleAllCodeMirrorFolds(view);
    case "openSpellcheckSuggestions":
      return options.openSpellcheckSuggestions?.(view) ?? false;
    default:
      return false;
  }
}

function codeMirrorShortcut(shortcut: string) {
  const parts = shortcut.split("+");
  const key = parts.at(-1);
  if (key && /^[A-Z]$/u.test(key)) parts[parts.length - 1] = key.toLocaleLowerCase();
  return parts.join("-");
}

export function markdownShortcutsPlugin(
  options: MarkdownShortcutsPluginOptions = {},
) {
  const shortcuts = normalizeKeyboardShortcuts(options.shortcuts);
  return defineMarkraPlugin({
    id: "markra.markdown-shortcuts",
    extension: keymap.of(
      keyboardShortcutActions.map((action) => ({
        key: codeMirrorShortcut(shortcuts[action]),
        run: (view) => runShortcutAction(view, action, options),
      })),
    ),
  });
}
