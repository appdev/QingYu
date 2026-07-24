import { act, renderHook, waitFor } from "@testing-library/react";
import type { SyncConfigDocument, SyncRunResult } from "../lib/sync-config";
import {
  getStoredRecentNotebooks
} from "../lib/settings/local-state";
import * as localStateModule from "../lib/settings/local-state";
import type { NotebookSwitchRequest } from "../lib/notebook-switch-events";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests
} from "../runtime";
import type { AppSyncCoordinator } from "./useAppSyncCoordinator";
import type { PrimaryWorkspaceController } from "./usePrimaryWorkspace";
import { useNotebookSwitchCoordinator } from "./useNotebookSwitchCoordinator";

function deferred<T>() {
  let reject!: (error: unknown) => unknown;
  let resolve!: (value: T) => unknown;
  const promise = new Promise<T>((complete, fail) => {
    resolve = complete;
    reject = fail;
  });
  return { promise, reject, resolve };
}

function configuredDocument(enabled = true): SyncConfigDocument {
  return {
    config: {
      autoSyncOnSave: true,
      enabled,
      intervalMinutes: 0,
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
      version: 2,
      webdav: {
        password: "secret",
        serverUrl: "https://dav.example.test",
        username: "writer"
      }
    },
    configured: true,
    issues: [],
    readiness: enabled ? "ready" : "disabled",
    revision: "rev-1"
  };
}

function syncResult(notesRoot: string): SyncRunResult {
  return {
    notebookName: notesRoot.split("/").at(-1) ?? "",
    notesRoot,
    provider: "webdav",
    revision: "rev-1",
    summary: {
      bytesDownloaded: 0,
      bytesUploaded: 0,
      conflictFiles: 0,
      downloadedFiles: 0,
      scannedFiles: 1,
      skippedFiles: 0,
      uploadedFiles: 0
    },
    trigger: "manual"
  };
}

function createAppSync(overrides: Partial<AppSyncCoordinator> = {}): AppSyncCoordinator {
  return {
    beginNotebookSwitch: vi.fn(async () => undefined),
    finishNotebookSwitch: vi.fn(() => undefined),
    notifyDocumentSaved: vi.fn(async () => undefined),
    run: vi.fn(async () => null),
    running: false,
    status: null,
    ...overrides
  };
}

function createPrimaryWorkspace(
  overrides: Partial<PrimaryWorkspaceController> = {}
): PrimaryWorkspaceController {
  return {
    canChooseDesktopRoot: true,
    commitDesktopRoot: vi.fn(async (path) => path),
    commitManagedRoot: vi.fn(async (name) => `/app-data/workspaces/${name}`),
    deferDesktopSetup: vi.fn(async () => undefined),
    error: null,
    managedName: null,
    resetOnboarding: vi.fn(async () => undefined),
    retry: vi.fn(async () => undefined),
    root: "/Old",
    status: "ready",
    workspaceRoot: "/",
    ...overrides
  };
}

function renderCoordinator({
  appSync = createAppSync(),
  configDocument = configuredDocument(),
  flushActiveDocument = vi.fn(async () => undefined),
  primaryRoot = "/Old",
  primaryWindowOwner = true,
  primaryWorkspace = createPrimaryWorkspace(),
  trueMobile = false
}: {
  appSync?: AppSyncCoordinator;
  configDocument?: SyncConfigDocument | null;
  flushActiveDocument?: () => Promise<unknown>;
  primaryRoot?: string | null;
  primaryWindowOwner?: boolean;
  primaryWorkspace?: PrimaryWorkspaceController;
  trueMobile?: boolean;
} = {}) {
  return renderHook(
    ({ root }) => useNotebookSwitchCoordinator({
      appSync,
      configDocument,
      flushActiveDocument,
      primaryRoot: root,
      primaryWindowOwner,
      primaryWorkspace,
      trueMobile
    }),
    { initialProps: { root: primaryRoot } }
  );
}

describe("notebook switch coordinator", () => {
  beforeEach(() => {
    const runtime = createDefaultAppRuntime();
    configureAppRuntime({
      ...runtime,
      files: {
        ...runtime.files,
        openMarkdownFolder: vi.fn(async () => null)
      },
      syncConfig: {
        ...runtime.syncConfig,
        sync: vi.fn(async (request) => syncResult(
          "notesRoot" in request ? request.notesRoot : "/Prepared"
        ))
      },
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget: vi.fn(async ({ notebookName, parentPath }) => ({
          lease: "prepared-capability",
          notesRoot: `${parentPath}/${notebookName}`
        }))
      }
    });
  });

  afterEach(() => {
    resetAppRuntimeForTests();
    vi.restoreAllMocks();
  });

  it("orders flush, barrier drain, persistence, mount, release, and visible sync", async () => {
    const order: string[] = [];
    const drain = deferred<undefined>();
    const release = deferred<undefined>();
    const appSync = createAppSync({
      beginNotebookSwitch: vi.fn(async () => {
        order.push("block");
        await drain.promise;
        order.push("drain");
      }),
      finishNotebookSwitch: vi.fn(async () => {
        order.push("release");
        await release.promise;
        order.push("settled");
      }),
      run: vi.fn(async () => {
        order.push("sync");
        return null;
      })
    });
    const primaryWorkspace = createPrimaryWorkspace({
      commitDesktopRoot: vi.fn(async () => {
        order.push("persist");
        return "/Canonical/New";
      })
    });
    const flushActiveDocument = vi.fn(async () => {
      order.push("flush");
    });
    const { result, rerender } = renderCoordinator({ appSync, flushActiveDocument, primaryWorkspace });

    let switching!: Promise<string | null>;
    act(() => {
      switching = result.current.switchDesktopNotebook("/Selected/New");
    });
    await waitFor(() => expect(order).toEqual(["flush", "block"]));
    drain.resolve(undefined);
    await waitFor(() => expect(order).toEqual(["flush", "block", "drain", "persist"]));
    expect(appSync.run).not.toHaveBeenCalled();

    rerender({ root: "/Canonical/New" });
    await waitFor(() => expect(order).toEqual([
      "flush", "block", "drain", "persist", "release"
    ]));
    expect(appSync.run).not.toHaveBeenCalled();
    release.resolve(undefined);
    await expect(switching).resolves.toBe("/Canonical/New");
    expect(order).toEqual([
      "flush", "block", "drain", "persist", "release", "settled", "sync"
    ]);
    expect(appSync.run).toHaveBeenCalledWith("app-launch", "rev-1");
  });

  it("leaves the old root active when the desktop picker is cancelled", async () => {
    const appSync = createAppSync();
    const flushActiveDocument = vi.fn(async () => undefined);
    const primaryWorkspace = createPrimaryWorkspace();
    const { result } = renderCoordinator({ appSync, flushActiveDocument, primaryWorkspace });

    await expect(result.current.switchDesktopNotebook()).resolves.toBeNull();

    expect(flushActiveDocument).not.toHaveBeenCalled();
    expect(appSync.beginNotebookSwitch).not.toHaveBeenCalled();
    expect(primaryWorkspace.commitDesktopRoot).not.toHaveBeenCalled();
  });

  it.each([
    ["flush", new Error("save-failed")],
    ["drain", new Error("drain-failed")]
  ])("aborts before persistence when %s fails", async (stage, failure) => {
    const appSync = createAppSync({
      beginNotebookSwitch: vi.fn(async () => {
        if (stage === "drain") throw failure;
      })
    });
    const flushActiveDocument = vi.fn(async () => {
      if (stage === "flush") throw failure;
    });
    const primaryWorkspace = createPrimaryWorkspace();
    const { result } = renderCoordinator({ appSync, flushActiveDocument, primaryWorkspace });

    await expect(result.current.switchDesktopNotebook("/New")).resolves.toBeNull();

    expect(primaryWorkspace.commitDesktopRoot).not.toHaveBeenCalled();
    expect(appSync.finishNotebookSwitch).toHaveBeenCalledTimes(stage === "drain" ? 1 : 0);
  });

  it("keeps a persisted root when its post-mount network run fails", async () => {
    const appSync = createAppSync({
      run: vi.fn(async () => {
        throw new Error("network-failed");
      })
    });
    const primaryWorkspace = createPrimaryWorkspace({
      commitDesktopRoot: vi.fn(async () => "/New")
    });
    const { result, rerender } = renderCoordinator({ appSync, primaryWorkspace });

    const switching = result.current.switchDesktopNotebook("/New");
    await waitFor(() => expect(primaryWorkspace.commitDesktopRoot).toHaveBeenCalled());
    rerender({ root: "/New" });

    await expect(switching).resolves.toBe("/New");
    expect(primaryWorkspace.commitDesktopRoot).toHaveBeenCalledTimes(1);
  });

  it("preserves a prepared restore target and the old root when bootstrap fails", async () => {
    const runtime = createDefaultAppRuntime();
    const prepareDesktopNotebookTarget = vi.fn(async () => ({
      lease: "prepared-b",
      notesRoot: "/Workspace/B"
    }));
    const openMarkdownFolder = vi.fn(async () => ({ name: "Unexpected", path: "/Unexpected" }));
    const sync = vi.fn(async () => {
      throw new Error("provider-failed");
    });
    configureAppRuntime({
      ...runtime,
      files: {
        ...runtime.files,
        openMarkdownFolder
      },
      syncConfig: { ...runtime.syncConfig, sync },
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget
      }
    });
    const primaryWorkspace = createPrimaryWorkspace({
      root: "/Workspace/A",
      workspaceRoot: "/Workspace"
    });
    const { result } = renderCoordinator({
      primaryRoot: "/Workspace/A",
      primaryWorkspace
    });

    await expect(result.current.restoreDesktopNotebook("B", "/Workspace")).resolves.toBeNull();

    expect(prepareDesktopNotebookTarget).toHaveBeenCalledWith({
      notebookName: "B",
      parentPath: "/Workspace"
    });
    expect(openMarkdownFolder).not.toHaveBeenCalled();
    expect(sync).toHaveBeenCalledWith({
      bootstrap: true,
      preparedTargetLease: "prepared-b",
      revision: "rev-1",
      trigger: "manual"
    });
    expect(primaryWorkspace.commitDesktopRoot).not.toHaveBeenCalled();
  });

  it("restores an established cloud notebook into the persisted Workspace without a picker", async () => {
    const runtime = createDefaultAppRuntime();
    const openMarkdownFolder = vi.fn(async () => ({ name: "Unexpected", path: "/Unexpected" }));
    const prepareDesktopNotebookTarget = vi.fn(async () => ({
      lease: "prepared-existing-b",
      notesRoot: "/Workspace/B"
    }));
    const sync = vi.fn(async () => syncResult("/Workspace/B"));
    configureAppRuntime({
      ...runtime,
      files: {
        ...runtime.files,
        openMarkdownFolder
      },
      syncConfig: { ...runtime.syncConfig, sync },
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget
      }
    });
    const commitDesktopRoot = vi.fn(async () => "/Workspace/B");
    const primaryWorkspace = createPrimaryWorkspace({
      commitDesktopRoot,
      root: "/Workspace/A",
      workspaceRoot: "/Workspace"
    });
    const { result, rerender } = renderCoordinator({
      primaryRoot: "/Workspace/A",
      primaryWorkspace
    });

    const restoring = result.current.restoreDesktopNotebook("B", "/Workspace");
    await waitFor(() => expect(sync).toHaveBeenCalledOnce());
    rerender({ root: "/Workspace/B" });

    await expect(restoring).resolves.toBe("/Workspace/B");
    expect(openMarkdownFolder).not.toHaveBeenCalled();
    expect(prepareDesktopNotebookTarget).toHaveBeenCalledWith({
      notebookName: "B",
      parentPath: "/Workspace"
    });
    expect(sync).toHaveBeenCalledWith({
      bootstrap: true,
      preparedTargetLease: "prepared-existing-b",
      revision: "rev-1",
      trigger: "manual"
    });
    expect(commitDesktopRoot).not.toHaveBeenCalled();
  });

  it("mounts the desktop root committed by native bootstrap without a second path commit", async () => {
    const runtime = createDefaultAppRuntime();
    const sync = vi.fn(async () => syncResult("/Workspace/B"));
    configureAppRuntime({
      ...runtime,
      syncConfig: { ...runtime.syncConfig, sync },
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget: vi.fn(async () => ({
          lease: "native-commit-b",
          notesRoot: "/Workspace/B"
        }))
      }
    });
    const commitDesktopRoot = vi.fn(async () => "/Workspace/B");
    const { result, rerender } = renderCoordinator({
      primaryRoot: "/Workspace/A",
      primaryWorkspace: createPrimaryWorkspace({
        commitDesktopRoot,
        root: "/Workspace/A",
        workspaceRoot: "/Workspace"
      })
    });

    const restoring = result.current.restoreDesktopNotebook("B", "/Workspace");
    await waitFor(() => expect(sync).toHaveBeenCalledOnce());
    rerender({ root: "/Workspace/B" });

    await expect(restoring).resolves.toBe("/Workspace/B");
    expect(commitDesktopRoot).not.toHaveBeenCalled();
    expect(sync).toHaveBeenCalledOnce();
  });

  it("discards an unconsumed prepared restore lease when flushing fails", async () => {
    const runtime = createDefaultAppRuntime();
    const discardPreparedDesktopNotebookTarget = vi.fn(async () => undefined);
    const sync = vi.fn(async () => syncResult("/Restore Parent/Cloud Notes"));
    configureAppRuntime({
      ...runtime,
      files: {
        ...runtime.files,
        openMarkdownFolder: vi.fn(async () => ({ name: "Restore Parent", path: "/Restore Parent" }))
      },
      syncConfig: { ...runtime.syncConfig, sync },
      workspace: {
        ...runtime.workspace,
        discardPreparedDesktopNotebookTarget,
        prepareDesktopNotebookTarget: vi.fn(async () => ({
          lease: "unconsumed-cloud-notes",
          notesRoot: "/Restore Parent/Cloud Notes"
        }))
      }
    });
    const { result } = renderCoordinator({
      flushActiveDocument: vi.fn(async () => {
        throw new Error("flush-failed");
      })
    });

    await expect(result.current.restoreDesktopNotebook("Cloud Notes")).resolves.toBeNull();

    expect(sync).not.toHaveBeenCalled();
    expect(discardPreparedDesktopNotebookTarget).toHaveBeenCalledOnce();
    expect(discardPreparedDesktopNotebookTarget).toHaveBeenCalledWith("unconsumed-cloud-notes");
  });

  it("discards a prepared restore lease when native target validation fails", async () => {
    const runtime = createDefaultAppRuntime();
    const discardPreparedDesktopNotebookTarget = vi.fn(async () => undefined);
    const sync = vi.fn(async () => syncResult("/Restore Parent/Other Notes"));
    configureAppRuntime({
      ...runtime,
      files: {
        ...runtime.files,
        openMarkdownFolder: vi.fn(async () => ({ name: "Restore Parent", path: "/Restore Parent" }))
      },
      syncConfig: { ...runtime.syncConfig, sync },
      workspace: {
        ...runtime.workspace,
        discardPreparedDesktopNotebookTarget,
        prepareDesktopNotebookTarget: vi.fn(async () => ({
          lease: "invalid-cloud-notes",
          notesRoot: "/Restore Parent/Other Notes"
        }))
      }
    });
    const { result } = renderCoordinator();

    await expect(result.current.restoreDesktopNotebook("Cloud Notes")).resolves.toBeNull();

    expect(sync).not.toHaveBeenCalled();
    expect(discardPreparedDesktopNotebookTarget).toHaveBeenCalledOnce();
    expect(discardPreparedDesktopNotebookTarget).toHaveBeenCalledWith("invalid-cloud-notes");
  });

  it("accepts a successfully bootstrapped native commit exactly once", async () => {
    const runtime = createDefaultAppRuntime();
    const sync = vi.fn(async () => syncResult("/Restore Parent/Cloud Notes"));
    const openMarkdownFolder = vi.fn(async () => ({
      name: "Restore Parent",
      path: "/Restore Parent"
    }));
    configureAppRuntime({
      ...runtime,
      files: {
        ...runtime.files,
        openMarkdownFolder
      },
      syncConfig: { ...runtime.syncConfig, sync },
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget: vi.fn(async () => ({
          lease: "prepared-cloud-notes",
          notesRoot: "/Restore Parent/Cloud Notes"
        }))
      }
    });
    const commitDesktopRoot = vi.fn(async () => "/Restore Parent/Cloud Notes");
    const primaryWorkspace = createPrimaryWorkspace({ commitDesktopRoot });
    const { result, rerender } = renderCoordinator({ primaryWorkspace });

    const restoring = result.current.restoreDesktopNotebook("Cloud Notes");
    await waitFor(() => expect(sync).toHaveBeenCalledTimes(1));
    rerender({ root: "/Restore Parent/Cloud Notes" });

    await expect(restoring).resolves.toBe("/Restore Parent/Cloud Notes");
    expect(openMarkdownFolder).toHaveBeenCalledOnce();
    expect(commitDesktopRoot).not.toHaveBeenCalled();
  });

  it("rejects a stale bootstrap result without publishing or rewriting global sync configuration", async () => {
    const runtime = createDefaultAppRuntime();
    const patch = vi.fn(runtime.syncConfig.patch);
    const requestApply = vi.fn(runtime.syncConfig.requestApply);
    const setEditing = vi.fn(runtime.syncConfig.setEditing);
    const sync = vi.fn(async () => ({
      ...syncResult("/Workspace/B"),
      revision: "stale-revision"
    }));
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        patch,
        requestApply,
        setEditing,
        sync
      },
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget: vi.fn(async () => ({
          lease: "stale-b",
          notesRoot: "/Workspace/B"
        }))
      }
    });
    const primaryWorkspace = createPrimaryWorkspace({
      root: "/Workspace/A",
      workspaceRoot: "/Workspace"
    });
    const { result } = renderCoordinator({
      primaryRoot: "/Workspace/A",
      primaryWorkspace
    });

    await expect(result.current.restoreDesktopNotebook("B", "/Workspace")).resolves.toBeNull();

    expect(primaryWorkspace.commitDesktopRoot).not.toHaveBeenCalled();
    expect(patch).not.toHaveBeenCalled();
    expect(requestApply).not.toHaveBeenCalled();
    expect(setEditing).not.toHaveBeenCalled();
  });

  it("rejects a local switch during remote restore and allows an explicit retry", async () => {
    const runtime = createDefaultAppRuntime();
    const bootstrap = deferred<SyncRunResult>();
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        sync: vi.fn(() => bootstrap.promise)
      },
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget: vi.fn(async () => ({
          lease: "concurrent-b",
          notesRoot: "/Workspace/B"
        }))
      }
    });
    const commitDesktopRoot = vi.fn(async (path: string) => path);
    const primaryWorkspace = createPrimaryWorkspace({
      commitDesktopRoot,
      root: "/Workspace/A",
      workspaceRoot: "/Workspace"
    });
    const { result, rerender } = renderCoordinator({
      primaryRoot: "/Workspace/A",
      primaryWorkspace
    });

    const restoring = result.current.restoreDesktopNotebook("B", "/Workspace");
    await waitFor(() => expect(result.current.switching).toBe(true));

    const competing = result.current.switchDesktopNotebook("/Workspace/C");
    await expect(competing).resolves.toBeNull();
    expect(commitDesktopRoot).not.toHaveBeenCalled();

    bootstrap.reject(new Error("retryable-bootstrap-failure"));
    await expect(restoring).resolves.toBeNull();

    const retry = result.current.switchDesktopNotebook("/Workspace/C");
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/Workspace/C"));
    rerender({ root: "/Workspace/C" });
    await expect(retry).resolves.toBe("/Workspace/C");
  });

  it("rejects a concurrent remote restore and allows an explicit retry", async () => {
    const runtime = createDefaultAppRuntime();
    const firstBootstrap = deferred<SyncRunResult>();
    let syncAttempt = 0;
    const prepareDesktopNotebookTarget = vi.fn(async ({ notebookName, parentPath }) => ({
      lease: `prepared-${notebookName}`,
      notesRoot: `${parentPath}/${notebookName}`
    }));
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        sync: vi.fn(async () => {
          syncAttempt += 1;
          if (syncAttempt === 1) return firstBootstrap.promise;
          return syncResult("/Workspace/C");
        })
      },
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget
      }
    });
    const commitDesktopRoot = vi.fn(async (path: string) => path);
    const primaryWorkspace = createPrimaryWorkspace({
      commitDesktopRoot,
      root: "/Workspace/A",
      workspaceRoot: "/Workspace"
    });
    const { result, rerender } = renderCoordinator({
      primaryRoot: "/Workspace/A",
      primaryWorkspace
    });

    const firstRestore = result.current.restoreDesktopNotebook("B", "/Workspace");
    await waitFor(() => expect(result.current.switching).toBe(true));

    await expect(result.current.restoreDesktopNotebook("C", "/Workspace")).resolves.toBeNull();
    expect(prepareDesktopNotebookTarget).toHaveBeenCalledTimes(1);
    expect(prepareDesktopNotebookTarget).toHaveBeenCalledWith({
      notebookName: "B",
      parentPath: "/Workspace"
    });

    firstBootstrap.reject(new Error("retryable-bootstrap-failure"));
    await expect(firstRestore).resolves.toBeNull();

    const retry = result.current.restoreDesktopNotebook("C", "/Workspace");
    await waitFor(() => expect(prepareDesktopNotebookTarget).toHaveBeenCalledWith({
      notebookName: "C",
      parentPath: "/Workspace"
    }));
    rerender({ root: "/Workspace/C" });
    await expect(retry).resolves.toBe("/Workspace/C");
    expect(commitDesktopRoot).not.toHaveBeenCalled();
  });

  it("rejects a remote restore while a local switch is genuinely in progress", async () => {
    const runtime = createDefaultAppRuntime();
    const prepareDesktopNotebookTarget = vi.fn(async () => ({
      lease: "unexpected-b",
      notesRoot: "/Workspace/B"
    }));
    configureAppRuntime({
      ...runtime,
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget
      }
    });
    const commitDesktopRoot = vi.fn(async (path: string) => path);
    const primaryWorkspace = createPrimaryWorkspace({
      commitDesktopRoot,
      root: "/Workspace/A",
      workspaceRoot: "/Workspace"
    });
    const { result, rerender } = renderCoordinator({
      primaryRoot: "/Workspace/A",
      primaryWorkspace
    });

    const localSwitch = result.current.switchDesktopNotebook("/Workspace/C");
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/Workspace/C"));

    await expect(result.current.restoreDesktopNotebook("B", "/Workspace")).resolves.toBeNull();
    expect(prepareDesktopNotebookTarget).not.toHaveBeenCalled();

    rerender({ root: "/Workspace/C" });
    await expect(localSwitch).resolves.toBe("/Workspace/C");
  });

  it("allows a remote restore immediately after the local switch caller settles", async () => {
    const runtime = createDefaultAppRuntime();
    const prepareDesktopNotebookTarget = vi.fn(async () => ({
      lease: "prepared-b-after-local",
      notesRoot: "/Workspace/B"
    }));
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        sync: vi.fn(async () => syncResult("/Workspace/B"))
      },
      workspace: {
        ...runtime.workspace,
        prepareDesktopNotebookTarget
      }
    });
    const commitDesktopRoot = vi.fn(async (path: string) => path);
    const primaryWorkspace = createPrimaryWorkspace({
      commitDesktopRoot,
      root: "/Workspace/A",
      workspaceRoot: "/Workspace"
    });
    const { result, rerender } = renderCoordinator({
      primaryRoot: "/Workspace/A",
      primaryWorkspace
    });

    const localSwitch = result.current.switchDesktopNotebook("/Workspace/C");
    const restoreAfterLocal = localSwitch.then((root) => {
      expect(root).toBe("/Workspace/C");
      return result.current.restoreDesktopNotebook("B", "/Workspace");
    });
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/Workspace/C"));
    rerender({ root: "/Workspace/C" });
    await waitFor(() => expect(prepareDesktopNotebookTarget).toHaveBeenCalledWith({
      notebookName: "B",
      parentPath: "/Workspace"
    }));
    rerender({ root: "/Workspace/B" });

    await expect(restoreAfterLocal).resolves.toBe("/Workspace/B");
    expect(commitDesktopRoot).toHaveBeenCalledTimes(1);
    expect(commitDesktopRoot).toHaveBeenCalledWith("/Workspace/C");
  });

  it("records only a successful canonical transaction as a recent notebook", async () => {
    const commitDesktopRoot = vi.fn(async (path: string) => (
      path === "/Broken" ? null : "/Canonical/Good"
    ));
    const primaryWorkspace = createPrimaryWorkspace({ commitDesktopRoot });
    const { result, rerender } = renderCoordinator({ primaryWorkspace });

    await expect(result.current.switchDesktopNotebook("/Broken")).resolves.toBeNull();
    expect(await getStoredRecentNotebooks()).toEqual([]);

    const switching = result.current.switchDesktopNotebook("/Alias/Good");
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledTimes(2));
    rerender({ root: "/Canonical/Good" });
    await switching;

    expect(await getStoredRecentNotebooks()).toEqual([
      { name: "Good", path: "/Canonical/Good" }
    ]);
    await waitFor(() => expect(result.current.recentNotebooks).toEqual([
      { name: "Good", path: "/Canonical/Good" }
    ]));
  });

  it("ignores a slow initial recent-notebook load after a newer removal", async () => {
    const initialLoad = deferred<Array<{ name: string; path: string }>>();
    vi.spyOn(localStateModule, "getStoredRecentNotebooks").mockReturnValueOnce(initialLoad.promise);
    vi.spyOn(localStateModule, "removeStoredRecentNotebook").mockResolvedValueOnce([]);
    const { result } = renderCoordinator();

    await act(async () => {
      await result.current.removeRecentNotebook("/Stale");
    });
    await act(async () => {
      initialLoad.resolve([{ name: "Stale", path: "/Stale" }]);
      await initialLoad.promise;
    });

    expect(result.current.recentNotebooks).toEqual([]);
  });

  it("does not own switching from an external editor window", async () => {
    const removeStoredRecentNotebook = vi
      .spyOn(localStateModule, "removeStoredRecentNotebook")
      .mockResolvedValueOnce([]);
    const saveStoredRecentNotebook = vi
      .spyOn(localStateModule, "saveStoredRecentNotebook")
      .mockResolvedValueOnce([]);
    const primaryWorkspace = createPrimaryWorkspace();
    const { result } = renderCoordinator({ primaryWindowOwner: false, primaryWorkspace });

    await expect(result.current.switchDesktopNotebook("/New")).resolves.toBeNull();
    await result.current.removeRecentNotebook("/Recent");

    expect(primaryWorkspace.commitDesktopRoot).not.toHaveBeenCalled();
    expect(saveStoredRecentNotebook).not.toHaveBeenCalled();
    expect(removeStoredRecentNotebook).not.toHaveBeenCalled();
  });

  it("queues a second realtime directory drain while the first switch is still mounting", async () => {
    const commitDesktopRoot = vi.fn(async (path: string) => path);
    const primaryWorkspace = createPrimaryWorkspace({ commitDesktopRoot });
    const { result, rerender } = renderCoordinator({ primaryWorkspace });

    let firstSwitch!: Promise<string | null>;
    act(() => {
      firstSwitch = result.current.switchDesktopNotebook("/First Drain");
    });
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/First Drain"));

    let secondSwitch!: Promise<string | null>;
    act(() => {
      secondSwitch = result.current.switchDesktopNotebook("/Second Drain");
    });
    expect(commitDesktopRoot).toHaveBeenCalledTimes(1);

    rerender({ root: "/First Drain" });
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/Second Drain"));
    rerender({ root: "/Second Drain" });

    await expect(firstSwitch).resolves.toBe("/First Drain");
    await expect(secondSwitch).resolves.toBe("/Second Drain");
    expect(commitDesktopRoot).toHaveBeenCalledTimes(2);
  });

  it("queues a later native event drain behind an active switch transaction", async () => {
    const runtime = createDefaultAppRuntime();
    let listener: ((event: { payload: NotebookSwitchRequest }) => unknown) | null = null;
    configureAppRuntime({
      ...runtime,
      events: {
        emit: async () => undefined,
        isAvailable: () => true,
        listen: async (_event, next) => {
          listener = next as (event: { payload: NotebookSwitchRequest }) => unknown;
          return () => {
            listener = null;
          };
        }
      }
    });
    const commitDesktopRoot = vi.fn(async (path: string) => path);
    const { rerender } = renderCoordinator({
      primaryWorkspace: createPrimaryWorkspace({ commitDesktopRoot })
    });
    await waitFor(() => expect(listener).not.toBeNull());

    act(() => {
      const deliver = listener as (event: { payload: NotebookSwitchRequest }) => unknown;
      deliver({ payload: { path: "/First Native Drain", source: "native-open" } });
    });
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/First Native Drain"));

    act(() => {
      const deliver = listener as (event: { payload: NotebookSwitchRequest }) => unknown;
      deliver({ payload: { path: "/Second Native Drain", source: "native-open" } });
    });
    expect(commitDesktopRoot).toHaveBeenCalledTimes(1);

    rerender({ root: "/First Native Drain" });
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/Second Native Drain"));
    rerender({ root: "/Second Native Drain" });
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledTimes(2));
  });

  it("settles an active and pending directory request when the owner unmounts", async () => {
    const commitDesktopRoot = vi.fn(async (path: string) => path);
    const { result, unmount } = renderCoordinator({
      primaryWorkspace: createPrimaryWorkspace({ commitDesktopRoot })
    });

    const activeSwitch = result.current.switchDesktopNotebook("/Active");
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/Active"));
    const pendingSwitch = result.current.switchDesktopNotebook("/Pending");

    unmount();

    await expect(activeSwitch).resolves.toBeNull();
    await expect(pendingSwitch).resolves.toBeNull();
    expect(commitDesktopRoot).toHaveBeenCalledTimes(1);
  });

  it("restarts the request pump when settled callers enqueue during the finally handoff", async () => {
    const commitDesktopRoot = vi.fn(async () => null);
    const { result } = renderCoordinator({
      primaryWorkspace: createPrimaryWorkspace({ commitDesktopRoot })
    });

    const firstSwitch = result.current.switchDesktopNotebook("/First Handoff");
    const handoffSwitches = firstSwitch.then(() => Promise.all([
      result.current.switchDesktopNotebook("/Coalesced Handoff"),
      result.current.switchDesktopNotebook("/Final Handoff")
    ]));

    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/Final Handoff"));
    await expect(handoffSwitches).resolves.toEqual([null, null]);
    expect(commitDesktopRoot).toHaveBeenCalledTimes(2);
    expect(commitDesktopRoot).not.toHaveBeenCalledWith("/Coalesced Handoff");
  });

  it("settles an active switch when its deferred commit resolves after owner unmount", async () => {
    const deferredCommit = deferred<string | null>();
    const commitDesktopRoot = vi.fn(() => deferredCommit.promise);
    const { result, unmount } = renderCoordinator({
      primaryWorkspace: createPrimaryWorkspace({ commitDesktopRoot })
    });
    let outcome = "pending";

    const switching = result.current.switchDesktopNotebook("/Deferred Commit");
    switching.then((root) => {
      outcome = root === null ? "settled-null" : `settled:${root}`;
    });
    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledWith("/Deferred Commit"));

    unmount();
    deferredCommit.resolve("/Deferred Commit");

    await waitFor(() => expect(outcome).toBe("settled-null"));
    await expect(switching).resolves.toBeNull();
  });

  it("coalesces simultaneous native directory requests to the last valid path", async () => {
    const runtime = createDefaultAppRuntime();
    let listener: ((event: { payload: NotebookSwitchRequest }) => unknown) | null = null;
    configureAppRuntime({
      ...runtime,
      events: {
        emit: async () => undefined,
        isAvailable: () => true,
        listen: async (_event, next) => {
          listener = next as (event: { payload: NotebookSwitchRequest }) => unknown;
          return () => {
            listener = null;
          };
        }
      }
    });
    const commitDesktopRoot = vi.fn(async () => null);
    renderCoordinator({
      primaryWorkspace: createPrimaryWorkspace({ commitDesktopRoot })
    });
    await waitFor(() => expect(listener).not.toBeNull());

    act(() => {
      const deliver = listener as (event: { payload: NotebookSwitchRequest }) => unknown;
      deliver({ payload: { path: "/First", source: "native-open" } });
      deliver({ payload: { path: "/Last", source: "native-open" } });
    });

    await waitFor(() => expect(commitDesktopRoot).toHaveBeenCalledOnce());
    expect(commitDesktopRoot).toHaveBeenCalledWith("/Last");
  });
});
