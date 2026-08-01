import { parseMarkdownShortcut } from "@markra/editor";
import type { DesktopPlatform } from "./platform";

export function formatShortcutForPlatform(shortcut: string, platform: DesktopPlatform) {
  const parsed = parseMarkdownShortcut(shortcut);
  if (!parsed) return shortcut;

  return [
    parsed.mod ? (platform === "macos" ? "⌘" : "Ctrl") : null,
    parsed.shift ? (platform === "macos" ? "⇧" : "Shift") : null,
    parsed.alt ? (platform === "macos" ? "⌥" : "Alt") : null,
    parsed.key
  ].filter((part): part is string => Boolean(part)).join("+");
}
