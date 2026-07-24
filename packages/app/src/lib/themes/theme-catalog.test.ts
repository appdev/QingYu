import {
  mergeThemeCatalog,
  protectedThemeDescriptors,
  type ThemeCatalogSnapshot
} from "./theme-catalog";

describe("theme catalog", () => {
  it("keeps protected defaults first and sorts each appearance independently", () => {
    const native: ThemeCatalogSnapshot = {
      invalidFiles: [],
      themes: [
        {
          appearance: "dark",
          fileName: "zeta.css",
          fingerprint: "zeta-fingerprint",
          id: "zeta",
          name: "Zeta",
          preview: { accent: "#89b4fa", background: "#1e1e2e", panel: "#313244", text: "#cdd6f4" },
          source: "third-party",
          storageKind: "inlineCss"
        },
        {
          appearance: "light",
          fileName: "beta.css",
          fingerprint: "beta-fingerprint",
          id: "beta",
          name: "Alpha",
          preview: { accent: "#0969da", background: "#ffffff", panel: "#f6f8fa", text: "#1f2328" },
          source: "third-party",
          storageKind: "resourceDirectory"
        },
        {
          appearance: "light",
          fileName: "alpha.css",
          fingerprint: "alpha-fingerprint",
          id: "alpha",
          name: "Alpha",
          preview: { accent: "#0969da", background: "#ffffff", panel: "#f6f8fa", text: "#1f2328" },
          source: "third-party",
          storageKind: "inlineCss"
        }
      ]
    };

    const merged = mergeThemeCatalog(native);

    expect(merged.lightThemes.map(({ id }) => id)).toEqual(["light", "alpha", "beta"]);
    expect(merged.darkThemes.map(({ id }) => id)).toEqual(["dark", "zeta"]);
    expect(merged.themes.map(({ id }) => id)).toEqual(["light", "alpha", "beta", "dark", "zeta"]);
  });

  it("preserves the native storage kind for activation routing", () => {
    const native: ThemeCatalogSnapshot = {
      invalidFiles: [],
      themes: [{
        appearance: "dark",
        fileName: "drake-ayu",
        fingerprint: "drake-fingerprint",
        id: "drake-ayu",
        name: "Drake Ayu",
        preview: { accent: "#ffcc66", background: "#0f1419", panel: "#131721", text: "#bfbdb6" },
        source: "third-party",
        storageKind: "resourceDirectory"
      }]
    };

    expect(mergeThemeCatalog(native).darkThemes[1]?.storageKind).toBe("resourceDirectory");
  });

  it("exposes exactly two protected defaults", () => {
    expect(protectedThemeDescriptors.map(({ id, source }) => ({ id, source }))).toEqual([
      { id: "light", source: "default" },
      { id: "dark", source: "default" }
    ]);
  });
});
