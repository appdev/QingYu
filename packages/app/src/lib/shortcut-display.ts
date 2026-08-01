import { parseMarkdownShortcut } from "@markra/editor";
import type { DesktopPlatform } from "./platform";

export function shortcutTokensForPlatform(shortcut: string, platform: DesktopPlatform) {
  const parsed = parseMarkdownShortcut(shortcut);
  if (!parsed) return null;

  return [
    parsed.mod ? (platform === "macos" ? "⌘" : "Ctrl") : null,
    parsed.shift ? (platform === "macos" ? "⇧" : "Shift") : null,
    parsed.alt ? (platform === "macos" ? "⌥" : "Alt") : null,
    parsed.key
  ].filter((part): part is string => Boolean(part));
}

export function formatShortcutForPlatform(shortcut: string, platform: DesktopPlatform) {
  return shortcutTokensForPlatform(shortcut, platform)?.join("+") ?? shortcut;
}
