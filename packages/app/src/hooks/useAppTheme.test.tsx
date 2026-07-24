import { act, renderHook, waitFor } from "@testing-library/react";
import { configureAppRuntime, createDefaultAppRuntime, resetAppRuntimeForTests } from "../runtime";
import {
  approveThemeFingerprint,
  type AppThemePreferences
} from "../lib/settings/app-settings";
import {
  protectedThemeDescriptors,
  type ThemeActivationPayload,
  type ThemeDescriptor
} from "../lib/themes/theme-catalog";
import { useAppTheme } from "./useAppTheme";

const nord: ThemeDescriptor = {
  appearance: "dark",
  author: "Arctic Studio",
  fileName: "nord.css",
  fingerprint: "a".repeat(64),
  id: "nord",
  name: "Nord",
  preview: { accent: "#88c0d0", background: "#2e3440", panel: "#3b4252", text: "#eceff4" },
  source: "third-party",
  storageKind: "inlineCss"
};

const sepia: ThemeDescriptor = {
  appearance: "light",
  fileName: "sepia.css",
  fingerprint: "b".repeat(64),
  id: "sepia",
  name: "Sepia",
  preview: { accent: "#8b5e34", background: "#fbf0d9", panel: "#f3e3c4", text: "#3b2f22" },
  source: "third-party",
  storageKind: "inlineCss"
};

const drakeAyu: ThemeDescriptor = {
  appearance: "dark",
  author: "Jens & Pyrmont",
  fileName: null,
  fingerprint: "c".repeat(64),
  id: "drake-ayu",
  name: "Drake Ayu",
  preview: { accent: "#ffcc66", background: "#0f1419", panel: "#14191f", text: "#e6e1cf" },
  source: "third-party",
  storageKind: "resourceDirectory"
};

const midnight: ThemeDescriptor = {
  appearance: "dark",
  fileName: null,
  fingerprint: "d".repeat(64),
  id: "midnight",
  name: "Midnight",
  preview: { accent: "#61afef", background: "#10131a", panel: "#171b24", text: "#e6e9ef" },
  source: "third-party",
  storageKind: "resourceDirectory"
};

function deferred<T>() {
  let resolvePromise: (value: T) => unknown = () => undefined;
  const promise = new Promise<T>((resolve) => {
    resolvePromise = resolve;
  });

  return { promise, resolve: resolvePromise };
}

function inlineActivation(theme: ThemeDescriptor): ThemeActivationPayload {
  return {
    fingerprint: theme.fingerprint,
    id: theme.id,
    source: { kind: "inline", css: `:root { --loaded-theme: ${theme.id}; }` },
    token: `${theme.id}-deferred-token`
  };
}

function stylesheetActivation(theme: ThemeDescriptor): ThemeActivationPayload {
  return {
    fingerprint: theme.fingerprint,
    id: theme.id,
    source: { kind: "stylesheet", href: `asset://theme.css?fingerprint=${theme.fingerprint}` },
    token: `${theme.id}-deferred-token`
  };
}

function candidateLink(token: string) {
  return document.querySelector<HTMLLinkElement>(`link[data-markra-theme-candidate="${token}"]`);
}

async function waitForCandidate(token: string) {
  return waitFor(() => {
    const link = candidateLink(token);
    expect(link).toBeInTheDocument();
    return link!;
  });
}

function activeLink() {
  return document.getElementById("markra-third-party-theme-link") as HTMLLinkElement | null;
}

function dispatchStylesheetEvent(link: HTMLLinkElement, type: "error" | "load") {
  act(() => {
    link.dispatchEvent(new Event(type));
  });
}

function runtimeWithThemes(preferences: AppThemePreferences, themes: ThemeDescriptor[]) {
  const runtime = createDefaultAppRuntime();
  runtime.settings.readGroup = vi.fn(async (group) => group === "appearance" ? preferences : null) as typeof runtime.settings.readGroup;
  runtime.settings.writeGroup = vi.fn(async () => undefined);
  runtime.themes.list = vi.fn(async () => ({ invalidFiles: [], themes }));
  runtime.themes.prepareActivation = vi.fn(async (id, expectedFingerprint) => {
    const theme = themes.find((candidate) => candidate.id === id);
    return theme?.storageKind === "resourceDirectory"
      ? {
          fingerprint: expectedFingerprint,
          id,
          source: { kind: "stylesheet" as const, href: `asset://themes/${id}/theme.css?fingerprint=${expectedFingerprint}` },
          token: `${id}-token`
        }
      : {
          fingerprint: expectedFingerprint,
          id,
          source: { kind: "inline" as const, css: `:root { --loaded-theme: ${id}; }` },
          token: `${id}-token`
        };
  });
  runtime.themes.commitActivation = vi.fn(async () => undefined);
  runtime.themes.cancelActivation = vi.fn(async () => undefined);
  runtime.themes.releaseActivation = vi.fn(async () => undefined);
  runtime.themes.confirmActivation = vi.fn(async () => true);
  configureAppRuntime(runtime);
  return runtime;
}

describe("useAppTheme", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    document.getElementById("markra-third-party-theme-style")?.remove();
    document.getElementById("markra-third-party-theme-link")?.remove();
    document.querySelectorAll("[data-markra-theme-candidate]").forEach((element) => element.remove());
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-theme-appearance");
    document.documentElement.removeAttribute("data-theme-transition");
    document.documentElement.style.removeProperty("color-scheme");
    resetAppRuntimeForTests();
  });

  it("suppresses transitions until the new protected theme has painted", async () => {
    runtimeWithThemes({ appearanceMode: "light", darkTheme: "dark", lightTheme: "light" }, []);
    const { result } = renderHook(() => useAppTheme());
    await waitFor(() => expect(result.current.ready).toBe(true));
    await waitFor(() => expect(document.documentElement).not.toHaveAttribute("data-theme-transition"));

    const frameCallbacks: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frameCallbacks.push(callback);
      return frameCallbacks.length;
    });

    act(() => result.current.selectAppearanceMode("dark"));

    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    expect(document.documentElement).toHaveAttribute("data-theme-transition", "suspended");
    expect(frameCallbacks).toHaveLength(1);

    act(() => frameCallbacks.shift()?.(0));
    expect(document.documentElement).toHaveAttribute("data-theme-transition", "suspended");
    expect(frameCallbacks).toHaveLength(1);

    act(() => frameCallbacks.shift()?.(16));
    expect(document.documentElement).not.toHaveAttribute("data-theme-transition");
  });

  it("loads and confirms an unapproved stored third-party fingerprint", async () => {
    const runtime = runtimeWithThemes({ appearanceMode: "dark", darkTheme: "nord", lightTheme: "light" }, [nord]);
    const { result } = renderHook(() => useAppTheme());

    await waitFor(() => expect(result.current.ready).toBe(true));

    expect(runtime.themes.prepareActivation).toHaveBeenCalledWith("nord", nord.fingerprint);
    expect(runtime.themes.confirmActivation).toHaveBeenCalledWith("Nord");
    expect(runtime.themes.commitActivation).toHaveBeenCalledWith("nord-token");
    expect(document.documentElement.dataset.theme).toBe("nord");
    expect(document.documentElement.dataset.themeAppearance).toBe("dark");
    expect(document.getElementById("markra-third-party-theme-style")?.textContent).toContain("--loaded-theme: nord");
  });

  it("does not persist a preview selection until confirmation succeeds", async () => {
    const runtime = runtimeWithThemes({ appearanceMode: "light", darkTheme: "dark", lightTheme: "light" }, [sepia]);
    let resolveConfirmation: (accepted: boolean) => unknown = () => undefined;
    runtime.themes.confirmActivation = vi.fn(() => new Promise<boolean>((resolve) => {
      resolveConfirmation = resolve;
    }));
    const { result } = renderHook(() => useAppTheme());
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectTheme(sepia));
    await waitFor(() => expect(runtime.themes.confirmActivation).toHaveBeenCalledWith("Sepia"));
    expect(runtime.settings.writeGroup).not.toHaveBeenCalled();

    await act(async () => resolveConfirmation(true));
    await waitFor(() => expect(runtime.settings.writeGroup).toHaveBeenCalledWith("appearance", expect.objectContaining({ lightTheme: "sepia" })));
    expect(result.current.lightTheme).toBe("sepia");
  });

  it("restores the previous protected theme when guarded preview is rejected", async () => {
    const runtime = runtimeWithThemes({ appearanceMode: "light", darkTheme: "dark", lightTheme: "light" }, [sepia]);
    runtime.themes.confirmActivation = vi.fn(async () => false);
    const { result } = renderHook(() => useAppTheme());
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectTheme(sepia));

    await waitFor(() => expect(result.current.lightTheme).toBe("light"));
    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.getElementById("markra-third-party-theme-style")).not.toBeInTheDocument();
    expect(runtime.settings.writeGroup).not.toHaveBeenCalled();
    expect(runtime.themes.cancelActivation).toHaveBeenCalledWith("sepia-token");
  });

  it("keeps an appearance switch local until its stored third-party theme is approved", async () => {
    const runtime = runtimeWithThemes({ appearanceMode: "dark", darkTheme: "dark", lightTheme: "sepia" }, [sepia]);
    let resolveConfirmation: (accepted: boolean) => unknown = () => undefined;
    runtime.themes.confirmActivation = vi.fn(() => new Promise<boolean>((resolve) => {
      resolveConfirmation = resolve;
    }));
    const { result } = renderHook(() => useAppTheme());
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectAppearanceMode("light"));
    await waitFor(() => expect(runtime.themes.confirmActivation).toHaveBeenCalledWith("Sepia"));
    expect(runtime.settings.writeGroup).not.toHaveBeenCalled();

    await act(async () => resolveConfirmation(true));
    await waitFor(() => expect(runtime.settings.writeGroup).toHaveBeenCalledWith(
      "appearance",
      expect.objectContaining({ appearanceMode: "light", lightTheme: "sepia" })
    ));
  });

  it("repairs a missing selected id to the matching protected default", async () => {
    const runtime = runtimeWithThemes({ appearanceMode: "dark", darkTheme: "missing-theme", lightTheme: "light" }, []);
    const { result } = renderHook(() => useAppTheme());

    await waitFor(() => expect(result.current.darkTheme).toBe("dark"));
    await waitFor(() => expect(runtime.settings.writeGroup).toHaveBeenCalledWith("appearance", expect.objectContaining({ darkTheme: "dark" })));
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("loads a resource candidate before previewing, confirming, and committing it", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: drakeAyu.id, lightTheme: "light" },
      [drakeAyu]
    );
    const { result } = renderHook(() => useAppTheme());

    const candidate = await waitForCandidate("drake-ayu-token");
    expect(candidate).toHaveAttribute("rel", "stylesheet");
    expect(candidate).toHaveAttribute(
      "href",
      `asset://themes/drake-ayu/theme.css?fingerprint=${drakeAyu.fingerprint}`
    );
    expect(result.current.ready).toBe(false);
    expect(runtime.themes.confirmActivation).not.toHaveBeenCalled();
    expect(runtime.themes.commitActivation).not.toHaveBeenCalled();
    expect(document.documentElement.dataset.theme).not.toBe(drakeAyu.id);

    dispatchStylesheetEvent(candidate, "load");

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(runtime.themes.confirmActivation).toHaveBeenCalledWith(drakeAyu.name);
    expect(runtime.themes.commitActivation).toHaveBeenCalledWith("drake-ayu-token");
    expect(activeLink()).toBe(candidate);
    expect(candidate).not.toHaveAttribute("data-markra-theme-candidate");
    expect(document.documentElement.dataset.theme).toBe(drakeAyu.id);
  });

  it("keeps an approved resource theme gated on load while skipping confirmation", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: drakeAyu.id, lightTheme: "light" },
      [drakeAyu]
    );
    await approveThemeFingerprint(drakeAyu.id, drakeAyu.fingerprint);
    const { result } = renderHook(() => useAppTheme());

    const candidate = await waitForCandidate("drake-ayu-token");
    expect(result.current.ready).toBe(false);
    expect(runtime.themes.confirmActivation).not.toHaveBeenCalled();
    expect(runtime.themes.commitActivation).not.toHaveBeenCalled();

    dispatchStylesheetEvent(candidate, "load");

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(runtime.themes.confirmActivation).not.toHaveBeenCalled();
    expect(runtime.themes.commitActivation).toHaveBeenCalledWith("drake-ayu-token");
  });

  it("falls back to the matching protected theme when a resource stylesheet errors", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: drakeAyu.id, lightTheme: "light" },
      [drakeAyu]
    );
    const previousStyle = document.createElement("style");
    previousStyle.id = "markra-third-party-theme-style";
    document.head.append(previousStyle);
    const { result } = renderHook(() => useAppTheme());

    const candidate = await waitForCandidate("drake-ayu-token");
    dispatchStylesheetEvent(candidate, "error");

    await waitFor(() => expect(result.current.darkTheme).toBe("dark"));
    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.themeError).toContain("Drake Ayu");
    expect(runtime.themes.cancelActivation).toHaveBeenCalledWith("drake-ayu-token");
    await waitFor(() => expect(runtime.themes.releaseActivation).toHaveBeenCalled());
    expect(runtime.themes.commitActivation).not.toHaveBeenCalled();
    expect(document.querySelector("#markra-third-party-theme-style, #markra-third-party-theme-link, [data-markra-theme-candidate]"))
      .not.toBeInTheDocument();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(runtime.settings.writeGroup).toHaveBeenCalledWith(
      "appearance",
      expect.objectContaining({ darkTheme: "dark" })
    );
  });

  it("cancels a rejected resource preview and restores preferences without persistence", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: "dark", lightTheme: "light" },
      [drakeAyu]
    );
    runtime.themes.confirmActivation = vi.fn(async () => false);
    const { result } = renderHook(() => useAppTheme());
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectTheme(drakeAyu));
    const candidate = await waitForCandidate("drake-ayu-token");
    expect(runtime.settings.writeGroup).not.toHaveBeenCalled();
    dispatchStylesheetEvent(candidate, "load");

    await waitFor(() => expect(result.current.darkTheme).toBe("dark"));
    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(runtime.settings.writeGroup).not.toHaveBeenCalled();
    expect(runtime.themes.cancelActivation).toHaveBeenCalledWith("drake-ayu-token");
    expect(runtime.themes.commitActivation).not.toHaveBeenCalled();
    expect(activeLink()).not.toBeInTheDocument();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("does not let a rejected activation clear a newer pending selection while cancel waits", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: "dark", lightTheme: "light" },
      [drakeAyu, midnight]
    );
    const slowCancel = deferred<unknown>();
    runtime.themes.cancelActivation = vi.fn((token) => token === "drake-ayu-token"
      ? slowCancel.promise
      : Promise.resolve(undefined));
    runtime.themes.confirmActivation = vi.fn(async (themeName) => themeName !== drakeAyu.name);
    const { result } = renderHook(() => useAppTheme());
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectTheme(drakeAyu));
    const rejectedCandidate = await waitForCandidate("drake-ayu-token");
    dispatchStylesheetEvent(rejectedCandidate, "load");
    await waitFor(() => expect(runtime.themes.cancelActivation).toHaveBeenCalledWith("drake-ayu-token"));

    act(() => result.current.selectTheme(midnight));
    const currentCandidate = await waitForCandidate("midnight-token");
    await act(async () => {
      slowCancel.resolve(undefined);
      await slowCancel.promise;
    });
    dispatchStylesheetEvent(currentCandidate, "load");

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.darkTheme).toBe(midnight.id);
    expect(runtime.settings.writeGroup).toHaveBeenCalledWith(
      "appearance",
      expect.objectContaining({ darkTheme: midnight.id })
    );
    expect(runtime.themes.commitActivation).toHaveBeenCalledWith("midnight-token");
    expect(document.documentElement.dataset.theme).toBe(midnight.id);
  });

  it("prevents a slow resource candidate from winning after a faster selection", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: "dark", lightTheme: "light" },
      [drakeAyu, midnight]
    );
    await approveThemeFingerprint(drakeAyu.id, drakeAyu.fingerprint);
    await approveThemeFingerprint(midnight.id, midnight.fingerprint);
    const { result } = renderHook(() => useAppTheme());
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectTheme(drakeAyu));
    const slowCandidate = await waitForCandidate("drake-ayu-token");
    act(() => result.current.selectTheme(midnight));
    const fastCandidate = await waitForCandidate("midnight-token");

    dispatchStylesheetEvent(fastCandidate, "load");
    await waitFor(() => expect(result.current.ready).toBe(true));
    dispatchStylesheetEvent(slowCandidate, "load");

    expect(runtime.themes.cancelActivation).toHaveBeenCalledWith("drake-ayu-token");
    expect(runtime.themes.commitActivation).not.toHaveBeenCalledWith("drake-ayu-token");
    expect(runtime.themes.commitActivation).toHaveBeenCalledWith("midnight-token");
    expect(activeLink()).toBe(fastCandidate);
    expect(document.documentElement.dataset.theme).toBe(midnight.id);
  });

  it("serializes native preparation and cancels a stale token before preparing the latest selection", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: "dark", lightTheme: "light" },
      [drakeAyu, midnight]
    );
    await approveThemeFingerprint(midnight.id, midnight.fingerprint);
    const slowPrepare = deferred<ThemeActivationPayload>();
    const slowCancel = deferred<unknown>();
    const events: string[] = [];
    let preparesInFlight = 0;
    let maxPreparesInFlight = 0;
    runtime.themes.prepareActivation = vi.fn(async (id) => {
      events.push(`prepare:${id}:start`);
      preparesInFlight += 1;
      maxPreparesInFlight = Math.max(maxPreparesInFlight, preparesInFlight);
      const payload = id === drakeAyu.id
        ? await slowPrepare.promise
        : stylesheetActivation(midnight);
      preparesInFlight -= 1;
      events.push(`prepare:${id}:end`);
      return payload;
    });
    runtime.themes.cancelActivation = vi.fn(async (token) => {
      events.push(`cancel:${token}:start`);
      if (token === "drake-ayu-deferred-token") await slowCancel.promise;
      events.push(`cancel:${token}:end`);
    });
    const { result } = renderHook(() => useAppTheme());
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectTheme(drakeAyu));
    await waitFor(() => expect(runtime.themes.prepareActivation).toHaveBeenCalledWith(
      drakeAyu.id,
      drakeAyu.fingerprint
    ));
    act(() => result.current.selectTheme(midnight));
    expect(runtime.themes.prepareActivation).toHaveBeenCalledTimes(1);

    await act(async () => {
      slowPrepare.resolve(stylesheetActivation(drakeAyu));
      await slowPrepare.promise;
    });
    await waitFor(() => expect(runtime.themes.cancelActivation).toHaveBeenCalledWith(
      "drake-ayu-deferred-token"
    ));
    expect(runtime.themes.prepareActivation).toHaveBeenCalledTimes(1);
    expect(candidateLink("drake-ayu-deferred-token")).not.toBeInTheDocument();

    await act(async () => {
      slowCancel.resolve(undefined);
      await slowCancel.promise;
    });
    const currentCandidate = await waitForCandidate("midnight-deferred-token");
    dispatchStylesheetEvent(currentCandidate, "load");

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(maxPreparesInFlight).toBe(1);
    expect(events).toEqual([
      `prepare:${drakeAyu.id}:start`,
      `prepare:${drakeAyu.id}:end`,
      "cancel:drake-ayu-deferred-token:start",
      "cancel:drake-ayu-deferred-token:end",
      `prepare:${midnight.id}:start`,
      `prepare:${midnight.id}:end`
    ]);
    expect(runtime.themes.commitActivation).not.toHaveBeenCalledWith("drake-ayu-deferred-token");
    expect(runtime.themes.commitActivation).toHaveBeenCalledWith("midnight-deferred-token");
    expect(result.current.darkTheme).toBe(midnight.id);
    expect(document.documentElement.dataset.theme).toBe(midnight.id);
    expect(runtime.settings.writeGroup).not.toHaveBeenCalledWith(
      "appearance",
      expect.objectContaining({ darkTheme: "dark" })
    );
  });

  it("releases a committed resource activation when switching to a protected theme", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: drakeAyu.id, lightTheme: "light" },
      [drakeAyu]
    );
    await approveThemeFingerprint(drakeAyu.id, drakeAyu.fingerprint);
    const { result } = renderHook(() => useAppTheme());
    const candidate = await waitForCandidate("drake-ayu-token");
    dispatchStylesheetEvent(candidate, "load");
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectTheme(protectedThemeDescriptors[1]));

    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    await waitFor(() => expect(runtime.themes.releaseActivation).toHaveBeenCalled());
    expect(activeLink()).not.toBeInTheDocument();
  });

  it("releases a committed resource activation when catalog repair falls back to protected", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: drakeAyu.id, lightTheme: "light" },
      [drakeAyu]
    );
    await approveThemeFingerprint(drakeAyu.id, drakeAyu.fingerprint);
    const { result } = renderHook(() => useAppTheme());
    const candidate = await waitForCandidate("drake-ayu-token");
    dispatchStylesheetEvent(candidate, "load");
    await waitFor(() => expect(result.current.ready).toBe(true));
    runtime.themes.releaseActivation = vi.fn(async () => undefined);
    runtime.themes.list = vi.fn(async () => ({ invalidFiles: [], themes: [] }));

    await act(async () => result.current.catalog.refresh());

    await waitFor(() => expect(result.current.darkTheme).toBe("dark"));
    expect(runtime.themes.releaseActivation).toHaveBeenCalledTimes(1);
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("retains the active resource link until the next resource candidate loads", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: drakeAyu.id, lightTheme: "light" },
      [drakeAyu, midnight]
    );
    await approveThemeFingerprint(drakeAyu.id, drakeAyu.fingerprint);
    await approveThemeFingerprint(midnight.id, midnight.fingerprint);
    const { result } = renderHook(() => useAppTheme());
    const firstCandidate = await waitForCandidate("drake-ayu-token");
    dispatchStylesheetEvent(firstCandidate, "load");
    await waitFor(() => expect(result.current.ready).toBe(true));
    const firstActive = activeLink();

    act(() => result.current.selectTheme(midnight));
    const secondCandidate = await waitForCandidate("midnight-token");
    expect(firstActive).toBeInTheDocument();
    expect(activeLink()).toBe(firstActive);

    dispatchStylesheetEvent(secondCandidate!, "load");

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(firstActive).not.toBeInTheDocument();
    expect(activeLink()).toBe(secondCandidate);
    expect(document.documentElement.dataset.theme).toBe(midnight.id);
  });

  it("cancels a pending resource and releases the active activation on unmount", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: drakeAyu.id, lightTheme: "light" },
      [drakeAyu, midnight]
    );
    await approveThemeFingerprint(drakeAyu.id, drakeAyu.fingerprint);
    const { result, unmount } = renderHook(() => useAppTheme());
    const firstCandidate = await waitForCandidate("drake-ayu-token");
    dispatchStylesheetEvent(firstCandidate, "load");
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectTheme(midnight));
    await waitFor(() => expect(candidateLink("midnight-token")).toBeInTheDocument());
    unmount();

    await waitFor(() => expect(runtime.themes.cancelActivation).toHaveBeenCalledWith("midnight-token"));
    await waitFor(() => expect(runtime.themes.releaseActivation).toHaveBeenCalled());
    expect(document.querySelector("#markra-third-party-theme-style, #markra-third-party-theme-link, [data-markra-theme-candidate]"))
      .not.toBeInTheDocument();
  });

  it("repairs a resource preference and surfaces the error when native preparation fails", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: drakeAyu.id, lightTheme: "light" },
      [drakeAyu]
    );
    runtime.themes.prepareActivation = vi.fn(async () => {
      throw new Error("Theme fingerprint mismatch");
    });
    const { result } = renderHook(() => useAppTheme());

    await waitFor(() => expect(result.current.darkTheme).toBe("dark"));
    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.themeError).toContain("Theme fingerprint mismatch");
    expect(runtime.settings.writeGroup).toHaveBeenCalledWith(
      "appearance",
      expect.objectContaining({ darkTheme: "dark" })
    );
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(runtime.themes.commitActivation).not.toHaveBeenCalled();
  });

  it("cancels a deferred activation once when a later selection makes its effect stale", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "light", darkTheme: "dark", lightTheme: "light" },
      [sepia]
    );
    const pending = deferred<ThemeActivationPayload>();
    runtime.themes.prepareActivation = vi.fn(() => pending.promise);
    const { result } = renderHook(() => useAppTheme());
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.selectTheme(sepia));
    await waitFor(() => expect(runtime.themes.prepareActivation).toHaveBeenCalledWith(
      sepia.id,
      sepia.fingerprint
    ));
    act(() => result.current.selectTheme(protectedThemeDescriptors[0]));
    await waitFor(() => expect(result.current.lightTheme).toBe("light"));

    await act(async () => {
      pending.resolve(inlineActivation(sepia));
      await pending.promise;
    });

    await waitFor(() => expect(runtime.themes.cancelActivation).toHaveBeenCalledTimes(1));
    expect(runtime.themes.cancelActivation).toHaveBeenCalledWith("sepia-deferred-token");
    expect(runtime.themes.commitActivation).not.toHaveBeenCalled();
    expect(document.getElementById("markra-third-party-theme-style")).not.toBeInTheDocument();
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("cancels a prepared token once when the post-prepare branch errors", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: "nord", lightTheme: "light" },
      [nord]
    );
    const pending = deferred<ThemeActivationPayload>();
    runtime.themes.prepareActivation = vi.fn(() => pending.promise);
    runtime.themes.confirmActivation = vi.fn(async () => {
      throw new Error("confirmation failed");
    });
    renderHook(() => useAppTheme());
    await waitFor(() => expect(runtime.themes.prepareActivation).toHaveBeenCalledTimes(1));

    await act(async () => {
      pending.resolve(inlineActivation(nord));
      await pending.promise;
    });

    await waitFor(() => expect(runtime.themes.cancelActivation).toHaveBeenCalledTimes(1));
    expect(runtime.themes.cancelActivation).toHaveBeenCalledWith("nord-deferred-token");
    expect(runtime.themes.commitActivation).not.toHaveBeenCalled();
    expect(document.getElementById("markra-third-party-theme-style")).not.toBeInTheDocument();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("cancels a token once when prepare resolves after unmount", async () => {
    const runtime = runtimeWithThemes(
      { appearanceMode: "dark", darkTheme: "nord", lightTheme: "light" },
      [nord]
    );
    const pending = deferred<ThemeActivationPayload>();
    const payload = stylesheetActivation(nord);
    if (payload.source.kind !== "stylesheet") throw new Error("expected stylesheet payload");
    const stylesheetHref = payload.source.href;
    runtime.themes.prepareActivation = vi.fn(() => pending.promise);
    const { unmount } = renderHook(() => useAppTheme());
    await waitFor(() => expect(runtime.themes.prepareActivation).toHaveBeenCalledTimes(1));

    unmount();
    await act(async () => {
      pending.resolve(payload);
      await pending.promise;
    });

    await waitFor(() => expect(runtime.themes.cancelActivation).toHaveBeenCalledTimes(1));
    expect(runtime.themes.cancelActivation).toHaveBeenCalledWith("nord-deferred-token");
    expect(runtime.themes.commitActivation).not.toHaveBeenCalled();
    expect(document.getElementById("markra-third-party-theme-style")).not.toBeInTheDocument();
    const staleLink = [...document.querySelectorAll<HTMLLinkElement>("link")]
      .find((link) => link.getAttribute("href") === stylesheetHref);
    expect(staleLink).toBeUndefined();
    expect(document.documentElement.dataset.theme).not.toBe("nord");
  });
});
