import { builtInThemeDescriptors, builtInThemeIds } from "./registry";

describe("built-in theme registry", () => {
  it("exposes the four approved frontend-owned themes", () => {
    expect(builtInThemeDescriptors.map((theme) => ({
      appearance: theme.appearance,
      id: theme.id,
      name: theme.name,
      preview: theme.preview,
      source: theme.source
    }))).toEqual([
      {
        appearance: "light",
        id: "light",
        name: "轻语 · 纸白",
        preview: {
          accent: "#1c5d33",
          background: "#ffffff",
          panel: "#f7f7f7",
          text: "#262626"
        },
        source: "builtin"
      },
      {
        appearance: "dark",
        id: "dark",
        name: "轻语 · 夜读",
        preview: {
          accent: "#54c59f",
          background: "#23282d",
          panel: "#282e33",
          text: "#e7e9ea"
        },
        source: "builtin"
      },
      {
        appearance: "light",
        id: "classic-light",
        name: "经典浅色",
        preview: {
          accent: "#1a1c1e",
          background: "#ffffff",
          panel: "#fafafa",
          text: "#555555"
        },
        source: "builtin"
      },
      {
        appearance: "dark",
        id: "classic-dark",
        name: "经典深色",
        preview: {
          accent: "#f4f4f5",
          background: "#1e1e1e",
          panel: "#252526",
          text: "#d4d4d4"
        },
        source: "builtin"
      }
    ]);
  });

  it("uses unique immutable built-in ids with inline frontend storage", () => {
    expect([...builtInThemeIds]).toEqual(["light", "dark", "classic-light", "classic-dark"]);
    expect(new Set(builtInThemeDescriptors.map(({ id }) => id)).size).toBe(4);
    expect(builtInThemeDescriptors.every((theme) => (
      theme.fileName === null && theme.storageKind === "inlineCss"
    ))).toBe(true);
  });
});
