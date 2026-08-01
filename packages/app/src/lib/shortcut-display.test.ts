import {
  formatShortcutForPlatform,
  shortcutTokensForPlatform
} from "./shortcut-display";

describe("shortcutTokensForPlatform", () => {
  it("returns one conventional macOS symbol or key per keycap", () => {
    expect(shortcutTokensForPlatform("Mod+N", "macos")).toEqual(["⌘", "N"]);
    expect(shortcutTokensForPlatform("Mod+Shift+M", "macos")).toEqual(["⌘", "⇧", "M"]);
    expect(shortcutTokensForPlatform("Mod+Alt+F", "macos")).toEqual(["⌘", "⌥", "F"]);
  });

  it.each(["windows", "linux"] as const)("returns one conventional %s label per keycap", (platform) => {
    expect(shortcutTokensForPlatform("Mod+N", platform)).toEqual(["Ctrl", "N"]);
    expect(shortcutTokensForPlatform("Mod+Shift+M", platform)).toEqual(["Ctrl", "Shift", "M"]);
    expect(shortcutTokensForPlatform("Mod+Alt+F", platform)).toEqual(["Ctrl", "Alt", "F"]);
  });

  it("omits an unrecognized shortcut instead of presenting it as a keycap", () => {
    expect(shortcutTokensForPlatform("launch-writing-mode", "macos")).toBeNull();
  });
});

describe("formatShortcutForPlatform", () => {
  it("uses conventional macOS symbols", () => {
    expect(formatShortcutForPlatform("Mod+N", "macos")).toBe("⌘+N");
    expect(formatShortcutForPlatform("Mod+Shift+M", "macos")).toBe("⌘+⇧+M");
    expect(formatShortcutForPlatform("Mod+Alt+F", "macos")).toBe("⌘+⌥+F");
  });

  it.each(["windows", "linux"] as const)("uses Ctrl labels on %s", (platform) => {
    expect(formatShortcutForPlatform("Mod+N", platform)).toBe("Ctrl+N");
    expect(formatShortcutForPlatform("Mod+Shift+M", platform)).toBe("Ctrl+Shift+M");
    expect(formatShortcutForPlatform("Mod+Alt+F", platform)).toBe("Ctrl+Alt+F");
  });

  it("keeps an unrecognized shortcut unchanged", () => {
    expect(formatShortcutForPlatform("launch-writing-mode", "macos"))
      .toBe("launch-writing-mode");
  });
});
