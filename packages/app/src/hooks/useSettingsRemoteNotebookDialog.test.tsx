import { act, renderHook, waitFor } from "@testing-library/react";
import type { I18nKey } from "@markra/shared";
import { requestPrimaryCloudNotebookRestore } from "../lib/cloud-notebook-restore-events";
import type { SyncConfigDocument } from "../lib/sync-config";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppRuntime,
  type RemoteNotebookCatalogEntry
} from "../runtime";
import { useSettingsRemoteNotebookDialog } from "./useSettingsRemoteNotebookDialog";

vi.mock("../lib/cloud-notebook-restore-events", () => ({
  requestPrimaryCloudNotebookRestore: vi.fn()
}));

const mockedRequestPrimaryCloudNotebookRestore = vi.mocked(requestPrimaryCloudNotebookRestore);

function document(revision = "rev-2"): SyncConfigDocument {
  return {
    config: {
      enabled: true,
      intervalSeconds: 30,
      generateConflictDocument: false,
      mode: "startup-exit",
      provider: "webdav",
      remoteRoot: "qingyu",
      s3: {
        accessKeyId: "",
        bucket: "",
        endpointUrl: "",
        region: "",
        secretAccessKey: "",
        requestTimeoutSeconds: 60,
        addressingStyle: "auto",
        tlsVerification: "verify"
      },
      version: 3,
      webdav: { password: "", serverUrl: "https://dav.example.test", username: "" }
    },
    configured: true,
    issues: [],
    readiness: "ready",
    revision
  };
}

function entry(name: string): RemoteNotebookCatalogEntry {
  return {
    available: true,
    disabledReason: null,
    displayName: name,
    name,
    provider: "webdav",
    repositoryId: null
  };
}

function s3Entry(displayName: string): RemoteNotebookCatalogEntry {
  return {
    available: true,
    disabledReason: null,
    displayName,
    name: displayName,
    provider: "s3",
    repositoryId: "00000000-0000-4000-8000-000000000051"
  };
}

function deferred<T>() {
  let resolve!: (value: T) => undefined;
  let reject!: (reason?: unknown) => undefined;
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

function installRuntime(options: {
  dejavuSync?: boolean;
  listNotebooks?: AppRuntime["syncConfig"]["listNotebooks"];
  load?: AppRuntime["syncConfig"]["load"];
} = {}) {
  const runtime = createDefaultAppRuntime();
  const hideSettingsWindow = vi.fn(async () => undefined);
  const load = vi.fn(options.load ?? (async () => ({ ...document(), status: "loaded" as const })));
  const listNotebooks = vi.fn(options.listNotebooks ?? (async () => [entry("Archive")]));
  configureAppRuntime({
    ...runtime,
    features: { ...runtime.features, dejavuSync: options.dejavuSync ?? false },
    syncConfig: { ...runtime.syncConfig, listNotebooks, load },
    window: { ...runtime.window, hideSettingsWindow }
  });
  return { hideSettingsWindow, listNotebooks, load };
}

function setup(primaryRoot: string | null = "/Workspace/Current") {
  const onSessionFailure = vi.fn();
  const syncSession = {
    begin: vi.fn(async () => undefined),
    end: vi.fn(async () => undefined)
  };
  const translate = (key: I18nKey) => key === "notebooks.remote.refreshError"
    ? "Could not refresh cloud notebooks."
    : key;
  const rendered = renderHook(
    ({ root }) => useSettingsRemoteNotebookDialog({
      onSessionFailure,
      primaryRoot: root,
      syncSession,
      translate
    }),
    { initialProps: { root: primaryRoot } }
  );
  return { ...rendered, onSessionFailure, syncSession };
}

describe("useSettingsRemoteNotebookDialog", () => {
  beforeEach(() => {
    resetAppRuntimeForTests();
    mockedRequestPrimaryCloudNotebookRestore.mockReset();
  });

  afterEach(() => resetAppRuntimeForTests());

  it("ends editing, loads the authoritative catalog, and never hides Settings", async () => {
    const runtime = installRuntime();
    const { result, syncSession } = setup();

    await act(async () => result.current.openDialog());

    expect(syncSession.end).toHaveBeenCalledWith("catalog-handoff");
    expect(runtime.load).toHaveBeenCalledTimes(1);
    expect(runtime.listNotebooks).toHaveBeenCalledWith({ revision: "rev-2" });
    expect(result.current.open).toBe(true);
    expect(result.current.entries).toEqual([entry("Archive")]);
    expect(runtime.hideSettingsWindow).not.toHaveBeenCalled();
  });

  it("serializes concurrent open requests and exposes loading before the catalog resolves", async () => {
    const catalog = deferred<RemoteNotebookCatalogEntry[]>();
    const runtime = installRuntime({ listNotebooks: async () => catalog.promise });
    const { result, syncSession } = setup();

    let first!: Promise<unknown>;
    let second!: Promise<unknown>;
    act(() => {
      first = result.current.openDialog();
      second = result.current.openDialog();
    });
    await waitFor(() => expect(result.current.loading).toBe(true));

    expect(first).toBe(second);
    expect(syncSession.end).toHaveBeenCalledTimes(1);
    expect(runtime.listNotebooks).toHaveBeenCalledTimes(1);

    await act(async () => catalog.resolve([entry("Archive")]));
    await expect(first).resolves.toBeUndefined();
    expect(result.current.loading).toBe(false);
  });

  it("starts a fresh ended-session transaction when cancel is followed by an immediate reopen", async () => {
    const firstCatalog = deferred<RemoteNotebookCatalogEntry[]>();
    const secondCatalog = deferred<RemoteNotebookCatalogEntry[]>();
    const sessionRestart = deferred<undefined>();
    const runtime = installRuntime({
      listNotebooks: vi.fn()
        .mockImplementationOnce(async () => firstCatalog.promise)
        .mockImplementationOnce(async () => secondCatalog.promise)
    });
    const { result, syncSession } = setup();
    let sessionActive = true;
    syncSession.end.mockImplementation(async () => {
      sessionActive = false;
      return undefined;
    });
    syncSession.begin.mockImplementation(async () => {
      await sessionRestart.promise;
      sessionActive = true;
    });

    let firstOpen!: Promise<unknown>;
    act(() => {
      firstOpen = result.current.openDialog();
    });
    await waitFor(() => expect(runtime.listNotebooks).toHaveBeenCalledTimes(1));

    let secondOpen!: Promise<unknown>;
    act(() => {
      result.current.cancel();
      secondOpen = result.current.openDialog();
    });

    expect(secondOpen).not.toBe(firstOpen);
    expect(syncSession.end).toHaveBeenCalledTimes(1);

    act(() => sessionRestart.resolve(undefined));
    await waitFor(() => expect(runtime.listNotebooks).toHaveBeenCalledTimes(2));
    expect(sessionActive).toBe(false);

    await act(async () => {
      secondCatalog.resolve([entry("Fresh")]);
      await secondOpen;
    });
    expect(result.current.open).toBe(true);
    expect(result.current.entries).toEqual([entry("Fresh")]);

    await act(async () => {
      firstCatalog.resolve([entry("Stale")]);
      await firstOpen;
    });
    expect(result.current.entries).toEqual([entry("Fresh")]);
  });

  it("cancels only the dialog and resumes the Sync editing session", async () => {
    installRuntime();
    const { result, syncSession } = setup();
    await act(async () => result.current.openDialog());

    await act(async () => {
      await result.current.cancel();
    });

    await waitFor(() => expect(syncSession.begin).toHaveBeenCalledTimes(1));
    expect(result.current.open).toBe(false);
    expect(result.current.entries).toEqual([]);
  });

  it("replaces a catalog failure with entries from a successful refresh", async () => {
    const runtime = installRuntime({
      listNotebooks: vi.fn()
        .mockRejectedValueOnce(new Error("secret backend detail"))
        .mockResolvedValueOnce([entry("Archive")])
    });
    const { result } = setup();

    await act(async () => result.current.openDialog());
    expect(result.current.error).toBe("Could not refresh cloud notebooks.");
    expect(result.current.error).not.toContain("secret backend detail");

    await act(async () => result.current.refresh());
    expect(runtime.load).toHaveBeenCalledTimes(2);
    expect(result.current.entries).toEqual([entry("Archive")]);
    expect(result.current.error).toBeNull();
  });

  it("ignores a late catalog result after cancel", async () => {
    const catalog = deferred<RemoteNotebookCatalogEntry[]>();
    installRuntime({ listNotebooks: async () => catalog.promise });
    const { result } = setup();

    act(() => {
      result.current.openDialog().catch(() => {});
    });
    await waitFor(() => expect(result.current.loading).toBe(true));
    let cancel!: Promise<unknown>;
    act(() => {
      cancel = Promise.resolve(result.current.cancel());
    });
    await act(async () => {
      catalog.resolve([entry("Too Late")]);
      await cancel;
    });

    expect(result.current.open).toBe(false);
    expect(result.current.entries).toEqual([]);
  });

  it("derives the current notebook name from the primary root", () => {
    installRuntime();
    const { result, rerender } = setup("C:\\Workspace\\Daily Notes");

    expect(result.current.currentNotebookName).toBe("Daily Notes");
    rerender({ root: null });
    expect(result.current.currentNotebookName).toBeNull();
  });

  it("keeps a failed restore open and retryable", async () => {
    installRuntime();
    const { result } = setup();
    await act(async () => result.current.openDialog());
    mockedRequestPrimaryCloudNotebookRestore.mockResolvedValueOnce(false);

    await expect(result.current.restore(entry("Archive"))).rejects.toThrow();

    expect(result.current.open).toBe(true);
    expect(mockedRequestPrimaryCloudNotebookRestore).toHaveBeenCalledWith(expect.objectContaining({
      remoteName: "Archive",
      provider: "webdav",
      revision: "rev-2",
      signal: expect.any(AbortSignal)
    }));
  });

  it("closes only the dialog and resumes Sync after a successful same-name restore", async () => {
    installRuntime();
    const { result, syncSession } = setup("/Workspace/Archive");
    await act(async () => result.current.openDialog());
    mockedRequestPrimaryCloudNotebookRestore.mockResolvedValueOnce(true);

    await act(async () => result.current.restore(entry("Archive")));

    expect(result.current.open).toBe(false);
    expect(syncSession.begin).toHaveBeenCalledTimes(1);
  });

  it("waits for a different restored primary root before resuming Sync", async () => {
    installRuntime();
    const { result, rerender, syncSession } = setup("/Workspace/Current");
    await act(async () => result.current.openDialog());
    mockedRequestPrimaryCloudNotebookRestore.mockResolvedValueOnce(true);

    await act(async () => result.current.restore(entry("Archive")));
    expect(result.current.open).toBe(false);
    expect(syncSession.begin).not.toHaveBeenCalled();

    rerender({ root: "/Restored/Archive" });
    await waitFor(() => expect(syncSession.begin).toHaveBeenCalledTimes(1));

    rerender({ root: "/Workspace/Current" });
    rerender({ root: "/Restored/Archive" });
    expect(syncSession.begin).toHaveBeenCalledTimes(1);
  });

  it("does not resume an old restore waiter after a new dialog transaction opens", async () => {
    installRuntime();
    const { result, rerender, syncSession } = setup("/Workspace/Current");
    await act(async () => result.current.openDialog());
    mockedRequestPrimaryCloudNotebookRestore.mockResolvedValueOnce(true);
    await act(async () => result.current.restore(entry("Archive")));

    await act(async () => result.current.openDialog());
    rerender({ root: "/Restored/Archive" });

    expect(result.current.open).toBe(true);
    expect(syncSession.begin).not.toHaveBeenCalled();
  });

  it("does not resume an old restore waiter after cancel starts a newer session", async () => {
    installRuntime();
    const { result, rerender, syncSession } = setup("/Workspace/Current");
    await act(async () => result.current.openDialog());
    mockedRequestPrimaryCloudNotebookRestore.mockResolvedValueOnce(true);
    await act(async () => result.current.restore(entry("Archive")));

    await act(async () => {
      await result.current.cancel();
    });
    await waitFor(() => expect(syncSession.begin).toHaveBeenCalledTimes(1));
    rerender({ root: "/Restored/Archive" });

    expect(syncSession.begin).toHaveBeenCalledTimes(1);
  });

  it("does not resume an old restore waiter after a same-name restore operation", async () => {
    installRuntime();
    const { result, rerender, syncSession } = setup("/Workspace/Current");
    await act(async () => result.current.openDialog());
    mockedRequestPrimaryCloudNotebookRestore
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(true);
    await act(async () => result.current.restore(entry("Archive")));

    await act(async () => result.current.restore(entry("Current")));
    expect(syncSession.begin).toHaveBeenCalledTimes(1);
    rerender({ root: "/Restored/Archive" });

    expect(syncSession.begin).toHaveBeenCalledTimes(1);
  });

  it("reports session end and restart failures without hiding Settings", async () => {
    const runtime = installRuntime();
    const first = setup();
    first.syncSession.end.mockRejectedValueOnce(new Error("write failed"));

    await act(async () => first.result.current.openDialog());

    expect(first.onSessionFailure).toHaveBeenCalledTimes(1);
    expect(first.result.current.open).toBe(false);
    expect(runtime.hideSettingsWindow).not.toHaveBeenCalled();

    first.syncSession.end.mockResolvedValueOnce(undefined);
    first.syncSession.begin.mockRejectedValueOnce(new Error("reload failed"));
    await act(async () => first.result.current.openDialog());
    await act(async () => {
      await first.result.current.cancel();
    });
    await waitFor(() => expect(first.onSessionFailure).toHaveBeenCalledTimes(2));
  });

  it("aborts an active restore request when unmounted", async () => {
    installRuntime();
    let restoreSignal: AbortSignal | undefined;
    mockedRequestPrimaryCloudNotebookRestore.mockImplementationOnce(async (input) => {
      restoreSignal = input.signal;
      return new Promise<boolean>(() => {});
    });
    const { result, unmount } = setup();
    await act(async () => result.current.openDialog());

    act(() => {
      result.current.restore(entry("Archive")).catch(() => {});
    });
    await waitFor(() => expect(restoreSignal).toBeDefined());
    unmount();

    expect(restoreSignal?.aborted).toBe(true);
  });

  it("accepts an S3 repository binding for the selected local root and resumes immediately", async () => {
    installRuntime({ dejavuSync: true, listNotebooks: async () => [s3Entry("Archive")] });
    const { result, syncSession } = setup("/Workspace/Current");
    await act(async () => result.current.openDialog());
    mockedRequestPrimaryCloudNotebookRestore.mockResolvedValueOnce(true);

    await act(async () => result.current.restore(s3Entry("Archive")));

    expect(mockedRequestPrimaryCloudNotebookRestore).toHaveBeenCalledWith(expect.objectContaining({
      displayName: "Archive",
      notesRoot: "/Workspace/Current",
      provider: "s3",
      repositoryId: "00000000-0000-4000-8000-000000000051",
      revision: "rev-2",
      signal: expect.any(AbortSignal)
    }));
    expect(result.current.open).toBe(false);
    expect(syncSession.begin).toHaveBeenCalledTimes(1);
  });

  it("omits and rejects S3 repository entries when Dejavu sync is unavailable", async () => {
    installRuntime({
      listNotebooks: async () => [entry("WebDAV Notes"), s3Entry("S3 Archive")]
    });
    const { result } = setup("/Workspace/Current");

    await act(async () => result.current.openDialog());

    expect(result.current.entries).toEqual([entry("WebDAV Notes")]);
    await expect(result.current.restore(s3Entry("S3 Archive"))).rejects.toThrow(
      "Cloud notebook restore failed"
    );
    expect(mockedRequestPrimaryCloudNotebookRestore).not.toHaveBeenCalled();
  });
});
