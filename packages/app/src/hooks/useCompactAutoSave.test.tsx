import { act, renderHook } from "@testing-library/react";
import { vi } from "vitest";
import { useCompactAutoSave } from "./useCompactAutoSave";

type HookProps = {
  content: string;
  dirty: boolean;
  documentKey: string | null;
  enabled: boolean;
};

function deferred<T>() {
  let resolve!: (value: T) => undefined;
  let reject!: (reason: unknown) => undefined;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = (value) => {
      resolvePromise(value);
      return undefined;
    };
    reject = (reason) => {
      rejectPromise(reason);
      return undefined;
    };
  });
  return { promise, reject, resolve };
}

function renderCompactAutoSave(
  saveDirtyMarkdownFiles: () => Promise<unknown>,
  initialProps: Partial<HookProps> = {}
) {
  const props: HookProps = {
    content: "# Draft\n\nEdit",
    dirty: true,
    documentKey: "file:/notes/draft.md",
    enabled: true,
    ...initialProps
  };

  return renderHook(
    (currentProps: HookProps) => useCompactAutoSave({
      ...currentProps,
      errorMessage: "The note could not be saved.",
      saveDirtyMarkdownFiles
    }),
    { initialProps: props }
  );
}

describe("useCompactAutoSave", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("waits 1500 ms after a dirty edit before saving locally", async () => {
    const saveDirtyMarkdownFiles = vi.fn().mockResolvedValue([]);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    expect(result.current.status).toBe("dirty");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1499);
    });
    expect(saveDirtyMarkdownFiles).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);
    expect(result.current).toMatchObject({ error: null, status: "saved" });
  });

  it("restarts the 1500 ms timer after a subsequent edit", async () => {
    const saveDirtyMarkdownFiles = vi.fn().mockResolvedValue([]);
    const { rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    rerender({
      content: "# Draft\n\nLater edit",
      dirty: true,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1499);
    });
    expect(saveDirtyMarkdownFiles).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);
  });

  it("never schedules the Compact timer while disabled", async () => {
    const saveDirtyMarkdownFiles = vi.fn().mockResolvedValue([]);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles, { enabled: false });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });

    expect(saveDirtyMarkdownFiles).not.toHaveBeenCalled();
    expect(result.current.status).toBe("saved");
  });

  it("shares one in-flight local write across concurrent flush callers", async () => {
    const pendingSave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn(() => pendingSave.promise);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let navigationFlush!: Promise<unknown>;
    let retryFlush!: Promise<unknown>;
    act(() => {
      navigationFlush = result.current.flush("navigation");
      retryFlush = result.current.retry();
    });

    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);
    expect(result.current.status).toBe("saving");
    await act(async () => {
      pendingSave.resolve([]);
      await Promise.all([navigationFlush, retryFlush]);
    });
    expect(result.current.status).toBe("saved");
  });

  it("keeps a newer edit dirty when an older local write completes", async () => {
    const pendingSave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn(() => pendingSave.promise);
    const { result, rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let firstFlush!: Promise<unknown>;
    act(() => {
      firstFlush = result.current.flush("navigation");
    });
    rerender({
      content: "# Draft\n\nTyped while saving",
      dirty: true,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });
    expect(result.current.status).toBe("dirty");

    await act(async () => {
      pendingSave.resolve([]);
      await firstFlush;
    });

    expect(result.current.status).toBe("dirty");
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);
  });

  it("flushes immediately for navigation and cancels the pending timer", async () => {
    const saveDirtyMarkdownFiles = vi.fn().mockResolvedValue([]);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
      await result.current.flush("navigation");
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);
  });

  it("attempts a navigation flush before a deferred editor change reaches React dirty state", async () => {
    const saveDirtyMarkdownFiles = vi.fn().mockResolvedValue([]);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles, { dirty: false });

    await act(async () => {
      await result.current.flush("navigation");
    });

    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);
  });

  it("queues one forced snapshot behind a timer write when navigation sees a deferred editor edit", async () => {
    const timerSave = deferred<unknown[]>();
    const navigationSave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn()
      .mockImplementationOnce(() => timerSave.promise)
      .mockImplementationOnce(() => navigationSave.promise);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let timerFlush!: Promise<unknown>;
    let navigationFlush!: Promise<unknown>;
    let navigationResolved = false;
    act(() => {
      timerFlush = result.current.flush("timer");
      navigationFlush = result.current.flush("navigation").then((value) => {
        navigationResolved = true;
        return value;
      });
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);

    await act(async () => {
      timerSave.resolve([]);
      await Promise.resolve();
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(2);
    expect(navigationResolved).toBe(false);

    await act(async () => {
      navigationSave.resolve([]);
      await Promise.all([timerFlush, navigationFlush]);
    });
    expect(navigationResolved).toBe(true);
  });

  it.each(["navigation", "pagehide"] as const)(
    "runs the queued %s snapshot after the timer write fails",
    async (reason) => {
      const timerSave = deferred<unknown[]>();
      const forcedSave = deferred<unknown[]>();
      const saveDirtyMarkdownFiles = vi.fn()
        .mockImplementationOnce(() => timerSave.promise)
        .mockImplementationOnce(() => forcedSave.promise);
      const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles);

      let sharedFlush!: Promise<unknown>;
      let flushOutcome: "pending" | "rejected" | "resolved" = "pending";
      act(() => {
        const timerFlush = result.current.flush("timer");
        sharedFlush = result.current.flush(reason);
        expect(sharedFlush).toBe(timerFlush);
        sharedFlush.then(
          () => {
            flushOutcome = "resolved";
          },
          () => {
            flushOutcome = "rejected";
          }
        );
      });

      await act(async () => {
        timerSave.reject(new Error("ENOSPC timer write"));
        await Promise.resolve();
      });
      expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(2);
      expect(flushOutcome).toBe("pending");

      await act(async () => {
        forcedSave.resolve([]);
        await sharedFlush;
      });
      expect(flushOutcome).toBe("resolved");
      expect(result.current).toMatchObject({ error: null, status: "saved" });
    }
  );

  it("keeps only the forced snapshot failure and does not retry it in a loop", async () => {
    const timerSave = deferred<unknown[]>();
    const forcedSave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn()
      .mockImplementationOnce(() => timerSave.promise)
      .mockImplementationOnce(() => forcedSave.promise);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let sharedFlush!: Promise<unknown>;
    act(() => {
      result.current.flush("timer");
      sharedFlush = result.current.flush("navigation");
    });
    await act(async () => {
      timerSave.reject(new Error("ENOSPC timer write"));
      await Promise.resolve();
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(2);

    await act(async () => {
      forcedSave.reject(new Error("EACCES forced snapshot"));
      await expect(sharedFlush).rejects.toThrow("forced snapshot");
    });
    expect(result.current).toMatchObject({
      error: "Permission denied while saving this note.",
      status: "error"
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(2);

    saveDirtyMarkdownFiles.mockResolvedValueOnce([]);
    await act(async () => {
      await result.current.retry();
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(3);
    expect(result.current).toMatchObject({ error: null, status: "saved" });
  });

  it("coalesces concurrent navigation callers into one forced snapshot", async () => {
    const navigationSave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn(() => navigationSave.promise);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles, { dirty: false });

    let firstNavigation!: Promise<unknown>;
    let secondNavigation!: Promise<unknown>;
    act(() => {
      firstNavigation = result.current.flush("navigation");
      secondNavigation = result.current.flush("navigation");
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);

    await act(async () => {
      navigationSave.resolve([]);
      await Promise.all([firstNavigation, secondNavigation]);
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);
  });

  it("flushes on hidden visibility and pagehide, then cleans up both listeners", async () => {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    const removeDocumentListener = vi.spyOn(document, "removeEventListener");
    const removeWindowListener = vi.spyOn(window, "removeEventListener");
    const saveDirtyMarkdownFiles = vi.fn().mockResolvedValue([]);
    const { rerender, unmount } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);

    rerender({
      content: "# Draft\n\nEdit after resume",
      dirty: true,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });
    await act(async () => {
      window.dispatchEvent(new Event("pagehide"));
      await Promise.resolve();
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(2);

    unmount();
    expect(removeDocumentListener).toHaveBeenCalledWith("visibilitychange", expect.any(Function));
    expect(removeWindowListener).toHaveBeenCalledWith("pagehide", expect.any(Function));
  });

  it("keeps a safe failure visible until serialized Retry succeeds", async () => {
    const failedSave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn(() => failedSave.promise);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let firstFlush!: Promise<unknown>;
    let concurrentRetry!: Promise<unknown>;
    act(() => {
      firstFlush = result.current.flush("navigation");
      concurrentRetry = result.current.retry();
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);

    await act(async () => {
      failedSave.reject(new Error("https://user:secret@example.test/private-config"));
      await expect(Promise.all([firstFlush, concurrentRetry])).rejects.toThrow();
    });
    expect(result.current).toMatchObject({
      error: "The note could not be saved.",
      status: "error"
    });
    expect(result.current.error).not.toContain("secret");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(result.current.status).toBe("error");
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);

    saveDirtyMarkdownFiles.mockResolvedValueOnce([]);
    await act(async () => {
      await result.current.retry();
    });
    expect(result.current).toMatchObject({ error: null, status: "saved" });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(2);
  });

  it("queues a newer revision behind an in-flight write and makes navigation await both", async () => {
    const firstSave = deferred<unknown[]>();
    const secondSave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn()
      .mockImplementationOnce(() => firstSave.promise)
      .mockImplementationOnce(() => secondSave.promise);
    const { result, rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let firstFlush!: Promise<unknown>;
    act(() => {
      firstFlush = result.current.flush("timer");
    });
    rerender({
      content: "# Draft\n\nNew revision B",
      dirty: true,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });
    let navigationResolved = false;
    let navigationFlush!: Promise<unknown>;
    act(() => {
      navigationFlush = result.current.flush("navigation").then((value) => {
        navigationResolved = true;
        return value;
      });
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);

    await act(async () => {
      firstSave.resolve([]);
      await Promise.resolve();
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(2);
    expect(navigationResolved).toBe(false);

    await act(async () => {
      secondSave.resolve([]);
      await Promise.all([firstFlush, navigationFlush]);
    });
    expect(navigationResolved).toBe(true);
    expect(result.current.status).toBe("saved");
  });

  it("clears the timer and prior error when explicit Save makes the document clean", async () => {
    const saveDirtyMarkdownFiles = vi.fn().mockRejectedValue(new Error("write failed"));
    const { result, rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    await act(async () => {
      await expect(result.current.flush("navigation")).rejects.toThrow("write failed");
    });
    expect(result.current.status).toBe("error");

    rerender({
      content: "# Draft\n\nEdit",
      dirty: false,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });
    expect(result.current).toMatchObject({ error: null, status: "saved" });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);
  });

  it("keeps the persistent error visible when more text is entered after failure", async () => {
    const saveDirtyMarkdownFiles = vi.fn().mockRejectedValue(new Error("write failed"));
    const { result, rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    await act(async () => {
      await expect(result.current.flush("navigation")).rejects.toThrow("write failed");
    });
    rerender({
      content: "# Draft\n\nNew text after failure",
      dirty: true,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });

    expect(result.current).toMatchObject({
      error: "The note could not be saved.",
      status: "error"
    });
  });

  it.each([
    [new Error("ENOSPC while writing /private?token=secret"), "Not enough storage space to save this note."],
    [new Error("EACCES permission denied for https://user:secret@example.test"), "Permission denied while saving this note."],
    [new Error("EROFS: read-only file system /private/config"), "This note is in a read-only location."],
    [{ credentials: "user:secret", endpoint: "https://example.test" }, "The note could not be saved."]
  ])("maps native write failures to a fixed safe reason without leaking raw detail", async (writeError, expectedReason) => {
    const saveDirtyMarkdownFiles = vi.fn().mockRejectedValue(writeError);
    const { result } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    await act(async () => {
      await expect(result.current.flush("navigation")).rejects.toBe(writeError);
    });

    expect(result.current.error).toBe(expectedReason);
    expect(result.current.error).not.toContain("secret");
    expect(result.current.error).not.toContain("https://");
  });

  it("does not let an older autosave completion overwrite a newer explicit Save", async () => {
    const autosave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn(() => autosave.promise);
    const { result, rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let autosaveFlush!: Promise<unknown>;
    act(() => {
      autosaveFlush = result.current.flush("timer");
    });
    rerender({
      content: "# Draft\n\nRevision B",
      dirty: true,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });
    rerender({
      content: "# Draft\n\nRevision B",
      dirty: false,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });
    expect(result.current.status).toBe("saved");

    await act(async () => {
      autosave.resolve([]);
      await autosaveFlush;
    });

    expect(result.current).toMatchObject({ error: null, status: "saved" });
    expect(saveDirtyMarkdownFiles).toHaveBeenCalledTimes(1);
  });

  it("does not let an old document autosave overwrite a newly selected clean document", async () => {
    const autosave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn(() => autosave.promise);
    const { result, rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let autosaveFlush!: Promise<unknown>;
    act(() => {
      autosaveFlush = result.current.flush("timer");
    });
    rerender({
      content: "# Another note",
      dirty: false,
      documentKey: "file:/notes/another.md",
      enabled: true
    });
    expect(result.current.status).toBe("saved");

    await act(async () => {
      autosave.resolve([]);
      await autosaveFlush;
    });

    expect(result.current).toMatchObject({ error: null, status: "saved" });
  });

  it("does not show a stale autosave failure after a newer explicit Save", async () => {
    const autosave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn(() => autosave.promise);
    const { result, rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let autosaveFlush!: Promise<unknown>;
    act(() => {
      autosaveFlush = result.current.flush("timer");
    });
    rerender({
      content: "# Draft\n\nRevision B",
      dirty: true,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });
    rerender({
      content: "# Draft\n\nRevision B",
      dirty: false,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });

    await act(async () => {
      autosave.reject(new Error("ENOSPC old autosave"));
      await expect(autosaveFlush).rejects.toThrow("old autosave");
    });

    expect(result.current).toMatchObject({ error: null, status: "saved" });
  });

  it("does not show an in-flight autosave failure after explicit Save cleans the same revision", async () => {
    const autosave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn(() => autosave.promise);
    const { result, rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let autosaveFlush!: Promise<unknown>;
    act(() => {
      autosaveFlush = result.current.flush("timer");
    });
    rerender({
      content: "# Draft",
      dirty: false,
      documentKey: "file:/notes/draft.md",
      enabled: true
    });

    await act(async () => {
      autosave.reject(new Error("ENOSPC old autosave"));
      await expect(autosaveFlush).rejects.toThrow("old autosave");
    });

    expect(result.current).toMatchObject({ error: null, status: "saved" });
  });

  it("does not show an old document failure over a newly selected clean document", async () => {
    const autosave = deferred<unknown[]>();
    const saveDirtyMarkdownFiles = vi.fn(() => autosave.promise);
    const { result, rerender } = renderCompactAutoSave(saveDirtyMarkdownFiles);

    let autosaveFlush!: Promise<unknown>;
    act(() => {
      autosaveFlush = result.current.flush("timer");
    });
    rerender({
      content: "# Another note",
      dirty: false,
      documentKey: "file:/notes/another.md",
      enabled: true
    });

    await act(async () => {
      autosave.reject(new Error("EACCES old document"));
      await expect(autosaveFlush).rejects.toThrow("old document");
    });

    expect(result.current).toMatchObject({ error: null, status: "saved" });
  });
});
