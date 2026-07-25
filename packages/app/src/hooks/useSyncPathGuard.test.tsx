import { act, render, renderHook, waitFor } from "@testing-library/react";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type RuntimeEvent
} from "../runtime";
import {
  syncPathGuardReleaseEvent,
  syncPathGuardRequestEvent,
  type SyncPathGuardRelease,
  type SyncPathGuardRequest
} from "../lib/sync-path-events";
import { useSyncPathGuard } from "./useSyncPathGuard";
import { SyncPathMutationRegistry } from "../lib/sync-path-mutations";

describe("useSyncPathGuard", () => {
  const root = "/notes";
  const listeners = new Map<string, (event: RuntimeEvent<unknown>) => unknown>();
  const acknowledge = vi.fn(async () => undefined);
  let mutationRegistry: SyncPathMutationRegistry;

  beforeEach(() => {
    listeners.clear();
    mutationRegistry = new SyncPathMutationRegistry();
    acknowledge.mockClear();
    const runtime = createDefaultAppRuntime();
    runtime.events.isAvailable = () => true;
    runtime.events.listen = vi.fn(async (event, handler) => {
      listeners.set(event, handler as (event: RuntimeEvent<unknown>) => unknown);
      return () => listeners.delete(event);
    });
    runtime.syncPathGuard.acknowledge = acknowledge;
    configureAppRuntime(runtime);
  });

  afterEach(() => resetAppRuntimeForTests());

  function request(
    requestId: string,
    relativePaths: string[]
  ): SyncPathGuardRequest {
    return {
      jobId: "319b5308-1e93-4909-95ac-cd198cc454ac",
      notesRoot: root,
      relativePaths,
      requestId
    };
  }

  async function publish<T>(event: string, payload: T) {
    await act(async () => {
      await listeners.get(event)?.({ payload });
    });
  }

  it("flushes exact paths, acknowledges, and releases overlapping requests independently", async () => {
    const saveDirtyMarkdownPaths = vi.fn(async () => true);
    const { result } = renderHook(() => useSyncPathGuard({
      enabled: true,
      mutationRegistry,
      notesRoot: root,
      saveDirtyMarkdownPaths
    }));
    await waitFor(() => expect(listeners.has(syncPathGuardRequestEvent)).toBe(true));
    const first = request("e728a5d6-31ed-490d-bb8a-8f15cb550e74", ["shared.md", "one.md"]);
    const second = request("a56fc2d3-b85e-4e72-883b-27d52573fda9", ["shared.md", "two.md"]);

    await publish(syncPathGuardRequestEvent, first);
    await publish(syncPathGuardRequestEvent, second);
    expect(saveDirtyMarkdownPaths).toHaveBeenNthCalledWith(1, ["/notes/shared.md", "/notes/one.md"]);
    expect(saveDirtyMarkdownPaths).toHaveBeenNthCalledWith(2, ["/notes/shared.md", "/notes/two.md"]);
    expect(acknowledge).toHaveBeenCalledTimes(2);
    expect([...result.current.guardedPaths].sort()).toEqual([
      "/notes/one.md",
      "/notes/shared.md",
      "/notes/two.md"
    ]);

    await publish<SyncPathGuardRelease>(syncPathGuardReleaseEvent, {
      notesRoot: root,
      relativePaths: first.relativePaths,
      requestId: first.requestId
    });
    expect([...result.current.guardedPaths].sort()).toEqual([
      "/notes/shared.md",
      "/notes/two.md"
    ]);
    await publish<SyncPathGuardRelease>(syncPathGuardReleaseEvent, {
      notesRoot: root,
      relativePaths: second.relativePaths,
      requestId: second.requestId
    });
    expect(result.current.guardedPaths.size).toBe(0);
  });

  it("installs release handling before accepting requests", async () => {
    const runtime = createDefaultAppRuntime();
    const installed: string[] = [];
    runtime.events.isAvailable = () => true;
    runtime.events.listen = vi.fn(async (event) => {
      installed.push(event);
      return () => undefined;
    });
    runtime.syncPathGuard.acknowledge = acknowledge;
    configureAppRuntime(runtime);
    const saveDirtyMarkdownPaths = vi.fn(async () => true);

    renderHook(() => useSyncPathGuard({
      enabled: true,
      mutationRegistry,
      notesRoot: root,
      saveDirtyMarkdownPaths
    }));
    await waitFor(() => expect(installed).toHaveLength(2));

    expect(installed).toEqual([syncPathGuardReleaseEvent, syncPathGuardRequestEvent]);
  });

  it("acknowledges only after the guarded paths have committed to the rendered hook state", async () => {
    const saveDirtyMarkdownPaths = vi.fn(async () => true);
    const Harness = () => {
      const { guardedPaths } = useSyncPathGuard({
        enabled: true,
        mutationRegistry,
        notesRoot: root,
        saveDirtyMarkdownPaths
      });
      return <div data-testid="guard-state">{guardedPaths.has("/notes/guarded.md") ? "guarded" : "open"}</div>;
    };
    const rendered = render(<Harness />);
    const guardStateAtAcknowledgement: string[] = [];
    acknowledge.mockImplementationOnce(async () => {
      guardStateAtAcknowledgement.push(rendered.getByTestId("guard-state").textContent ?? "");
    });
    await waitFor(() => expect(listeners.has(syncPathGuardRequestEvent)).toBe(true));

    await publish(syncPathGuardRequestEvent, request(
      "e728a5d6-31ed-490d-bb8a-8f15cb550e74",
      ["guarded.md"]
    ));

    expect(guardStateAtAcknowledgement).toEqual(["guarded"]);
  });

  it("does not acknowledge or guard after a failed save", async () => {
    const saveDirtyMarkdownPaths = vi.fn(async () => false);
    const { result } = renderHook(() => useSyncPathGuard({
      enabled: true,
      mutationRegistry,
      notesRoot: root,
      saveDirtyMarkdownPaths
    }));
    await waitFor(() => expect(listeners.has(syncPathGuardRequestEvent)).toBe(true));

    await publish(syncPathGuardRequestEvent, request(
      "e728a5d6-31ed-490d-bb8a-8f15cb550e74",
      ["blocked.md"]
    ));
    expect(acknowledge).not.toHaveBeenCalled();
    expect(result.current.guardedPaths.size).toBe(0);
  });

  it("ignores traversal payloads and releases for a different request", async () => {
    const saveDirtyMarkdownPaths = vi.fn(async () => true);
    const { result } = renderHook(() => useSyncPathGuard({
      enabled: true,
      mutationRegistry,
      notesRoot: root,
      saveDirtyMarkdownPaths
    }));
    await waitFor(() => expect(listeners.has(syncPathGuardRequestEvent)).toBe(true));

    await publish(syncPathGuardRequestEvent, request(
      "e728a5d6-31ed-490d-bb8a-8f15cb550e74",
      ["../secret.md"]
    ));
    expect(saveDirtyMarkdownPaths).not.toHaveBeenCalled();
    expect(acknowledge).not.toHaveBeenCalled();

    const valid = request("a56fc2d3-b85e-4e72-883b-27d52573fda9", ["kept.md"]);
    await publish(syncPathGuardRequestEvent, valid);
    await publish<SyncPathGuardRelease>(syncPathGuardReleaseEvent, {
      notesRoot: root,
      relativePaths: ["kept.md"],
      requestId: "e728a5d6-31ed-490d-bb8a-8f15cb550e74"
    });
    expect(result.current.guardedPaths.has("/notes/kept.md")).toBe(true);
  });

  it("stops waiting before the native 15-second timeout and never acknowledges", async () => {
    vi.useFakeTimers();
    try {
      const saveDirtyMarkdownPaths = vi.fn(() => new Promise<boolean>(() => {}));
      renderHook(() => useSyncPathGuard({
        enabled: true,
        mutationRegistry,
        notesRoot: root,
        saveDirtyMarkdownPaths
      }));
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });
      const handling = listeners.get(syncPathGuardRequestEvent)?.({
        payload: request("e728a5d6-31ed-490d-bb8a-8f15cb550e74", ["kept.md"])
      });

      await act(async () => {
        await vi.advanceTimersByTimeAsync(14_500);
        await handling;
      });

      expect(saveDirtyMarkdownPaths).toHaveBeenCalledTimes(1);
      expect(acknowledge).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("waits for an affected in-flight mutation and blocks new intersections while preparing", async () => {
    const mutationRegistry = new SyncPathMutationRegistry();
    const activeLease = mutationRegistry.acquire({ sourcePath: "/notes/folder/guarded.md" });
    expect(activeLease).not.toBeNull();
    const saveDirtyMarkdownPaths = vi.fn(async () => true);
    renderHook(() => useSyncPathGuard({
      enabled: true,
      mutationRegistry,
      notesRoot: root,
      saveDirtyMarkdownPaths
    }));
    await waitFor(() => expect(listeners.has(syncPathGuardRequestEvent)).toBe(true));

    const handling = listeners.get(syncPathGuardRequestEvent)?.({
      payload: request("e728a5d6-31ed-490d-bb8a-8f15cb550e74", ["folder/guarded.md"])
    });
    await act(async () => Promise.resolve());

    expect(acknowledge).not.toHaveBeenCalled();
    expect(saveDirtyMarkdownPaths).not.toHaveBeenCalled();
    expect(mutationRegistry.acquire({ destinationPath: "/notes/folder/guarded.md" })).toBeNull();
    const unrelatedLease = mutationRegistry.acquire({ sourcePath: "/notes/other.md" });
    expect(unrelatedLease).not.toBeNull();
    unrelatedLease?.release();

    activeLease?.release();
    await act(async () => {
      await handling;
    });
    await waitFor(() => expect(acknowledge).toHaveBeenCalledTimes(1));
    expect(saveDirtyMarkdownPaths).toHaveBeenCalledTimes(1);
  });

  it("does not delay acknowledgement for an unrelated in-flight mutation", async () => {
    const mutationRegistry = new SyncPathMutationRegistry();
    const unrelatedLease = mutationRegistry.acquire({ sourcePath: "/notes/other.md" });
    const saveDirtyMarkdownPaths = vi.fn(async () => true);
    renderHook(() => useSyncPathGuard({
      enabled: true,
      mutationRegistry,
      notesRoot: root,
      saveDirtyMarkdownPaths
    }));
    await waitFor(() => expect(listeners.has(syncPathGuardRequestEvent)).toBe(true));

    await publish(syncPathGuardRequestEvent, request(
      "e728a5d6-31ed-490d-bb8a-8f15cb550e74",
      ["guarded.md"]
    ));

    expect(acknowledge).toHaveBeenCalledTimes(1);
    unrelatedLease?.release();
  });
});
