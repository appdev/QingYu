import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type {
  AcceptedMaintenanceJob,
  AppSyncConfigRuntime,
  RemoteNotebookCatalogEntry,
  SyncConfigDocument,
  SyncJobStatus
} from "../../lib/sync-config";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  getAppRuntime,
  resetAppRuntimeForTests
} from "../../runtime";
import { CompactRepositoryAccess } from "./CompactRepositoryAccess";

const repositoryId = "00000000-0000-4000-8000-0000000000d1";
const jobId = "00000000-0000-4000-8000-0000000000d2";

function configDocument(
  revision = "rev-1",
  enabled = true
): SyncConfigDocument {
  return {
    config: {
      enabled,
      generateConflictDocument: false,
      intervalSeconds: 30,
      mode: "automatic",
      provider: "s3",
      remoteRoot: "qingyu",
      s3: {
        accessKeyId: "",
        addressingStyle: "auto",
        bucket: "notes",
        endpointUrl: "https://s3.example.test",
        region: "auto",
        requestTimeoutSeconds: 60,
        secretAccessKey: "",
        tlsVerification: "verify"
      },
      version: 3,
      webdav: { password: "", serverUrl: "", username: "" }
    },
    configured: true,
    issues: [],
    readiness: enabled ? "ready" : "disabled",
    revision
  };
}

function entry(
  name = "Shared notes",
  overrides: Partial<RemoteNotebookCatalogEntry> = {}
): RemoteNotebookCatalogEntry {
  return {
    available: true,
    disabledReason: null,
    displayName: name,
    name,
    provider: "s3",
    repositoryId,
    ...overrides
  } as RemoteNotebookCatalogEntry;
}

function terminalJob(
  completionState: "failed" | "succeeded"
): SyncJobStatus {
  return {
    acceptedAt: "2026-08-02T10:00:00Z",
    completionState,
    error: completionState === "failed" ? {
      category: "authentication",
      code: "repository-auth-failed",
      httpStatus: null,
      method: null,
      objectId: null,
      operation: "repository-recovery",
      provider: "s3",
      providerErrorCode: null,
      relativePath: null,
      requestId: null,
      runId: jobId
    } : null,
    finishedAt: "2026-08-02T10:00:01Z",
    jobId,
    provider: "s3",
    revision: "rev-1",
    summary: completionState === "succeeded" ? {
      bytesDownloaded: 4,
      bytesUploaded: 0,
      conflictFiles: 0,
      downloadedFiles: 1,
      scannedFiles: 1,
      skippedFiles: 0,
      uploadedFiles: 0
    } : null
  };
}

function deferred<T>() {
  let resolve!: (value: T) => undefined;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = (value) => {
      resolvePromise(value);
      return undefined;
    };
  });
  return { promise, resolve };
}

function installRuntime(overrides: Partial<AppSyncConfigRuntime> = {}) {
  const runtime = createDefaultAppRuntime();
  const syncConfig: AppSyncConfigRuntime = {
    ...runtime.syncConfig,
    bindRepository: vi.fn(async () => ({ jobId, notesRoot: "kernel-workspace://primary", repositoryId })),
    changeGlobalKey: vi.fn(async () => ({
      jobId: "kernel-key-import-completed",
      operation: "change-global-key" as const,
      repositoryId: null
    })),
    initializeGlobalKey: vi.fn(async () => ({ configured: true })),
    listNotebooks: vi.fn(async () => [entry()]),
    loadJob: vi.fn(async () => terminalJob("succeeded")),
    loadKeyState: vi.fn(async () => ({ configured: true })),
    ...overrides
  };
  configureAppRuntime({
    ...runtime,
    features: { ...runtime.features, dejavuSync: true },
    kernel: { ...runtime.kernel, availability: "available" },
    syncConfig
  });
  return syncConfig;
}

function renderAccess(
  overrides: Partial<{
    configDocument: SyncConfigDocument;
    dirty: boolean;
    primaryRoot: string;
    saving: boolean;
  }> = {}
) {
  return render(
    <CompactRepositoryAccess
      configDocument={overrides.configDocument ?? configDocument()}
      dirty={overrides.dirty ?? false}
      language="en"
      primaryRoot={overrides.primaryRoot ?? "kernel-workspace://primary"}
      saving={overrides.saving ?? false}
    />
  );
}

describe("CompactRepositoryAccess", () => {
  afterEach(() => {
    resetAppRuntimeForTests();
    vi.restoreAllMocks();
  });

  it("imports an absent key without exposing it and then loads the exact-revision catalog", async () => {
    const syncConfig = installRuntime({
      loadKeyState: vi.fn(async () => ({ configured: false }))
    });
    renderAccess();

    expect(await screen.findByText("No repository key has been created on this device.")).toBeVisible();
    expect(syncConfig.listNotebooks).not.toHaveBeenCalled();

    const input = screen.getByLabelText("Repository key or passphrase");
    fireEvent.change(input, { target: { value: "  creator-secret-key  " } });
    expect(input).toHaveAttribute("type", "password");
    fireEvent.click(screen.getByRole("button", { name: "Import key" }));

    await waitFor(() => expect(syncConfig.initializeGlobalKey).toHaveBeenCalledWith({
      key: "creator-secret-key"
    }));
    expect(input).toHaveValue("");
    expect(document.body).not.toHaveTextContent("creator-secret-key");
    await waitFor(() => expect(syncConfig.listNotebooks).toHaveBeenCalledWith({ revision: "rev-1" }));
  });

  it("restores the authoritative repository binding when the compact catalog remounts", async () => {
    const loadRepositoryBinding = vi.fn(async () => ({ repositoryId }));
    installRuntime({ loadRepositoryBinding });

    const firstView = renderAccess();
    expect(await screen.findByRole("radio", { name: "Shared notes" })).toBeChecked();
    expect(screen.getByRole("button", { name: "Join notebook" })).toBeDisabled();
    expect(loadRepositoryBinding).toHaveBeenCalledWith({
      notesRoot: "kernel-workspace://primary"
    });

    firstView.unmount();
    renderAccess();

    expect(await screen.findByRole("radio", { name: "Shared notes" })).toBeChecked();
    expect(loadRepositoryBinding).toHaveBeenCalledTimes(2);
  });

  it("fails closed when repository key state cannot be determined", async () => {
    const initializeGlobalKey = vi.fn(async () => ({ configured: true }));
    installRuntime({
      initializeGlobalKey,
      loadKeyState: vi.fn(async () => {
        throw new Error("unknown key state");
      })
    });
    renderAccess();

    expect(await screen.findByText("Repository key status or import failed. Try again.")).toBeVisible();
    expect(screen.queryByLabelText("Repository key or passphrase")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Import key" })).not.toBeInTheDocument();
    expect(initializeGlobalKey).not.toHaveBeenCalled();
  });

  it("does not expose a partial repository flow without an active Kernel job contract", () => {
    const runtime = createDefaultAppRuntime();
    const loadKeyState = vi.fn(async () => ({ configured: false }));
    configureAppRuntime({
      ...runtime,
      features: { ...runtime.features, dejavuSync: true },
      syncConfig: { ...runtime.syncConfig, loadKeyState }
    });

    renderAccess();

    expect(screen.queryByRole("region", { name: "Repository access" })).not.toBeInTheDocument();
    expect(loadKeyState).not.toHaveBeenCalled();
  });

  it("requires destructive confirmation to change a configured key and invalidates the old catalog", async () => {
    const pendingChange = deferred<AcceptedMaintenanceJob>();
    const syncConfig = installRuntime({
      changeGlobalKey: vi.fn(() => pendingChange.promise)
    });
    const rejectedConfirmation = deferred<boolean>();
    const acceptedConfirmation = deferred<boolean>();
    const confirm = vi.fn()
      .mockReturnValueOnce(rejectedConfirmation.promise)
      .mockReturnValueOnce(acceptedConfirmation.promise);
    Object.assign(getAppRuntime().dialog, { confirm });
    const browserConfirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    renderAccess();

    expect(await screen.findByRole("radio", { name: "Shared notes" })).toBeEnabled();
    fireEvent.click(screen.getByRole("radio", { name: "Shared notes" }));
    const input = screen.getByLabelText("Repository key or passphrase");
    fireEvent.change(input, { target: { value: "replacement-key" } });
    fireEvent.click(screen.getByRole("button", { name: "Change key" }));
    expect(syncConfig.changeGlobalKey).not.toHaveBeenCalled();

    await act(async () => {
      rejectedConfirmation.resolve(false);
    });
    expect(syncConfig.changeGlobalKey).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Change key" }));
    expect(syncConfig.changeGlobalKey).not.toHaveBeenCalled();
    await act(async () => {
      acceptedConfirmation.resolve(true);
    });
    expect(screen.queryByRole("radio", { name: "Shared notes" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Join notebook" })).not.toBeInTheDocument();
    pendingChange.resolve({
      jobId: "kernel-key-import-completed",
      operation: "change-global-key",
      repositoryId: null
    });
    await waitFor(() => expect(syncConfig.changeGlobalKey).toHaveBeenCalledWith({
      confirmed: true,
      newKey: "replacement-key"
    }));
    expect(confirm).toHaveBeenCalledWith(
      "Change the global key? Local repository data will be reset and current bindings disabled."
    );
    expect(browserConfirm).not.toHaveBeenCalled();
    expect(syncConfig.bindRepository).not.toHaveBeenCalled();
    await waitFor(() => expect(syncConfig.listNotebooks).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("radio", { name: "Shared notes" })).not.toBeChecked();
  });

  it("prevents overlapping configured-key changes while confirmation is pending", async () => {
    const pendingConfirmation = deferred<boolean>();
    const pendingChange = deferred<AcceptedMaintenanceJob>();
    const syncConfig = installRuntime({
      changeGlobalKey: vi.fn(() => pendingChange.promise)
    });
    const confirm = vi.fn(() => pendingConfirmation.promise);
    Object.assign(getAppRuntime().dialog, { confirm });
    renderAccess();

    const input = await screen.findByLabelText("Repository key or passphrase");
    fireEvent.change(input, { target: { value: "replacement-key" } });
    const changeKey = screen.getByRole("button", { name: "Change key" });
    fireEvent.click(changeKey);
    fireEvent.click(changeKey);

    expect(confirm).toHaveBeenCalledTimes(1);
    expect(syncConfig.changeGlobalKey).not.toHaveBeenCalled();

    await act(async () => {
      pendingConfirmation.resolve(true);
    });

    await waitFor(() => expect(syncConfig.changeGlobalKey).toHaveBeenCalledTimes(1));
    await act(async () => {
      pendingChange.resolve({
        jobId: "kernel-key-import-completed",
        operation: "change-global-key",
        repositoryId: null
      });
    });
  });

  it.each([
    ["malformed", new Error("malformed catalog with secret backend detail")],
    ["transport", new Error("Authorization: Bearer must-not-render")]
  ])("shows a safe retry state for a %s catalog failure", async (_kind, failure) => {
    const listNotebooks = vi.fn()
      .mockRejectedValueOnce(failure)
      .mockResolvedValueOnce([]);
    installRuntime({ listNotebooks });
    renderAccess();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The cloud notebook list could not be refreshed.");
    expect(alert).not.toHaveTextContent(/secret backend detail|bearer|authorization/iu);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));

    expect(await screen.findByText("No cloud notebooks found.")).toBeVisible();
    expect(listNotebooks).toHaveBeenNthCalledWith(1, { revision: "rev-1" });
    expect(listNotebooks).toHaveBeenNthCalledWith(2, { revision: "rev-1" });
  });

  it("gates catalog and selection on a stable draft revision", async () => {
    const listNotebooks = vi.fn(async () => [entry()]);
    const syncConfig = installRuntime({ listNotebooks });
    const view = renderAccess({ dirty: true });

    expect(await screen.findByText("Finish saving the sync configuration before choosing a cloud notebook.")).toBeVisible();
    expect(listNotebooks).not.toHaveBeenCalled();

    view.rerender(
      <CompactRepositoryAccess
        configDocument={configDocument("rev-1")}
        dirty={false}
        language="en"
        primaryRoot="kernel-workspace://primary"
        saving={false}
      />
    );
    expect(await screen.findByRole("radio", { name: "Shared notes" })).toBeEnabled();
    expect(listNotebooks).toHaveBeenLastCalledWith({ revision: "rev-1" });

    view.rerender(
      <CompactRepositoryAccess
        configDocument={configDocument("rev-2")}
        dirty={false}
        language="en"
        primaryRoot="kernel-workspace://primary"
        saving={false}
      />
    );
    await waitFor(() => expect(listNotebooks).toHaveBeenLastCalledWith({ revision: "rev-2" }));
    fireEvent.click(screen.getByRole("radio", { name: "Shared notes" }));
    vi.spyOn(window, "confirm").mockReturnValue(true);
    fireEvent.click(screen.getByRole("button", { name: "Join notebook" }));

    await waitFor(() => expect(syncConfig.bindRepository).toHaveBeenCalledWith({
      displayName: "Shared notes",
      notesRoot: "kernel-workspace://primary",
      repositoryId,
      revision: "rev-2"
    }));
  });

  it("awaits one native runtime confirmation before compact bind submission", async () => {
    const pendingConfirmation = deferred<boolean>();
    const syncConfig = installRuntime();
    const confirm = vi.fn(() => pendingConfirmation.promise);
    Object.assign(getAppRuntime().dialog, { confirm });
    const browserConfirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    const join = screen.getByRole("button", { name: "Join notebook" });
    fireEvent.click(join);
    fireEvent.click(join);

    expect(confirm).toHaveBeenCalledTimes(1);
    expect(confirm).toHaveBeenCalledWith(
      "Join this cloud notebook? Existing local files with matching names will be merged during recovery."
    );
    expect(browserConfirm).not.toHaveBeenCalled();
    expect(syncConfig.bindRepository).not.toHaveBeenCalled();

    await act(async () => {
      pendingConfirmation.resolve(true);
    });

    await waitFor(() => expect(syncConfig.bindRepository).toHaveBeenCalledTimes(1));
    expect(syncConfig.bindRepository).toHaveBeenCalledWith({
      displayName: "Shared notes",
      notesRoot: "kernel-workspace://primary",
      repositoryId,
      revision: "rev-1"
    });
  });

  it("does not bind an obsolete workspace root after async confirmation", async () => {
    const pendingConfirmation = deferred<boolean>();
    const syncConfig = installRuntime();
    Object.assign(getAppRuntime().dialog, {
      confirm: vi.fn(() => pendingConfirmation.promise)
    });
    const view = renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    fireEvent.click(screen.getByRole("button", { name: "Join notebook" }));
    view.rerender(
      <CompactRepositoryAccess
        configDocument={configDocument()}
        dirty={false}
        language="en"
        primaryRoot="kernel-workspace://replacement"
        saving={false}
      />
    );

    await act(async () => {
      pendingConfirmation.resolve(true);
    });

    expect(syncConfig.bindRepository).not.toHaveBeenCalled();
  });

  it("does not bind an obsolete catalog revision after async confirmation", async () => {
    const pendingConfirmation = deferred<boolean>();
    const syncConfig = installRuntime();
    Object.assign(getAppRuntime().dialog, {
      confirm: vi.fn(() => pendingConfirmation.promise)
    });
    const view = renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    fireEvent.click(screen.getByRole("button", { name: "Join notebook" }));
    view.rerender(
      <CompactRepositoryAccess
        configDocument={configDocument("rev-2")}
        dirty={false}
        language="en"
        primaryRoot="kernel-workspace://primary"
        saving={false}
      />
    );
    await waitFor(() => expect(syncConfig.listNotebooks).toHaveBeenLastCalledWith({
      revision: "rev-2"
    }));

    await act(async () => {
      pendingConfirmation.resolve(true);
    });

    expect(syncConfig.bindRepository).not.toHaveBeenCalled();
  });

  it("does not bind after the compact repository screen unmounts during confirmation", async () => {
    const pendingConfirmation = deferred<boolean>();
    const syncConfig = installRuntime();
    Object.assign(getAppRuntime().dialog, {
      confirm: vi.fn(() => pendingConfirmation.promise)
    });
    const view = renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    fireEvent.click(screen.getByRole("button", { name: "Join notebook" }));
    view.unmount();

    await act(async () => {
      pendingConfirmation.resolve(true);
    });

    expect(syncConfig.bindRepository).not.toHaveBeenCalled();
  });

  it("does not bind a repository deselected during async confirmation", async () => {
    const pendingConfirmation = deferred<boolean>();
    const syncConfig = installRuntime({
      listNotebooks: vi.fn(async () => [
        entry(),
        entry("Other notes", {
          displayName: "Other notes",
          repositoryId: "00000000-0000-4000-8000-0000000000d3"
        })
      ])
    });
    Object.assign(getAppRuntime().dialog, {
      confirm: vi.fn(() => pendingConfirmation.promise)
    });
    renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    fireEvent.click(screen.getByRole("button", { name: "Join notebook" }));
    fireEvent.click(screen.getByRole("radio", { name: "Other notes" }));

    await act(async () => {
      pendingConfirmation.resolve(true);
    });

    expect(syncConfig.bindRepository).not.toHaveBeenCalled();
  });

  it.each([
    ["canceled", () => Promise.resolve(false)],
    ["unavailable", () => Promise.reject(new Error("native dialog unavailable"))]
  ])("fails closed when runtime confirmation is %s", async (_state, firstConfirmation) => {
    const syncConfig = installRuntime();
    const confirm = vi.fn()
      .mockImplementationOnce(firstConfirmation)
      .mockResolvedValueOnce(true);
    Object.assign(getAppRuntime().dialog, { confirm });
    vi.spyOn(window, "confirm").mockReturnValue(false);
    renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    const join = screen.getByRole("button", { name: "Join notebook" });
    fireEvent.click(join);
    await act(async () => Promise.resolve());

    expect(syncConfig.bindRepository).not.toHaveBeenCalled();
    expect(screen.queryByText("Starting repository recovery…")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(join).toBeEnabled();

    fireEvent.click(join);

    await waitFor(() => expect(syncConfig.bindRepository).toHaveBeenCalledTimes(1));
    expect(confirm).toHaveBeenCalledTimes(2);
  });

  it("shows accepted separately and waits for terminal success before claiming recovery", async () => {
    const pendingTerminal = deferred<SyncJobStatus>();
    const loadJob = vi.fn(() => pendingTerminal.promise);
    const enable = vi.fn();
    const patch = vi.fn();
    const syncConfig = installRuntime({ enable, loadJob, patch });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    renderAccess({ configDocument: configDocument("rev-1", false) });

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    fireEvent.click(screen.getByRole("button", { name: "Join notebook" }));

    expect(await screen.findByText("Repository accepted. Recovery is still running.")).toBeVisible();
    expect(screen.queryByText("Notebook joined and recovered.")).not.toBeInTheDocument();
    expect(syncConfig.bindRepository).toHaveBeenCalledWith({
      displayName: "Shared notes",
      notesRoot: "kernel-workspace://primary",
      repositoryId,
      revision: "rev-1"
    });
    expect(loadJob).toHaveBeenCalledWith({ jobId });

    await act(async () => pendingTerminal.resolve(terminalJob("succeeded")));

    expect(await screen.findByText("Notebook joined and recovered.")).toBeVisible();
    expect(screen.getByText("Sync remains off. Recovery did not enable scheduling.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Join notebook" })).toBeDisabled();
    expect(enable).not.toHaveBeenCalled();
    expect(patch).not.toHaveBeenCalled();
  });

  it("synchronously blocks repeated bind taps before the first request is accepted", async () => {
    const pendingAccepted = deferred<Awaited<ReturnType<AppSyncConfigRuntime["bindRepository"]>>>();
    const bindRepository = vi.fn(() => pendingAccepted.promise);
    const syncConfig = installRuntime({ bindRepository });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    const join = screen.getByRole("button", { name: "Join notebook" });
    fireEvent.click(join);
    fireEvent.click(join);

    await waitFor(() => expect(bindRepository).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Starting repository recovery…")).toBeVisible();
    await act(async () => {
      pendingAccepted.resolve({ jobId, notesRoot: "kernel-workspace://primary", repositoryId });
    });
    expect(await screen.findByText("Notebook joined and recovered.")).toBeVisible();
    expect(syncConfig.loadJob).toHaveBeenCalledWith({ jobId });
  });

  it("preserves a safe active-run admission error without automatically repeating the bind", async () => {
    const bindRepository = vi.fn(async () => Promise.reject({
      code: "sync_run_unavailable",
      message: "Authorization: Bearer must-not-render"
    }));
    installRuntime({ bindRepository });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    fireEvent.click(screen.getByRole("button", { name: "Join notebook" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Another sync run is active or recovering. Wait for it to finish, then try joining again."
    );
    expect(alert).toHaveTextContent("sync_run_unavailable");
    expect(alert).not.toHaveTextContent(/authorization|bearer|must-not-render/iu);
    expect(bindRepository).toHaveBeenCalledTimes(1);
    await act(async () => Promise.resolve());
    expect(bindRepository).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Join notebook" })).toBeEnabled();
  });

  it("retries an accepted job status read without dispatching a second recovery", async () => {
    const loadJob = vi.fn()
      .mockRejectedValueOnce(new Error("temporary transport failure"))
      .mockResolvedValueOnce(terminalJob("succeeded"));
    const syncConfig = installRuntime({ loadJob });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    fireEvent.click(screen.getByRole("button", { name: "Join notebook" }));

    expect(await screen.findByText("Recovery status is unavailable. Check the accepted job again before retrying recovery.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Join notebook" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Check recovery status" }));

    expect(await screen.findByText("Notebook joined and recovered.")).toBeVisible();
    expect(syncConfig.bindRepository).toHaveBeenCalledTimes(1);
    expect(loadJob).toHaveBeenCalledTimes(2);
  });

  it("shows a terminal failure and never labels a failed accepted job as successful", async () => {
    installRuntime({ loadJob: vi.fn(async () => terminalJob("failed")) });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    renderAccess();

    fireEvent.click(await screen.findByRole("radio", { name: "Shared notes" }));
    fireEvent.click(screen.getByRole("button", { name: "Join notebook" }));

    expect(await screen.findByText("Repository recovery failed.")).toBeVisible();
    expect(screen.getByText("repository-auth-failed")).toBeVisible();
    expect(screen.queryByText("Notebook joined and recovered.")).not.toBeInTheDocument();
  });

  it("disables unavailable catalog entries with a localized safe reason", async () => {
    installRuntime({
      listNotebooks: vi.fn(async () => [entry("Unavailable", {
        available: false,
        disabledReason: "internal-provider-403"
      })])
    });
    renderAccess();

    const radio = await screen.findByRole("radio", { name: "Unavailable" });
    expect(radio).toBeDisabled();
    expect(within(radio.closest("label")!).getByText("This cloud notebook is unavailable.")).toBeVisible();
    expect(document.body).not.toHaveTextContent("internal-provider-403");
  });
});
