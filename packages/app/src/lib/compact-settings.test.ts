import { compactSettingsCategories } from "./compact-settings";

const forbiddenDesktopCategories = [
  "ai",
  "providers",
  "web",
  "view",
  "templates",
  "shortcuts",
  "export",
  "network",
  "logs",
  "system",
  "spellcheck",
  "about"
] as const;

describe("compactSettingsCategories", () => {
  it("returns the curated capability-filtered categories in policy order", () => {
    expect(compactSettingsCategories({ applicationSync: true, mcpPolicy: true }))
      .toEqual(["general", "mcp", "storage", "sync", "appearance", "editor"]);
    expect(compactSettingsCategories({ applicationSync: false, mcpPolicy: false }))
      .toEqual(["general", "storage", "appearance", "editor"]);
  });

  it("never exposes a forbidden desktop category", () => {
    const categories = compactSettingsCategories({ applicationSync: true, mcpPolicy: true });

    forbiddenDesktopCategories.forEach((category) => {
      expect(categories).not.toContain(category);
    });
  });
});
