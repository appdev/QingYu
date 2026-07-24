import { act, renderHook } from "@testing-library/react";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppFormFactor
} from "../runtime";
import { useCompactMode } from "./useCompactMode";

const compactViewportQuery = "(max-width: 720px)";

function installCompactViewport(matches: boolean) {
  let currentMatches = matches;
  const listeners = new Set<(event: MediaQueryListEvent) => unknown>();
  const mediaQuery = {
    get matches() {
      return currentMatches;
    },
    media: compactViewportQuery,
    onchange: null,
    addEventListener: vi.fn((_event: "change", listener: (event: MediaQueryListEvent) => unknown) => {
      listeners.add(listener);
    }),
    removeEventListener: vi.fn((_event: "change", listener: (event: MediaQueryListEvent) => unknown) => {
      listeners.delete(listener);
    }),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn()
  } as unknown as MediaQueryList;
  const matchMedia = vi.fn((query: string) => {
    expect(query).toBe(compactViewportQuery);
    return mediaQuery;
  });

  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: matchMedia
  });

  return {
    matchMedia,
    mediaQuery,
    setMatches(nextMatches: boolean) {
      currentMatches = nextMatches;
      const event = { matches: nextMatches, media: compactViewportQuery } as MediaQueryListEvent;
      listeners.forEach((listener) => listener(event));
    }
  };
}

function configureFormFactor(formFactor: AppFormFactor) {
  const runtime = createDefaultAppRuntime();
  const resolveFormFactor = vi.fn(() => formFactor);

  configureAppRuntime({
    ...runtime,
    platform: {
      ...runtime.platform,
      resolveFormFactor
    }
  });

  return resolveFormFactor;
}

describe("useCompactMode", () => {
  afterEach(() => {
    resetAppRuntimeForTests();
  });

  it.each([
    { formFactor: "mobile", matches: false, compact: true },
    { formFactor: "mobile", matches: true, compact: true },
    { formFactor: "desktop", matches: true, compact: true },
    { formFactor: "desktop", matches: false, compact: false }
  ] satisfies Array<{ compact: boolean; formFactor: AppFormFactor; matches: boolean }>)(
    "derives Compact from runtime or viewport for $formFactor with matches=$matches",
    ({ compact, formFactor, matches }) => {
      const resolveFormFactor = configureFormFactor(formFactor);
      const viewport = installCompactViewport(matches);

      const { result } = renderHook(() => useCompactMode());

      expect(result.current).toEqual({
        compact,
        formFactor,
        trueMobile: formFactor === "mobile"
      });
      expect(resolveFormFactor).toHaveBeenCalledTimes(1);
      expect(viewport.matchMedia).toHaveBeenCalledTimes(1);
    }
  );

  it("subscribes once, responds to viewport changes, and removes the listener", () => {
    const resolveFormFactor = configureFormFactor("desktop");
    const viewport = installCompactViewport(false);

    const { result, unmount } = renderHook(() => useCompactMode());

    expect(viewport.mediaQuery.addEventListener).toHaveBeenCalledTimes(1);
    expect(result.current.compact).toBe(false);

    act(() => viewport.setMatches(true));

    expect(result.current.compact).toBe(true);
    expect(resolveFormFactor).toHaveBeenCalledTimes(1);

    unmount();

    expect(viewport.mediaQuery.removeEventListener).toHaveBeenCalledTimes(1);
    expect(viewport.mediaQuery.removeEventListener).toHaveBeenCalledWith(
      "change",
      vi.mocked(viewport.mediaQuery.addEventListener).mock.calls[0]?.[1]
    );
  });
});
