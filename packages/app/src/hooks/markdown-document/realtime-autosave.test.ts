import { act } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  createRealtimeMarkdownAutoSaveController,
  realtimeMarkdownAutoSaveIdleMs
} from "./realtime-autosave";

function deferred<T>() {
  let resolvePromise: (value: T) => unknown = () => undefined;
  let rejectPromise: (reason: unknown) => unknown = () => undefined;
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve;
    rejectPromise = reject;
  });
  return { promise, reject: rejectPromise, resolve: resolvePromise };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("realtime Markdown auto-save controller", () => {
  it("waits for a full idle window and resets it after every edit", async () => {
    vi.useFakeTimers();
    let dirty = true;
    const saveLatest = vi.fn(async () => {
      dirty = false;
      return "durable" as const;
    });
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => dirty,
      onError: () => undefined,
      saveLatest
    });

    controller.schedule("tab-a");
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs - 1));
    expect(saveLatest).not.toHaveBeenCalled();

    controller.schedule("tab-a");
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs - 1));
    expect(saveLatest).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(1));
    expect(saveLatest).toHaveBeenCalledTimes(1);
  });

  it("does not force a max-wait save while edits keep resetting the idle timer", async () => {
    vi.useFakeTimers();
    const saveLatest = vi.fn(async () => "durable" as const);
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => true,
      onError: () => undefined,
      saveLatest
    });

    controller.schedule("tab-a");
    for (let edit = 0; edit < 8; edit += 1) {
      await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs - 1));
      controller.schedule("tab-a");
    }

    expect(saveLatest).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs));
    expect(saveLatest).toHaveBeenCalledTimes(1);
  });

  it("coalesces edits received during a pending save into one later save", async () => {
    vi.useFakeTimers();
    let dirty = true;
    const firstSave = deferred<"durable">();
    const saveLatest = vi.fn()
      .mockImplementationOnce(() => firstSave.promise)
      .mockImplementationOnce(async () => {
        dirty = false;
        return "durable" as const;
      });
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => dirty,
      onError: () => undefined,
      saveLatest
    });

    controller.schedule("tab-a");
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs));
    expect(saveLatest).toHaveBeenCalledTimes(1);

    controller.schedule("tab-a");
    controller.schedule("tab-a");
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs));
    expect(saveLatest).toHaveBeenCalledTimes(1);

    await act(async () => {
      firstSave.resolve("durable");
      await Promise.resolve();
    });
    expect(saveLatest).toHaveBeenCalledTimes(2);
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs * 2));
    expect(saveLatest).toHaveBeenCalledTimes(2);
  });

  it("enqueueNow cancels the idle timer and starts an attempt immediately", async () => {
    vi.useFakeTimers();
    const save = deferred<"durable">();
    const saveLatest = vi.fn(() => save.promise);
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => true,
      onError: () => undefined,
      saveLatest
    });

    controller.schedule("tab-a");
    const attempt = controller.enqueueNow("tab-a");
    expect(saveLatest).toHaveBeenCalledTimes(1);
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs));
    expect(saveLatest).toHaveBeenCalledTimes(1);

    save.resolve("durable");
    await expect(attempt).resolves.toBe("durable");
  });

  it("flush waits for an active write and saves the latest dirty state before resolving", async () => {
    vi.useFakeTimers();
    let dirty = true;
    const firstSave = deferred<"durable">();
    const saveLatest = vi.fn()
      .mockImplementationOnce(() => firstSave.promise)
      .mockImplementationOnce(async () => {
        dirty = false;
        return "durable" as const;
      });
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => dirty,
      onError: () => undefined,
      saveLatest
    });

    controller.schedule("tab-a");
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs));
    const flush = controller.flush("tab-a");
    expect(saveLatest).toHaveBeenCalledTimes(1);

    await act(async () => {
      firstSave.resolve("durable");
      await Promise.resolve();
    });
    expect(saveLatest).toHaveBeenCalledTimes(2);
    await expect(flush).resolves.toBe("durable");
  });

  it("flush waits for a clean tab's in-flight durable write", async () => {
    const save = deferred<"durable">();
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => false,
      onError: () => undefined,
      saveLatest: () => save.promise
    });

    controller.enqueueNow("tab-a");
    let flushSettled = false;
    const flush = controller.flush("tab-a").then((result) => {
      flushSettled = true;
      return result;
    });
    await Promise.resolve();
    expect(flushSettled).toBe(false);

    save.resolve("durable");
    await expect(flush).resolves.toBe("durable");
  });

  it("flush reports a clean tab's in-flight failure", async () => {
    const error = new Error("disk unavailable");
    const save = deferred<"durable">();
    const onError = vi.fn();
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => false,
      onError,
      saveLatest: () => save.promise
    });

    controller.enqueueNow("tab-a");
    const flush = controller.flush("tab-a");
    save.reject(error);

    await expect(flush).resolves.toBe("failed");
    expect(onError).toHaveBeenCalledWith("tab-a", error);
  });

  it("flush returns failed and ineligible outcomes without conflating them", async () => {
    const error = new Error("disk unavailable");
    const onError = vi.fn();
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => true,
      onError,
      saveLatest: async (tabId) => {
        if (tabId === "ineligible") return "ineligible";
        throw error;
      }
    });

    await expect(controller.flush("ineligible")).resolves.toBe("ineligible");
    await expect(controller.flush("failed")).resolves.toBe("failed");
    expect(onError).toHaveBeenCalledOnce();
    expect(onError).toHaveBeenCalledWith("failed", error);
  });

  it("reports one error episode until a later edit resets error reporting", async () => {
    vi.useFakeTimers();
    const error = new Error("write failed");
    const onError = vi.fn();
    const saveLatest = vi.fn(async () => Promise.reject(error));
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => true,
      onError,
      saveLatest
    });

    controller.schedule("tab-a");
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs));
    await controller.enqueueNow("tab-a");
    expect(onError).toHaveBeenCalledOnce();

    controller.schedule("tab-a");
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs));
    expect(onError).toHaveBeenCalledTimes(2);
  });

  it("retries repeated failed flushes without reporting the unchanged edit episode twice", async () => {
    const error = new Error("write failed");
    const onError = vi.fn();
    const saveLatest = vi.fn(async () => Promise.reject(error));
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => true,
      onError,
      saveLatest
    });

    await expect(controller.flush("tab-a")).resolves.toBe("failed");
    await expect(controller.flush("tab-a")).resolves.toBe("failed");

    expect(saveLatest).toHaveBeenCalledTimes(2);
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("cancel removes a pending timer while an already-started write settles", async () => {
    vi.useFakeTimers();
    const firstSave = deferred<"durable">();
    const saveLatest = vi.fn((tabId: string) => {
      if (tabId === "tab-a") return firstSave.promise;
      return Promise.resolve("durable" as const);
    });
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => true,
      onError: () => undefined,
      saveLatest
    });

    const inFlight = controller.enqueueNow("tab-a");
    controller.schedule("tab-a");
    controller.cancel("tab-a");
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs * 2));
    expect(saveLatest).toHaveBeenCalledTimes(1);

    firstSave.resolve("durable");
    await expect(inFlight).resolves.toBe("durable");
  });

  it("dispose removes pending timers while an already-started write settles", async () => {
    vi.useFakeTimers();
    const firstSave = deferred<"durable">();
    const saveLatest = vi.fn((tabId: string) => {
      if (tabId === "tab-a") return firstSave.promise;
      return Promise.resolve("durable" as const);
    });
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: () => true,
      onError: () => undefined,
      saveLatest
    });

    const inFlight = controller.enqueueNow("tab-a");
    controller.schedule("tab-b");
    controller.dispose();
    await act(() => vi.advanceTimersByTimeAsync(realtimeMarkdownAutoSaveIdleMs * 2));
    expect(saveLatest).toHaveBeenCalledTimes(1);

    firstSave.resolve("durable");
    await expect(inFlight).resolves.toBe("durable");
  });

  it("flushAll handles unique tabs sequentially and reports failures", async () => {
    const calls: string[] = [];
    const dirtyTabs = new Set(["tab-a", "tab-b", "tab-c"]);
    const controller = createRealtimeMarkdownAutoSaveController({
      isDirty: (tabId) => dirtyTabs.has(tabId),
      onError: () => undefined,
      saveLatest: async (tabId) => {
        calls.push(tabId);
        if (tabId === "tab-c") throw new Error("write failed");
        dirtyTabs.delete(tabId);
        return tabId === "tab-b" ? "ineligible" : "durable";
      }
    });

    await expect(controller.flushAll(["tab-a", "tab-a", "tab-b", "tab-c"])).resolves.toBe(false);
    expect(calls).toEqual(["tab-a", "tab-b", "tab-c"]);
  });
});
