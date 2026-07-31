import {
  resolveServerStartupTheme,
  startServerStartupAppearance,
} from "./server-startup-appearance";

describe("server startup appearance", () => {
  afterEach(() => {
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-theme-appearance");
    document.documentElement.style.removeProperty("color-scheme");
  });

  it("uses the app startup theme parameters for light and dark appearances", () => {
    const search = "?startupAppearanceMode=system&startupLightTheme=classic-light&startupDarkTheme=classic-dark";

    expect(resolveServerStartupTheme(search, false)).toEqual({
      appearance: "light",
      theme: "classic-light",
    });
    expect(resolveServerStartupTheme(search, true)).toEqual({
      appearance: "dark",
      theme: "classic-dark",
    });
  });

  it("follows system appearance until the authenticated app takes over", () => {
    let listener: ((event: MediaQueryListEvent) => unknown) | undefined;
    const mediaQuery = {
      matches: false,
      addEventListener: vi.fn((_name: string, next: (event: MediaQueryListEvent) => unknown) => {
        listener = next;
      }),
      removeEventListener: vi.fn(),
    } as unknown as MediaQueryList;
    const stop = startServerStartupAppearance({
      matchMedia: () => mediaQuery,
      root: document.documentElement,
      search: "",
    });

    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.dataset.themeAppearance).toBe("light");
    listener?.({ matches: true } as MediaQueryListEvent);
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.dataset.themeAppearance).toBe("dark");

    stop();
    expect(mediaQuery.removeEventListener).toHaveBeenCalledOnce();
  });

  it("preserves an existing application theme during reauthentication", () => {
    document.documentElement.dataset.theme = "classic-dark";
    document.documentElement.dataset.themeAppearance = "dark";
    const matchMedia = vi.fn();

    startServerStartupAppearance({
      matchMedia,
      root: document.documentElement,
      search: "",
    });

    expect(matchMedia).not.toHaveBeenCalled();
    expect(document.documentElement.dataset.theme).toBe("classic-dark");
  });
});
