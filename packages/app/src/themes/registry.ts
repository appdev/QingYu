import type { ThemeDescriptor } from "../lib/themes/theme-catalog";

export const builtInThemeDescriptors = [
  {
    appearance: "light",
    author: "轻语",
    fileName: null,
    fingerprint: "builtin:light",
    id: "light",
    name: "轻语 · 纸白",
    preview: {
      accent: "#1c5d33",
      background: "#ffffff",
      panel: "#f7f7f7",
      text: "#262626"
    },
    source: "builtin",
    storageKind: "inlineCss"
  },
  {
    appearance: "dark",
    author: "轻语",
    fileName: null,
    fingerprint: "builtin:dark",
    id: "dark",
    name: "轻语 · 夜读",
    preview: {
      accent: "#54c59f",
      background: "#23282d",
      panel: "#282e33",
      text: "#e7e9ea"
    },
    source: "builtin",
    storageKind: "inlineCss"
  },
  {
    appearance: "light",
    author: "轻语",
    fileName: null,
    fingerprint: "builtin:classic-light",
    id: "classic-light",
    name: "经典浅色",
    preview: {
      accent: "#1a1c1e",
      background: "#ffffff",
      panel: "#fafafa",
      text: "#555555"
    },
    source: "builtin",
    storageKind: "inlineCss"
  },
  {
    appearance: "dark",
    author: "轻语",
    fileName: null,
    fingerprint: "builtin:classic-dark",
    id: "classic-dark",
    name: "经典深色",
    preview: {
      accent: "#f4f4f5",
      background: "#1e1e1e",
      panel: "#252526",
      text: "#d4d4d4"
    },
    source: "builtin",
    storageKind: "inlineCss"
  }
] as const satisfies readonly ThemeDescriptor[];

export const builtInThemeIds: ReadonlySet<string> = new Set(
  builtInThemeDescriptors.map(({ id }) => id)
);
