export type ThemeAppearance = "light" | "dark";
export type ThemeSource = "default" | "third-party";
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

export const protectedThemeDescriptors = [
  {
    appearance: "light",
    fileName: null,
    fingerprint: "default:light",
    id: "light",
    name: "Light",
    preview: {
      accent: "#1a1c1e",
      background: "#ffffff",
      panel: "#f6f8fa",
      text: "#1f2328"
    },
    source: "default",
    storageKind: "inlineCss"
  },
  {
    appearance: "dark",
    fileName: null,
    fingerprint: "default:dark",
    id: "dark",
    name: "Dark",
    preview: {
      accent: "#f4f4f5",
      background: "#0d1117",
      panel: "#161b22",
      text: "#f0f6fc"
    },
    source: "default",
    storageKind: "inlineCss"
  }
] as const satisfies readonly ThemeDescriptor[];

function compareThemeDescriptors(left: ThemeDescriptor, right: ThemeDescriptor) {
  const byName = left.name.localeCompare(right.name);

  return byName === 0 ? left.id.localeCompare(right.id) : byName;
}

export function mergeThemeCatalog(snapshot: ThemeCatalogSnapshot): MergedThemeCatalog {
  const lightDefault = protectedThemeDescriptors[0];
  const darkDefault = protectedThemeDescriptors[1];
  const thirdPartyThemes = snapshot.themes.filter(({ source }) => source === "third-party");
  const lightThemes = [
    lightDefault,
    ...thirdPartyThemes.filter(({ appearance }) => appearance === "light").sort(compareThemeDescriptors)
  ];
  const darkThemes = [
    darkDefault,
    ...thirdPartyThemes.filter(({ appearance }) => appearance === "dark").sort(compareThemeDescriptors)
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
