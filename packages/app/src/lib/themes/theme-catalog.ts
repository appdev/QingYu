import { builtInThemeDescriptors, builtInThemeIds } from "../../themes/registry";

export type ThemeAppearance = "light" | "dark";
export type ThemeSource = "builtin" | "third-party";
export type ThemeStorageKind = "inlineCss" | "resourceDirectory";

export type ThemePreview = {
  accent: string;
  background: string;
  panel: string;
  text: string;
};

export type ThemeDescriptor = {
  appearance: ThemeAppearance;
  author?: string;
  fileName: string | null;
  fingerprint: string;
  id: string;
  name: string;
  preview: ThemePreview;
  source: ThemeSource;
  storageKind: ThemeStorageKind;
  version?: string;
};

export type InvalidThemeFile = {
  fileName: string;
  reason: string;
};

export type ThemeCatalogSnapshot = {
  invalidFiles: InvalidThemeFile[];
  themes: ThemeDescriptor[];
};

export type MergedThemeCatalog = ThemeCatalogSnapshot & {
  darkThemes: ThemeDescriptor[];
  lightThemes: ThemeDescriptor[];
};

export type ThemeActivationPayload = {
  fingerprint: string;
  id: string;
  token: string;
  source:
    | { kind: "inline"; css: string }
    | { kind: "stylesheet"; href: string };
};

export type ThemeImportResult =
  | { kind: "imported"; theme: ThemeDescriptor }
  | {
      candidate: ThemeDescriptor;
      existing: ThemeDescriptor;
      kind: "conflict";
      sourcePath: string;
    };

export type ThemeRuntimeCapabilities = {
  canDelete: boolean;
  canImport: boolean;
  canOpenDirectory: boolean;
};

function compareThemeDescriptors(left: ThemeDescriptor, right: ThemeDescriptor) {
  const byName = left.name.localeCompare(right.name);

  return byName === 0 ? left.id.localeCompare(right.id) : byName;
}

export function mergeThemeCatalog(snapshot: ThemeCatalogSnapshot): MergedThemeCatalog {
  const installedThemes = snapshot.themes.filter(({ id }) => !builtInThemeIds.has(id));
  const lightBuiltIns = builtInThemeDescriptors.filter(({ appearance }) => appearance === "light");
  const darkBuiltIns = builtInThemeDescriptors.filter(({ appearance }) => appearance === "dark");
  const lightThemes = [
    ...lightBuiltIns,
    ...installedThemes.filter(({ appearance }) => appearance === "light").sort(compareThemeDescriptors)
  ];
  const darkThemes = [
    ...darkBuiltIns,
    ...installedThemes.filter(({ appearance }) => appearance === "dark").sort(compareThemeDescriptors)
  ];

  return {
    darkThemes,
    invalidFiles: snapshot.invalidFiles,
    lightThemes,
    themes: [...lightThemes, ...darkThemes]
  };
}

export function findThemeDescriptor(catalog: MergedThemeCatalog, id: string) {
  return catalog.themes.find((theme) => theme.id === id) ?? null;
}
