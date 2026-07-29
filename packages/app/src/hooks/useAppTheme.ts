import { diagnosticErrorMessage } from "@markra/shared";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  appAppearanceModeOptions,
  defaultAppThemePreferences,
  getStoredThemePreferences,
  isThemeId,
  normalizeAppThemePreferences,
  resolveAppThemePreferencesAppearance,
  resolveAppThemePreferencesEditorTheme,
  saveStoredThemePreferences,
  type AppAppearanceMode,
  type AppThemePreferences,
  type ResolvedAppTheme
} from "../lib/settings/app-settings";
import {
  findThemeDescriptor,
  type ThemeActivationPayload,
  type ThemeAppearance,
  type ThemeDescriptor
} from "../lib/themes/theme-catalog";
import {
  listenAppThemeChanged,
  notifyAppThemeChanged
} from "../lib/settings/settings-events";
import { getAppRuntime } from "../runtime";
import { useThemeCatalog } from "./useThemeCatalog";

const systemDarkThemeQuery = "(prefers-color-scheme: dark)";
const thirdPartyThemeStyleElementId = "markra-third-party-theme-style";
const thirdPartyThemeLinkElementId = "markra-third-party-theme-link";
const startupThemeStyleElementId = "markra-startup-theme-style";
const startupAppearanceModeParam = "startupAppearanceMode";
const startupLightThemeParam = "startupLightTheme";
const startupDarkThemeParam = "startupDarkTheme";
const themeTransitionAttribute = "data-theme-transition";
let themeTransitionSequence = 0;

function getSystemTheme(): ResolvedAppTheme {
  if (typeof window.matchMedia !== "function") return "light";

  return window.matchMedia(systemDarkThemeQuery).matches ? "dark" : "light";
}

function removeStartupThemeCss() {
  document.getElementById(startupThemeStyleElementId)?.remove();
  document.documentElement.style.removeProperty("background-color");
  document.documentElement.style.removeProperty("color-scheme");
}

function removeActiveThirdPartyThemeElements() {
  document.getElementById(thirdPartyThemeStyleElementId)?.remove();
  document.getElementById(thirdPartyThemeLinkElementId)?.remove();
}

function removeThirdPartyThemeElements() {
  removeActiveThirdPartyThemeElements();
  document.querySelectorAll("[data-markra-theme-candidate]").forEach((element) => element.remove());
}

function installInlineThirdPartyTheme(css: string) {
  const style = document.createElement("style");
  style.id = thirdPartyThemeStyleElementId;
  style.dataset.markraThirdPartyTheme = "true";
  style.textContent = css;
  removeActiveThirdPartyThemeElements();
  document.head.append(style);
  return style;
}

type PendingStylesheetLoad = {
  cancel: () => unknown;
  loaded: Promise<HTMLLinkElement | null>;
};

type ThemePreparationQueue = {
  current: Promise<unknown>;
};

async function acquireThemePreparation(queue: ThemePreparationQueue) {
  const previous = queue.current.catch(() => undefined);
  let release: () => unknown = () => undefined;
  const turn = new Promise<unknown>((resolve) => {
    release = () => resolve(undefined);
  });
  queue.current = previous.then(() => turn);
  await previous;
  return release;
}

function loadThirdPartyThemeStylesheet(
  href: string,
  token: string,
  themeName: string
): PendingStylesheetLoad {
  const element = document.createElement("link");
  element.rel = "stylesheet";
  element.href = href;
  element.dataset.markraThemeCandidate = token;
  let settled = false;
  let resolveLoad: (loaded: HTMLLinkElement | null) => unknown = () => undefined;
  let rejectLoad: (reason: Error) => unknown = () => undefined;

  const removeListeners = () => {
    element.removeEventListener("load", handleLoad);
    element.removeEventListener("error", handleError);
  };
  const handleLoad = () => {
    if (settled) return;
    settled = true;
    removeListeners();
    resolveLoad(element);
  };
  const handleError = () => {
    if (settled) return;
    settled = true;
    removeListeners();
    element.remove();
    rejectLoad(new Error(`Failed to load the stylesheet for ${themeName}.`));
  };
  const loaded = new Promise<HTMLLinkElement | null>((resolve, reject) => {
    resolveLoad = resolve;
    rejectLoad = reject;
  });

  element.addEventListener("load", handleLoad);
  element.addEventListener("error", handleError);
  document.head.append(element);

  return {
    cancel: () => {
      if (settled) return;
      settled = true;
      removeListeners();
      element.remove();
      resolveLoad(null);
    },
    loaded
  };
}

function promoteThirdPartyThemeLink(link: HTMLLinkElement) {
  removeActiveThirdPartyThemeElements();
  delete link.dataset.markraThemeCandidate;
  link.id = thirdPartyThemeLinkElementId;
}

function suspendThemeTransitions() {
  const sequence = ++themeTransitionSequence;
  const root = document.documentElement;
  root.setAttribute(themeTransitionAttribute, "suspended");

  window.requestAnimationFrame(() => {
    window.requestAnimationFrame(() => {
      if (sequence !== themeTransitionSequence) return;
      root.removeAttribute(themeTransitionAttribute);
    });
  });
}

function applyThemeRoot(themeId: string, appearance: ThemeAppearance) {
  suspendThemeTransitions();
  removeStartupThemeCss();
  document.documentElement.dataset.theme = themeId;
  document.documentElement.dataset.themeAppearance = appearance;
  document.documentElement.style.colorScheme = appearance;
}

function applyBuiltInTheme(themeId: string, appearance: ThemeAppearance) {
  removeThirdPartyThemeElements();
  applyThemeRoot(themeId, appearance);
}

function isAppAppearanceMode(value: string | null): value is AppAppearanceMode {
  return appAppearanceModeOptions.includes(value as AppAppearanceMode);
}

function startupThemePreferencesFromLocation(): AppThemePreferences | null {
  if (typeof window === "undefined") return null;

  const params = new URLSearchParams(window.location.search);
  const appearanceMode = params.get(startupAppearanceModeParam);
  const lightTheme = params.get(startupLightThemeParam);
  const darkTheme = params.get(startupDarkThemeParam);

  if (!isAppAppearanceMode(appearanceMode) || !isThemeId(lightTheme) || !isThemeId(darkTheme)) return null;

  return normalizeAppThemePreferences({ appearanceMode, darkTheme, lightTheme });
}

export function useAppTheme() {
  const startupThemePreferencesRef = useRef<AppThemePreferences | null>(startupThemePreferencesFromLocation());
  const [themePreferences, setThemePreferences] = useState<AppThemePreferences>(
    () => startupThemePreferencesRef.current ?? defaultAppThemePreferences
  );
  const [systemTheme, setSystemTheme] = useState<ResolvedAppTheme>(() => getSystemTheme());
  const [themePreferencesReady, setThemePreferencesReady] = useState(() => startupThemePreferencesRef.current !== null);
  const [themeActivationReady, setThemeActivationReady] = useState(false);
  const [themeError, setThemeError] = useState<string | null>(null);
  const liveThemePreferencesReceivedRef = useRef(false);
  const activationTokenRef = useRef(0);
  const activeNativeActivationRef = useRef(false);
  const themePreparationQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const repairThemeErrorRef = useRef<string | null>(null);
  const themeCatalog = useThemeCatalog();
  const editorTheme = resolveAppThemePreferencesEditorTheme(themePreferences, systemTheme);
  const resolvedTheme = resolveAppThemePreferencesAppearance(themePreferences, systemTheme);
  const activeTheme = useMemo(
    () => findThemeDescriptor(themeCatalog, editorTheme),
    [editorTheme, themeCatalog]
  );
  const activeBuiltInTheme = activeTheme?.source === "builtin"
    && activeTheme.appearance === resolvedTheme;

  const persistThemePreferences = useCallback(async (nextPreferences: AppThemePreferences) => {
    await saveStoredThemePreferences(nextPreferences);
    await notifyAppThemeChanged(nextPreferences);
  }, []);

  const commitThemePreferences = useCallback((nextPreferences: AppThemePreferences) => {
    const normalizedPreferences = normalizeAppThemePreferences(nextPreferences);
    setThemePreferences(normalizedPreferences);
    liveThemePreferencesReceivedRef.current = true;
    setThemePreferencesReady(true);
    persistThemePreferences(normalizedPreferences).catch((error) => {
      setThemeError(diagnosticErrorMessage(error));
    });
  }, [persistThemePreferences]);

  useEffect(() => {
    let active = true;

    getStoredThemePreferences().then((storedPreferences) => {
      if (active && !liveThemePreferencesReceivedRef.current) setThemePreferences(storedPreferences);
    }).catch((error) => {
      if (active) setThemeError(diagnosticErrorMessage(error));
    }).finally(() => {
      if (active) setThemePreferencesReady(true);
    });

    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;

    const mediaQuery = window.matchMedia(systemDarkThemeQuery);
    const handleSystemThemeChange = (event: MediaQueryListEvent) => {
      setSystemTheme(event.matches ? "dark" : "light");
    };

    setSystemTheme(mediaQuery.matches ? "dark" : "light");
    mediaQuery.addEventListener("change", handleSystemThemeChange);

    return () => {
      mediaQuery.removeEventListener("change", handleSystemThemeChange);
    };
  }, []);

  useEffect(() => {
    let active = true;
    let cleanup: (() => unknown) | null = null;

    listenAppThemeChanged((nextPreferences) => {
      if (!active) return;
      liveThemePreferencesReceivedRef.current = true;
      setThemePreferences(nextPreferences);
      setThemePreferencesReady(true);
    }).then((stopListening) => {
      if (!active) {
        stopListening();
        return;
      }
      cleanup = stopListening;
    }).catch(() => {});

    return () => {
      active = false;
      cleanup?.();
    };
  }, []);

  useEffect(() => {
    const themes = getAppRuntime().themes;

    return () => {
      activationTokenRef.current += 1;
      removeThirdPartyThemeElements();
      if (!activeNativeActivationRef.current) return;
      activeNativeActivationRef.current = false;
      themes.releaseActivation().catch(() => {});
    };
  }, []);

  useLayoutEffect(() => {
    if (!themePreferencesReady) return;

    const token = ++activationTokenRef.current;
    const themes = getAppRuntime().themes;
    setThemeActivationReady(false);
    if (repairThemeErrorRef.current) {
      setThemeError(repairThemeErrorRef.current);
      repairThemeErrorRef.current = null;
    } else {
      setThemeError(null);
    }

    const releaseNativeActivation = (force = false) => {
      if (!force && !activeNativeActivationRef.current) return;
      activeNativeActivationRef.current = false;
      themes.releaseActivation().catch((error) => {
        if (token === activationTokenRef.current) setThemeError(diagnosticErrorMessage(error));
      });
    };

    if (activeBuiltInTheme) {
      applyBuiltInTheme(activeTheme.id, activeTheme.appearance);
      releaseNativeActivation();
      setThemeActivationReady(true);
      return;
    }

    if (themeCatalog.loading) {
      applyBuiltInTheme(resolvedTheme, resolvedTheme);
      return;
    }

    const repairMissingTheme = (activationError?: string) => {
      const fallback = resolvedTheme;
      const repaired = {
        ...themePreferences,
        [resolvedTheme === "dark" ? "darkTheme" : "lightTheme"]: fallback
      };
      applyBuiltInTheme(fallback, resolvedTheme);
      if (activationError) repairThemeErrorRef.current = activationError;
      setThemePreferences(repaired);
      setThemeActivationReady(true);
      persistThemePreferences(repaired).catch((error) => setThemeError(diagnosticErrorMessage(error)));
    };

    if (!activeTheme || activeTheme.appearance !== resolvedTheme) {
      repairMissingTheme();
      return;
    }

    let cancelled = false;
    let pendingActivationToken: string | null = null;
    let pendingStylesheetLoad: PendingStylesheetLoad | null = null;
    let installedElement: HTMLLinkElement | HTMLStyleElement | null = null;
    const sequenceIsCurrent = () => !cancelled && token === activationTokenRef.current;

    const removeInstalledElement = () => {
      if (installedElement?.isConnected) installedElement.remove();
      installedElement = null;
    };

    const cancelPendingActivation = async () => {
      pendingStylesheetLoad?.cancel();
      pendingStylesheetLoad = null;
      const pendingToken = pendingActivationToken;
      pendingActivationToken = null;
      if (pendingToken) await themes.cancelActivation(pendingToken);
    };

    const activateThirdPartyTheme = async (theme: ThemeDescriptor) => {
      try {
        let payload: ThemeActivationPayload | null = null;
        const releasePreparation = await acquireThemePreparation(themePreparationQueueRef);
        try {
          if (!sequenceIsCurrent()) return;
          payload = await themes.prepareActivation(theme.id, theme.fingerprint);
          pendingActivationToken = payload.token;
          if (!sequenceIsCurrent()) {
            await cancelPendingActivation();
            return;
          }
        } finally {
          releasePreparation();
        }
        if (!payload) return;

        if (payload.source.kind === "inline") {
          installedElement = installInlineThirdPartyTheme(payload.source.css);
        } else {
          pendingStylesheetLoad = loadThirdPartyThemeStylesheet(
            payload.source.href,
            payload.token,
            theme.name
          );
          const loadedLink = await pendingStylesheetLoad.loaded;
          pendingStylesheetLoad = null;
          if (!loadedLink || !sequenceIsCurrent()) {
            loadedLink?.remove();
            await cancelPendingActivation();
            return;
          }
          promoteThirdPartyThemeLink(loadedLink);
          installedElement = loadedLink;
        }

        if (!sequenceIsCurrent()) {
          removeInstalledElement();
          await cancelPendingActivation();
          return;
        }
        applyThemeRoot(theme.id, theme.appearance);

        activeNativeActivationRef.current = true;
        await themes.commitActivation(payload.token);
        if (!sequenceIsCurrent()) return;
        pendingActivationToken = null;
        setThemeActivationReady(true);
      } catch (error) {
        removeInstalledElement();
        try {
          await cancelPendingActivation();
        } catch {
          // The original activation error remains the actionable failure.
        }
        if (!sequenceIsCurrent()) return;
        const activationError = diagnosticErrorMessage(error);
        setThemeError(activationError);
        applyBuiltInTheme(theme.appearance, theme.appearance);
        releaseNativeActivation(true);
        repairMissingTheme(activationError);
      }
    };

    activateThirdPartyTheme(activeTheme).catch((error) => setThemeError(diagnosticErrorMessage(error)));

    return () => {
      cancelled = true;
      pendingStylesheetLoad?.cancel();
      pendingStylesheetLoad = null;
      if (pendingActivationToken) removeInstalledElement();
      const pendingToken = pendingActivationToken;
      pendingActivationToken = null;
      if (pendingToken) themes.cancelActivation(pendingToken).catch(() => {});
    };
  }, [
    activeBuiltInTheme,
    activeTheme,
    persistThemePreferences,
    resolvedTheme,
    themeCatalog.loading,
    themePreferences,
    themePreferencesReady
  ]);

  const selectAppearanceMode = useCallback((appearanceMode: AppAppearanceMode) => {
    const nextPreferences = normalizeAppThemePreferences({ ...themePreferences, appearanceMode });
    commitThemePreferences(nextPreferences);
  }, [commitThemePreferences, themePreferences]);

  const selectTheme = useCallback((theme: ThemeDescriptor) => {
    const key = theme.appearance === "dark" ? "darkTheme" : "lightTheme";
    const nextPreferences = normalizeAppThemePreferences({
      ...themePreferences,
      [key]: theme.id
    });

    commitThemePreferences(nextPreferences);
  }, [commitThemePreferences, themePreferences]);

  const selectLightTheme = useCallback((themeId: string) => {
    const theme = themeCatalog.lightThemes.find(({ id }) => id === themeId);
    if (theme) selectTheme(theme);
  }, [selectTheme, themeCatalog.lightThemes]);

  const selectDarkTheme = useCallback((themeId: string) => {
    const theme = themeCatalog.darkThemes.find(({ id }) => id === themeId);
    if (theme) selectTheme(theme);
  }, [selectTheme, themeCatalog.darkThemes]);

  const toggleTheme = useCallback(() => {
    selectAppearanceMode(resolvedTheme === "dark" ? "light" : "dark");
  }, [resolvedTheme, selectAppearanceMode]);

  return {
    activeTheme,
    appearanceMode: themePreferences.appearanceMode,
    catalog: themeCatalog,
    darkTheme: themePreferences.darkTheme,
    editorTheme,
    lightTheme: themePreferences.lightTheme,
    ready: themePreferencesReady
      && themeActivationReady
      && (activeBuiltInTheme || !themeCatalog.loading),
    resolvedTheme,
    selectAppearanceMode,
    selectDarkTheme,
    selectLightTheme,
    selectTheme,
    themeError,
    themePreferences,
    toggleTheme
  };
}
