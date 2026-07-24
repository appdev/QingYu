import { act, renderHook, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import {
  compactNavigationReducer,
  useCompactNavigation,
  type CompactNavigation,
  type CompactNavigationState,
  type CompactOverlayPage,
  type CompactPage
} from "./useCompactNavigation";

const editorState: CompactNavigationState = [{ kind: "editor" }];
const compactHistoryMarkerKey = "__markraCompactNavigation";

function historyMarker(state: unknown) {
  return (state as Record<string, {
    sessionId: string;
    stack: CompactNavigationState;
  }>)[compactHistoryMarkerKey];
}

function reduce(state: CompactNavigationState, type: "push" | "replace", page: CompactOverlayPage) {
  return compactNavigationReducer(state, { type, page });
}

describe("compactNavigationReducer", () => {
  it("navigates from editor through files and move target, then back to editor", () => {
    let state = reduce(editorState, "push", { kind: "files" });
    state = reduce(state, "push", { kind: "move-target", path: "/notes/draft.md" });

    state = compactNavigationReducer(state, { type: "pop" });
    expect(state.at(-1)).toEqual({ kind: "files" });

    state = compactNavigationReducer(state, { type: "pop" });
    expect(state).toEqual(editorState);
  });

  it("navigates from editor through settings detail, then back to editor", () => {
    let state = reduce(editorState, "push", { kind: "settings" });
    state = reduce(state, "push", { kind: "settings-detail", category: "appearance" });

    state = compactNavigationReducer(state, { type: "pop" });
    expect(state.at(-1)).toEqual({ kind: "settings" });

    state = compactNavigationReducer(state, { type: "pop" });
    expect(state).toEqual(editorState);
  });

  it("navigates from editor through sync form, then back to editor", () => {
    let state = reduce(editorState, "push", { kind: "sync-status" });
    state = reduce(state, "push", { kind: "sync-form", mode: "edit" });

    state = compactNavigationReducer(state, { type: "pop" });
    expect(state.at(-1)).toEqual({ kind: "sync-status" });

    state = compactNavigationReducer(state, { type: "pop" });
    expect(state).toEqual(editorState);
  });

  it("does not create duplicate adjacent destinations", () => {
    const filesState = reduce(editorState, "push", { kind: "files" });

    expect(reduce(filesState, "push", { kind: "files" })).toBe(filesState);

    const firstTarget = reduce(filesState, "push", { kind: "move-target", path: "/notes" });
    expect(reduce(firstTarget, "push", { kind: "move-target", path: "/notes" })).toBe(firstTarget);
    expect(reduce(firstTarget, "push", { kind: "move-target", path: "/archive" })).toHaveLength(4);
  });

  it("keeps editor at stack index zero for replace and pop-to-editor", () => {
    const filesState = reduce(editorState, "replace", { kind: "files" });
    expect(filesState).toEqual([{ kind: "editor" }, { kind: "files" }]);

    const settingsState = reduce(filesState, "replace", { kind: "settings" });
    expect(settingsState).toEqual([{ kind: "editor" }, { kind: "settings" }]);

    expect(compactNavigationReducer(settingsState, { type: "pop-to-editor" })).toEqual(editorState);
    expect(compactNavigationReducer(editorState, { type: "pop" })).toBe(editorState);
  });
});

describe("useCompactNavigation", () => {
  beforeEach(() => {
    window.history.replaceState({}, "", "/");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("restores the marker target on browser Back and Forward", async () => {
    const pushState = vi.spyOn(window.history, "pushState");
    const { result } = renderHook(() => useCompactNavigation());

    act(() => result.current.push({ kind: "files" }));
    act(() => result.current.push({ kind: "move-target", path: "/notes" }));

    expect(pushState).toHaveBeenCalledTimes(2);
    expect(result.current.page).toEqual({ kind: "move-target", path: "/notes" });
    const filesHistoryState = pushState.mock.calls[0]?.[0];
    const moveTargetHistoryState = pushState.mock.calls[1]?.[0];

    act(() => window.dispatchEvent(new PopStateEvent("popstate", { state: filesHistoryState })));

    await waitFor(() => expect(result.current.page).toEqual({ kind: "files" }));

    act(() => window.dispatchEvent(new PopStateEvent("popstate", { state: moveTargetHistoryState })));

    await waitFor(() => expect(result.current.page).toEqual({ kind: "move-target", path: "/notes" }));
  });

  it("does not consume a popstate entry outside the Compact history session", async () => {
    const { result } = renderHook(() => useCompactNavigation());
    act(() => result.current.push({ kind: "files" }));

    act(() => window.dispatchEvent(new PopStateEvent("popstate", { state: { external: true } })));

    await act(async () => Promise.resolve());
    expect(result.current.page).toEqual({ kind: "files" });
  });

  it("awaits the page-exit action before popping a sync form", async () => {
    let finishExit: (() => unknown) | undefined;
    const exit = vi.fn(() => new Promise<unknown>((resolve) => {
      finishExit = () => resolve(undefined);
    }));
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    const { result } = renderHook(() => useCompactNavigation({ onBeforePop: (page) => {
      if (page.kind === "sync-form") return exit();
      return undefined;
    } }));

    act(() => result.current.push({ kind: "sync-status" }));
    act(() => result.current.push({ kind: "sync-form", mode: "recover" }));

    let backResult: Promise<boolean> | undefined;
    act(() => {
      backResult = result.current.pop();
    });

    expect(exit).toHaveBeenCalledTimes(1);
    expect(result.current.page).toEqual({ kind: "sync-form", mode: "recover" });

    await act(async () => {
      finishExit?.();
      await backResult;
    });

    expect(result.current.page).toEqual({ kind: "sync-status" });
  });

  it("routes a native back subscription through the same pop action", async () => {
    let nativeBack: (() => Promise<boolean>) | undefined;
    const subscribeToSystemBack = vi.fn(async (handler: () => Promise<boolean>) => {
      nativeBack = handler;
      return vi.fn();
    });
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    const { result } = renderHook(() => useCompactNavigation({ subscribeToSystemBack }));
    const editorHistoryState = window.history.state;

    act(() => result.current.push({ kind: "settings" }));
    let consumed: boolean | undefined;
    await act(async () => {
      consumed = await nativeBack?.();
    });

    await waitFor(() => expect(result.current.page).toEqual({ kind: "editor" }));
    expect(consumed).toBe(true);
    await act(async () => {
      window.dispatchEvent(new PopStateEvent("popstate", { state: editorHistoryState }));
      await Promise.resolve();
    });
    await expect(nativeBack?.()).resolves.toBe(false);
    expect(subscribeToSystemBack).toHaveBeenCalledTimes(1);
  });

  it("cleans up an async native Back registration that resolves after unmount", async () => {
    let finishRegistration: ((cleanup: () => unknown) => unknown) | undefined;
    const cleanup = vi.fn();
    const subscribeToSystemBack = vi.fn(() => new Promise<() => unknown>((resolve) => {
      finishRegistration = resolve;
    }));
    const rendered = renderHook(() => useCompactNavigation({ subscribeToSystemBack }));

    expect(subscribeToSystemBack).toHaveBeenCalledTimes(1);
    rendered.unmount();
    await act(async () => {
      finishRegistration?.(cleanup);
      await Promise.resolve();
    });

    expect(cleanup).toHaveBeenCalledTimes(1);
  });

  it("coalesces two native back events while sync exit is pending", async () => {
    let finishExit: (() => unknown) | undefined;
    const exit = vi.fn(() => new Promise<unknown>((resolve) => {
      finishExit = () => resolve(undefined);
    }));
    let nativeBack: (() => Promise<boolean>) | undefined;
    const subscribeToSystemBack = async (handler: () => Promise<boolean>) => {
      nativeBack = handler;
      return vi.fn();
    };
    const pushState = vi.spyOn(window.history, "pushState");
    const historyBack = vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    const { result } = renderHook(() => useCompactNavigation({
      onBeforePop: (page) => page.kind === "sync-form" ? exit() : undefined,
      subscribeToSystemBack
    }));

    act(() => result.current.push({ kind: "sync-status" }));
    const syncStatusHistoryState = pushState.mock.calls[0]?.[0];
    act(() => result.current.push({ kind: "sync-form", mode: "edit" }));

    let firstBack: Promise<boolean> | undefined;
    let secondBack: Promise<boolean> | undefined;
    act(() => {
      firstBack = nativeBack?.();
      secondBack = nativeBack?.();
    });

    expect(exit).toHaveBeenCalledTimes(1);
    expect(result.current.page).toEqual({ kind: "sync-form", mode: "edit" });

    await act(async () => {
      finishExit?.();
      await Promise.all([firstBack, secondBack]);
    });

    await expect(firstBack).resolves.toBe(true);
    await expect(secondBack).resolves.toBe(true);
    expect(result.current.page).toEqual({ kind: "sync-status" });
    expect(historyBack).toHaveBeenCalledTimes(1);

    await act(async () => {
      window.dispatchEvent(new PopStateEvent("popstate", { state: syncStatusHistoryState }));
      await Promise.resolve();
    });
    expect(result.current.stack).toEqual([{ kind: "editor" }, { kind: "sync-status" }]);
  });

  it("reconciles to the latest browser target while sync exit is pending", async () => {
    let finishExit: (() => unknown) | undefined;
    const exit = vi.fn(() => new Promise<unknown>((resolve) => {
      finishExit = () => resolve(undefined);
    }));
    const pushState = vi.spyOn(window.history, "pushState");
    const { result } = renderHook(() => useCompactNavigation({
      onBeforePop: (page) => page.kind === "sync-form" ? exit() : undefined
    }));
    const editorHistoryState = window.history.state;

    act(() => result.current.push({ kind: "sync-status" }));
    const syncStatusHistoryState = pushState.mock.calls[0]?.[0];
    act(() => result.current.push({ kind: "sync-form", mode: "recover" }));

    act(() => {
      window.dispatchEvent(new PopStateEvent("popstate", { state: syncStatusHistoryState }));
      window.dispatchEvent(new PopStateEvent("popstate", { state: editorHistoryState }));
    });

    expect(exit).toHaveBeenCalledTimes(1);
    expect(result.current.page).toEqual({ kind: "sync-form", mode: "recover" });

    await act(async () => {
      finishExit?.();
    });

    expect(result.current.page).toEqual({ kind: "editor" });
  });

  it("locks a successful guarded Back target when Forward arrives during exit", async () => {
    let finishExit: (() => unknown) | undefined;
    const exit = vi.fn(() => new Promise<unknown>((resolve) => {
      finishExit = () => resolve(undefined);
    }));
    const pushState = vi.spyOn(window.history, "pushState");
    const historyGo = vi.spyOn(window.history, "go").mockImplementation(() => undefined);
    const { result } = renderHook(() => useCompactNavigation({
      onBeforePop: (page) => page.kind === "sync-form" ? exit() : undefined
    }));

    act(() => result.current.push({ kind: "sync-status" }));
    const syncStatusHistoryState = pushState.mock.calls[0]?.[0];
    act(() => result.current.push({ kind: "sync-form", mode: "edit" }));
    const syncFormHistoryState = pushState.mock.calls[1]?.[0];

    act(() => {
      window.dispatchEvent(new PopStateEvent("popstate", { state: syncStatusHistoryState }));
      window.dispatchEvent(new PopStateEvent("popstate", { state: syncFormHistoryState }));
    });

    expect(exit).toHaveBeenCalledTimes(1);
    expect(result.current.page).toEqual({ kind: "sync-form", mode: "edit" });

    await act(async () => {
      finishExit?.();
    });

    expect(result.current.stack).toEqual([{ kind: "editor" }, { kind: "sync-status" }]);
    expect(historyGo).toHaveBeenCalledWith(-1);

    await act(async () => {
      window.dispatchEvent(new PopStateEvent("popstate", { state: syncStatusHistoryState }));
      await Promise.resolve();
    });
    expect(result.current.stack).toEqual(historyMarker(syncStatusHistoryState)?.stack);

    act(() => {
      window.dispatchEvent(new PopStateEvent("popstate", { state: syncFormHistoryState }));
    });
    await act(async () => Promise.resolve());

    expect(result.current.stack).toEqual([{ kind: "editor" }, { kind: "sync-status" }]);
    expect(historyGo).toHaveBeenCalledTimes(2);
  });

  it("consumes rejected native back above root and surfaces the navigation error", async () => {
    const exitError = new Error("sync settings exit failed");
    const exit = vi.fn().mockRejectedValue(exitError);
    const onNavigationError = vi.fn();
    let nativeBack: (() => Promise<boolean>) | undefined;
    const subscribeToSystemBack = async (handler: () => Promise<boolean>) => {
      nativeBack = handler;
      return vi.fn();
    };
    const historyBack = vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    const { result } = renderHook(() => useCompactNavigation({
      onBeforePop: (page) => page.kind === "sync-form" ? exit() : undefined,
      onNavigationError,
      subscribeToSystemBack
    }));

    act(() => result.current.push({ kind: "sync-status" }));
    act(() => result.current.push({ kind: "sync-form", mode: "recover" }));

    let consumed: boolean | undefined;
    await act(async () => {
      consumed = await nativeBack?.();
    });

    expect(consumed).toBe(true);
    expect(result.current.stack).toEqual([
      { kind: "editor" },
      { kind: "sync-status" },
      { kind: "sync-form", mode: "recover" }
    ]);
    expect(exit).toHaveBeenCalledTimes(1);
    expect(onNavigationError).toHaveBeenCalledWith(exitError);
    expect(historyBack).not.toHaveBeenCalled();
  });

  it("uses one reload-unique history session id across rerenders and ignores stale markers", async () => {
    const activeSessionId = "00000000-0000-4000-8000-000000000001";
    const randomUUID = vi.spyOn(globalThis.crypto, "randomUUID").mockReturnValue(activeSessionId);
    const pushState = vi.spyOn(window.history, "pushState");
    const { result, rerender } = renderHook(() => useCompactNavigation());
    const activeEditorHistoryState = window.history.state;

    rerender();
    act(() => result.current.push({ kind: "files" }));
    const activeFilesHistoryState = pushState.mock.calls[0]?.[0];
    const staleSameDepthHistoryState = {
      ...activeFilesHistoryState,
      [compactHistoryMarkerKey]: {
        ...historyMarker(activeFilesHistoryState),
        sessionId: "prior-reloaded-session",
        stack: [{ kind: "editor" }, { kind: "settings" }]
      }
    };

    act(() => {
      window.dispatchEvent(new PopStateEvent("popstate", { state: staleSameDepthHistoryState }));
    });
    await act(async () => Promise.resolve());

    expect(randomUUID).toHaveBeenCalledTimes(1);
    expect(historyMarker(activeEditorHistoryState)?.sessionId).toBe(activeSessionId);
    expect(historyMarker(activeFilesHistoryState)?.sessionId).toBe(activeSessionId);
    expect(result.current.page).toEqual({ kind: "files" });
  });

  it("rejects editor as a push or replace target and requires awaited pop-to-editor", async () => {
    type PushTarget = Parameters<CompactNavigation["push"]>[0];
    type ReplaceTarget = Parameters<CompactNavigation["replace"]>[0];
    expectTypeOf<{ kind: "editor" }>().not.toMatchTypeOf<PushTarget>();
    expectTypeOf<{ kind: "editor" }>().not.toMatchTypeOf<ReplaceTarget>();

    const exit = vi.fn().mockResolvedValue(undefined);
    const pushState = vi.spyOn(window.history, "pushState");
    vi.spyOn(window.history, "go").mockImplementation(() => undefined);
    const { result } = renderHook(() => useCompactNavigation({
      onBeforePop: (page) => page.kind === "sync-form" ? exit() : undefined
    }));

    act(() => result.current.push({ kind: "sync-status" }));
    act(() => result.current.push({ kind: "sync-form", mode: "edit" }));
    const historyTransitionCount = pushState.mock.calls.length;

    let editorPushAccepted: boolean | undefined;
    let editorReplaceAccepted: boolean | undefined;
    act(() => {
      editorPushAccepted = result.current.push({ kind: "editor" } as never);
      editorReplaceAccepted = result.current.replace({ kind: "editor" } as never);
    });
    expect(editorPushAccepted).toBe(false);
    expect(editorReplaceAccepted).toBe(false);
    expect(result.current.page).toEqual({ kind: "sync-form", mode: "edit" });
    expect(pushState).toHaveBeenCalledTimes(historyTransitionCount);
    expect(exit).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.popToEditor();
    });

    expect(exit).toHaveBeenCalledTimes(1);
    expect(result.current.page).toEqual({ kind: "editor" });
  });

  it("does not consume back at the editor root", async () => {
    const historyBack = vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    const { result } = renderHook(() => useCompactNavigation());

    await expect(result.current.pop()).resolves.toBe(false);

    expect(result.current.page).toEqual({ kind: "editor" });
    expect(result.current.canGoBack).toBe(false);
    expect(historyBack).not.toHaveBeenCalled();
  });

  it("pops only when the expected page is still current", async () => {
    const historyBack = vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    const { result } = renderHook(() => useCompactNavigation());
    const movePage = { kind: "move-target" as const, path: "/notes/draft.md" };

    act(() => result.current.push({ kind: "files" }));
    act(() => result.current.push(movePage));
    await act(async () => result.current.pop());
    expect(result.current.page).toEqual({ kind: "files" });
    historyBack.mockClear();

    await expect(result.current.popIfCurrent(movePage)).resolves.toBe(false);
    expect(result.current.page).toEqual({ kind: "files" });
    expect(historyBack).not.toHaveBeenCalled();
  });
});
