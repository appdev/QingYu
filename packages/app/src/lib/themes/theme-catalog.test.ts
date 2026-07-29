import {
  mergeThemeCatalog,
  type ThemeCatalogSnapshot,
  type ThemeDescriptor
} from "./theme-catalog";
import { builtInThemeDescriptors } from "../../themes/registry";

describe("theme catalog", () => {
  it("keeps each appearance's built-ins first and sorts native themes independently", () => {
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

    expect(merged.lightThemes.map(({ id }) => id)).toEqual(["light", "classic-light", "alpha", "beta"]);
    expect(merged.darkThemes.map(({ id }) => id)).toEqual(["dark", "classic-dark", "zeta"]);
    expect(merged.themes.map(({ id }) => id)).toEqual([
      "light",
      "classic-light",
      "alpha",
      "beta",
      "dark",
      "classic-dark",
      "zeta"
    ]);
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

    expect(mergeThemeCatalog(native).darkThemes.find(({ id }) => id === "drake-ayu")?.storageKind)
      .toBe("resourceDirectory");
  });

  it("filters native descriptors that collide with frontend built-ins", () => {
    const collisions = builtInThemeDescriptors.map((theme) => ({
      ...theme,
      fileName: `${theme.id}.css`,
      fingerprint: `native:${theme.id}`,
      name: `Native ${theme.id}`,
      source: "third-party" as const
    }));

    const merged = mergeThemeCatalog({ invalidFiles: [], themes: collisions });

    expect(merged.themes.map(({ id, fingerprint }) => ({ fingerprint, id }))).toEqual([
      { fingerprint: "builtin:light", id: "light" },
      { fingerprint: "builtin:classic-light", id: "classic-light" },
      { fingerprint: "builtin:dark", id: "dark" },
      { fingerprint: "builtin:classic-dark", id: "classic-dark" }
    ]);
  });

  it("exposes exactly four frontend built-ins", () => {
    expect(builtInThemeDescriptors.map(({ id, source }) => ({ id, source }))).toEqual([
      { id: "light", source: "builtin" },
      { id: "dark", source: "builtin" },
      { id: "classic-light", source: "builtin" },
      { id: "classic-dark", source: "builtin" }
    ]);
  });
});
