export type ServerStartupAppearance = "light" | "dark";

export type ServerStartupTheme = {
  appearance: ServerStartupAppearance;
  theme: "light" | "dark" | "classic-light" | "classic-dark";
};

type ServerStartupAppearanceOptions = {
  matchMedia?: (query: string) => MediaQueryList;
  root?: HTMLElement;
  search?: string;
};

const systemDarkQuery = "(prefers-color-scheme: dark)";

export function resolveServerStartupTheme(
  search: string,
  prefersDark: boolean,
): ServerStartupTheme {
  const params = new URLSearchParams(search);
  const appearanceMode = params.get("startupAppearanceMode");
  const lightTheme = lightThemeFromParam(params.get("startupLightTheme"));
  const darkTheme = darkThemeFromParam(params.get("startupDarkTheme"));
  const appearance = appearanceMode === "light" || appearanceMode === "dark"
    ? appearanceMode
    : prefersDark ? "dark" : "light";

  return appearance === "dark"
    ? { appearance, theme: darkTheme }
    : { appearance, theme: lightTheme };
}

export function startServerStartupAppearance(
  options: ServerStartupAppearanceOptions = {},
) {
  const root = options.root ?? document.documentElement;
  if (root.dataset.theme && root.dataset.themeAppearance) {
    return () => undefined;
  }

  const matchMedia = options.matchMedia ?? window.matchMedia.bind(window);
  const mediaQuery = matchMedia(systemDarkQuery);
  const search = options.search ?? window.location.search;
  const params = new URLSearchParams(search);
  const followsSystem = params.get("startupAppearanceMode") !== "light" &&
    params.get("startupAppearanceMode") !== "dark";
  const apply = (prefersDark: boolean) => {
    const resolved = resolveServerStartupTheme(search, prefersDark);
    root.dataset.theme = resolved.theme;
    root.dataset.themeAppearance = resolved.appearance;
    root.style.colorScheme = resolved.appearance;
  };

  apply(mediaQuery.matches);
  if (!followsSystem) return () => undefined;

  const handleChange = (event: MediaQueryListEvent) => apply(event.matches);
  mediaQuery.addEventListener("change", handleChange);

  return () => {
    mediaQuery.removeEventListener("change", handleChange);
  };
}

function lightThemeFromParam(value: string | null) {
  return value === "classic-light" ? value : "light";
}

function darkThemeFromParam(value: string | null) {
  return value === "classic-dark" ? value : "dark";
}
