import { formatShortcutForPlatform } from "./shortcut-display";

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
