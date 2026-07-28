import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type {
  DejavuRepositoryStatus,
  QingYuSyncConfig,
  SyncConfigDocument,
  SyncConfigPatch,
  SyncConfigLoadResult,
  SyncConflictRecord
} from "../../lib/sync-config";
import { translate } from "../../test/settings-components";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppEventsRuntime,
  type RuntimeEvent
} from "../../runtime";
import { SyncSettings, type SyncSettingsProps } from "./SyncSettings";

const config: QingYuSyncConfig = {
  enabled: true,
  intervalSeconds: 900,
  mode: "automatic",
  provider: "webdav",
  remoteRoot: "qingyu/main",
  s3: {
    accessKeyId: "access-value",
    bucket: "notes-bucket",
    endpointUrl: "https://s3.example.test",
    region: "us-east-1",
    secretAccessKey: "secret-value",
    requestTimeoutSeconds: 60,
    addressingStyle: "auto",
    tlsVerification: "verify"
  },
  version: 3,
  webdav: {
    password: "password-value",
    serverUrl: "https://dav.example.test",
    username: "user-value"
  }
};

function document(overrides: Partial<SyncConfigDocument> = {}): SyncConfigDocument {
  return {
    config,
    configured: true,
    issues: [],
    readiness: "ready",
    revision: "rev-1",
    ...overrides
  };
}

function loaded(overrides: Partial<SyncConfigDocument> = {}): SyncConfigLoadResult {
  return { ...document(overrides), status: "loaded" };
}

function createProps(overrides: Partial<SyncSettingsProps> = {}): SyncSettingsProps {
  const configDocument = document();
  return {
    configDocument,
    loadResult: { ...configDocument, status: "loaded" },
    primaryRoot: "/Notes",
    saving: false,
    status: null,
    syncRunning: false,
    testing: false,
    translate,
    onEnable: vi.fn(async () => undefined),
    onPatch: vi.fn(async (_patch: SyncConfigPatch) => undefined),
    onReset: vi.fn(async () => undefined),
    onRunSync: vi.fn(async () => undefined),
    onSelectCloudNotebook: vi.fn(async () => undefined),
    onTestConnection: vi.fn(async () => ({ checkedTarget: "dav.example.test", provider: "webdav" as const })),
    ...overrides
  };
}

const repositoryId = "00000000-0000-4000-8000-0000000000d1";

function conflict(overrides: Partial<SyncConflictRecord> = {}): SyncConflictRecord {
  return {
    conflictId: "00000000-0000-4000-8000-0000000000d2",
    occurredAt: "2026-07-28T09:00:00Z",
    relativePath: "notes/conflicted.md",
    repositoryId,
    resolution: null,
    ...overrides
  };
}

function repositoryStatus(
  overrides: Partial<DejavuRepositoryStatus> = {}
): DejavuRepositoryStatus {
  return {
    attempt: 1,
    automaticFailureCount: 0,
    conflicts: [],
    error: null,
    jobId: "00000000-0000-4000-8000-0000000000d3",
    lastAttemptAt: "2026-07-28T10:00:00Z",
    lastDnsRetryAt: null,
    lastSuccessfulSyncAt: null,
    maintenance: {
      lastLocalPurgeAt: null,
      nextLocalPurgeAt: null
    },
    nextScheduledAt: null,
    phase: "attempting",
    repositoryId,
    sameCount: 0,
    transfer: {
      downloadBytes: 0,
      downloadChunks: 0,
      downloadFiles: 0,
      uploadBytes: 0,
      uploadChunks: 0,
      uploadFiles: 0
    },
    trigger: "manual",
    version: 1,
    ...overrides
  };
}

function formattedDate(value: string) {
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(value)
  );
}

function createEventBus() {
  const listeners = new Map<string, Set<(event: RuntimeEvent<unknown>) => unknown>>();
  const events: AppEventsRuntime = {
    emit: async <TPayload,>(event: string, payload: TPayload) => {
      for (const listener of listeners.get(event) ?? []) {
        await listener({ payload });
      }
    },
    isAvailable: () => true,
    listen: async <TPayload,>(
      event: string,
      listener: (event: RuntimeEvent<TPayload>) => unknown
    ) => {
      const registered = listeners.get(event) ?? new Set();
      const normalizedListener = listener as (event: RuntimeEvent<unknown>) => unknown;
      registered.add(normalizedListener);
      listeners.set(event, registered);
      return () => registered.delete(normalizedListener);
    }
  };
  return {
    events,
    emit: events.emit,
    listenerCount: (event: string) => listeners.get(event)?.size ?? 0
  };
}

function configureRepositoryStatus(
  status: DejavuRepositoryStatus,
  events?: AppEventsRuntime
) {
  const runtime = createDefaultAppRuntime();
  runtime.syncConfig.loadRepositoryStatus = async () => status;
  if (events) runtime.events = events;
  configureAppRuntime(runtime);
}

function renderS3Settings() {
  const s3Document = document({ config: { ...config, provider: "s3" } });
  return render(<SyncSettings {...createProps({
    configDocument: s3Document,
    loadResult: { ...s3Document, status: "loaded" }
  })} />);
}

describe("SyncSettings application scope", () => {
  afterEach(() => resetAppRuntimeForTests());

  it("groups S3 settings from basic choices through connection status", () => {
    const s3Document = document({ config: { ...config, provider: "s3" } });
    render(<SyncSettings {...createProps({
      configDocument: s3Document,
      loadResult: { ...s3Document, status: "loaded" }
    })} />);

    expect(screen.getAllByRole("heading").map((heading) => heading.textContent)).toEqual([
      "Basic settings",
      "Sync schedule",
      "S3 connection",
      "Repository key",
      "Repository maintenance",
      "Advanced options",
      "Connection and status"
    ]);
  });

  it("groups WebDAV connection settings without an empty advanced section", () => {
    render(<SyncSettings {...createProps()} />);

    expect(screen.getAllByRole("heading").map((heading) => heading.textContent)).toEqual([
      "Basic settings",
      "Sync schedule",
      "WebDAV connection",
      "Connection and status"
    ]);
  });

  it("shows the current notebook directory as a read-only target", () => {
    render(<SyncSettings {...createProps()} />);

    expect(screen.getByText("/Notes")).toBeVisible();
    expect(screen.queryByRole("textbox", { name: "Current notebook directory" })).not.toBeInTheDocument();
    expect(screen.getByText(
      /data namespace.*not a notebook name.*discovered automatically/i
    )).toBeVisible();
  });

  it("keeps configuration editable but disables immediate sync without a current notebook", () => {
    render(<SyncSettings {...createProps({ primaryRoot: null })} />);

    expect(screen.getByRole("textbox", { name: "Remote root" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Sync now" })).toBeDisabled();
    expect(screen.getByText("Not selected")).toBeVisible();
  });

  it("persists scheduling and remote-root fields with the app-level patch shape", () => {
    const onPatch = vi.fn(async (_patch: SyncConfigPatch) => undefined);
    render(<SyncSettings {...createProps({ onPatch })} />);

    fireEvent.change(screen.getByRole("spinbutton", { name: "Automatic sync interval" }), {
      target: { value: "600" }
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Sync mode" }), {
      target: { value: "fully-manual" }
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Remote root" }), {
      target: { value: "qingyu/team" }
    });

    expect(onPatch).toHaveBeenCalledWith({ field: "mode", value: "fully-manual" });
    expect(onPatch).toHaveBeenCalledWith({ field: "intervalSeconds", value: 600 });
    expect(onPatch).toHaveBeenCalledWith({ field: "remoteRoot", value: "qingyu/team" });
    expect(screen.queryByRole("spinbutton", { name: "Automatic sync interval" })).not.toBeInTheDocument();
  });

  it("persists S3 timeout addressing and TLS verification changes immediately", () => {
    const onPatch = vi.fn(async (_patch: SyncConfigPatch) => undefined);
    const s3Document = document({ config: { ...config, provider: "s3" } });
    render(<SyncSettings {...createProps({
      configDocument: s3Document,
      loadResult: { ...s3Document, status: "loaded" },
      onPatch
    })} />);

    fireEvent.change(screen.getByRole("spinbutton", { name: "Request timeout" }), {
      target: { value: "299" }
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Addressing style" }), {
      target: { value: "path" }
    });
    fireEvent.change(screen.getByRole("combobox", { name: "TLS certificate verification" }), {
      target: { value: "skip" }
    });

    expect(onPatch).toHaveBeenCalledWith({ field: "s3.requestTimeoutSeconds", value: 299 });
    expect(onPatch).toHaveBeenCalledWith({ field: "s3.addressingStyle", value: "path" });
    expect(onPatch).toHaveBeenCalledWith({ field: "s3.tlsVerification", value: "skip" });
  });

  it("imports a local key and dispatches repository maintenance as accepted background work", async () => {
    const runtime = createDefaultAppRuntime();
    const initializeGlobalKey = vi.fn(async () => ({ configured: true }));
    const rebuildLocalRepository = vi.fn(async () => ({
      jobId: "00000000-0000-4000-8000-0000000000c1",
      operation: "rebuild-local-repository" as const,
      repositoryId: "00000000-0000-4000-8000-0000000000c2"
    }));
    runtime.syncConfig.initializeGlobalKey = initializeGlobalKey;
    runtime.syncConfig.loadKeyState = vi.fn(async () => ({ configured: false }));
    runtime.syncConfig.loadRepositoryStatus = vi.fn(async () => ({
      attempt: 1,
      automaticFailureCount: 0,
      conflicts: [],
      error: null,
      jobId: "00000000-0000-4000-8000-0000000000c3",
      lastAttemptAt: "2026-07-28T10:00:00Z",
      lastDnsRetryAt: null,
      lastSuccessfulSyncAt: "2026-07-28T10:00:00Z",
      maintenance: { lastLocalPurgeAt: null, nextLocalPurgeAt: null },
      nextScheduledAt: null,
      phase: "succeeded" as const,
      repositoryId: "00000000-0000-4000-8000-0000000000c2",
      sameCount: 0,
      transfer: {
        downloadBytes: 0,
        downloadChunks: 0,
        downloadFiles: 0,
        uploadBytes: 0,
        uploadChunks: 0,
        uploadFiles: 0
      },
      trigger: "manual" as const,
      version: 1 as const
    }));
    runtime.syncConfig.rebuildLocalRepository = rebuildLocalRepository;
    configureAppRuntime(runtime);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    try {
      const s3Document = document({ config: { ...config, provider: "s3" } });
      render(<SyncSettings {...createProps({
        configDocument: s3Document,
        loadResult: { ...s3Document, status: "loaded" }
      })} />);

      const keyInput = await screen.findByLabelText("Repository key or passphrase");
      fireEvent.change(keyInput, { target: { value: "test passphrase" } });
      fireEvent.click(screen.getByRole("button", { name: "Import key" }));
      await waitFor(() => expect(initializeGlobalKey).toHaveBeenCalledWith({ key: "test passphrase" }));

      const rebuild = await screen.findByRole("button", { name: "Rebuild local repository" });
      fireEvent.click(rebuild);
      await waitFor(() => expect(rebuildLocalRepository).toHaveBeenCalledWith({
        confirmed: true,
        repositoryId: "00000000-0000-4000-8000-0000000000c2"
      }));
    } finally {
      resetAppRuntimeForTests();
      vi.restoreAllMocks();
    }
  });

  it("shows the active Dejavu phase trigger attempt time and next schedule", async () => {
    configureRepositoryStatus(repositoryStatus({
      lastAttemptAt: "2026-07-28T10:00:00Z",
      nextScheduledAt: "2026-07-28T11:00:00Z",
      phase: "attempting",
      trigger: "interval"
    }));

    renderS3Settings();

    const summary = await screen.findByRole("status", { name: "Dejavu background sync" });
    expect(summary).toHaveTextContent("Syncing");
    expect(summary).toHaveTextContent("Trigger: Scheduled interval");
    expect(summary).toHaveTextContent(`Last attempt: ${formattedDate("2026-07-28T10:00:00Z")}`);
    expect(summary).toHaveTextContent(`Next scheduled sync: ${formattedDate("2026-07-28T11:00:00Z")}`);
  });

  it("shows the retained Dejavu success time and safe failure details", async () => {
    configureRepositoryStatus(repositoryStatus({
      error: {
        code: "repository-auth-failed",
        operation: "repository-sync"
      },
      lastAttemptAt: "2026-07-28T11:00:00Z",
      lastSuccessfulSyncAt: "2026-07-28T09:00:00Z",
      phase: "failed",
      trigger: "save"
    }));

    renderS3Settings();

    const summary = await screen.findByRole("status", { name: "Dejavu background sync" });
    expect(summary).toHaveTextContent("Failed");
    expect(summary).toHaveTextContent(`Last success: ${formattedDate("2026-07-28T09:00:00Z")}`);
    expect(summary).toHaveTextContent("Error code: repository-auth-failed");
    expect(summary).toHaveTextContent("Operation: repository-sync");
  });

  it("shows Dejavu transfer maintenance and unresolved conflict totals", async () => {
    configureRepositoryStatus(repositoryStatus({
      conflicts: [
        conflict(),
        conflict({
          conflictId: "00000000-0000-4000-8000-0000000000d4",
          relativePath: "notes/second.md"
        }),
        conflict({
          conflictId: "00000000-0000-4000-8000-0000000000d5",
          relativePath: "notes/resolved.md",
          resolution: "keep-local"
        })
      ],
      lastSuccessfulSyncAt: "2026-07-28T10:00:00Z",
      maintenance: {
        lastLocalPurgeAt: "2026-07-28T08:00:00Z",
        nextLocalPurgeAt: "2026-07-29T08:00:00Z"
      },
      phase: "succeeded",
      transfer: {
        downloadBytes: 600,
        downloadChunks: 4,
        downloadFiles: 2,
        uploadBytes: 700,
        uploadChunks: 5,
        uploadFiles: 3
      }
    }));

    renderS3Settings();

    const summary = await screen.findByRole("status", { name: "Dejavu background sync" });
    expect(summary).toHaveTextContent("Succeeded");
    expect(summary).toHaveTextContent("Uploaded files: 3");
    expect(summary).toHaveTextContent("Uploaded chunks: 5");
    expect(summary).toHaveTextContent("Bytes uploaded: 700");
    expect(summary).toHaveTextContent("Downloaded files: 2");
    expect(summary).toHaveTextContent("Downloaded chunks: 4");
    expect(summary).toHaveTextContent("Bytes downloaded: 600");
    expect(summary).toHaveTextContent(`Last local cleanup: ${formattedDate("2026-07-28T08:00:00Z")}`);
    expect(summary).toHaveTextContent(`Next local cleanup: ${formattedDate("2026-07-29T08:00:00Z")}`);
    expect(summary).toHaveTextContent("Unresolved conflicts: 2");
    expect(summary).toHaveTextContent("notes/conflicted.md");
    expect(summary).toHaveTextContent("notes/second.md");
    expect(summary).not.toHaveTextContent("notes/resolved.md");
  });

  it("updates the visible Dejavu fields from a status event while mounted", async () => {
    const events = createEventBus();
    configureRepositoryStatus(repositoryStatus({
      phase: "attempting",
      trigger: "manual"
    }), events.events);
    renderS3Settings();
    const summary = await screen.findByRole("status", { name: "Dejavu background sync" });
    expect(summary).toHaveTextContent("Syncing");
    await waitFor(() => expect(events.listenerCount("qingyu://dejavu-sync-status-changed")).toBe(1));

    await act(async () => {
      await events.emit("qingyu://dejavu-sync-status-changed", repositoryStatus({
        phase: "failed",
        repositoryId: "00000000-0000-4000-8000-0000000000ff"
      }));
      await events.emit("qingyu://dejavu-sync-status-changed", repositoryStatus({
        lastSuccessfulSyncAt: "2026-07-28T12:00:00Z",
        phase: "succeeded",
        transfer: {
          downloadBytes: 20,
          downloadChunks: 2,
          downloadFiles: 1,
          uploadBytes: 10,
          uploadChunks: 1,
          uploadFiles: 1
        }
      }));
    });

    expect(summary).toHaveTextContent("Succeeded");
    expect(summary).toHaveTextContent(`Last success: ${formattedDate("2026-07-28T12:00:00Z")}`);
    expect(summary).toHaveTextContent("Bytes uploaded: 10");
    expect(summary).toHaveTextContent("Bytes downloaded: 20");
  });

  it("keeps a newer Dejavu event when the initial status load resolves late", async () => {
    const events = createEventBus();
    let resolveInitialStatus!: (status: DejavuRepositoryStatus) => unknown;
    const runtime = createDefaultAppRuntime();
    runtime.events = events.events;
    runtime.syncConfig.loadRepositoryStatus = () => new Promise<DejavuRepositoryStatus>((resolve) => {
      resolveInitialStatus = resolve;
    });
    configureAppRuntime(runtime);
    renderS3Settings();
    await waitFor(() => expect(events.listenerCount("qingyu://dejavu-sync-status-changed")).toBe(1));

    await act(async () => {
      await events.emit("qingyu://dejavu-sync-status-changed", repositoryStatus({
        lastSuccessfulSyncAt: "2026-07-28T12:00:00Z",
        phase: "succeeded",
        transfer: {
          downloadBytes: 20,
          downloadChunks: 2,
          downloadFiles: 1,
          uploadBytes: 10,
          uploadChunks: 1,
          uploadFiles: 1
        }
      }));
      resolveInitialStatus(repositoryStatus({
        phase: "attempting",
        trigger: "interval"
      }));
    });

    const summary = await screen.findByRole("status", { name: "Dejavu background sync" });
    expect(summary).toHaveTextContent("Succeeded");
    expect(summary).toHaveTextContent("Bytes uploaded: 10");

    await act(async () => {
      await events.emit("qingyu://dejavu-sync-status-changed", repositoryStatus({
        error: {
          code: "other-repository-failed",
          operation: "repository-sync"
        },
        phase: "failed",
        repositoryId: "00000000-0000-4000-8000-0000000000ff"
      }));
    });
    expect(summary).toHaveTextContent("Succeeded");
    expect(summary).not.toHaveTextContent("other-repository-failed");
  });

  it("renders only safe Dejavu diagnostics and relative conflict paths", async () => {
    const absolutePath = "/Users/alice/Private/credentials.md";
    const status = repositoryStatus({
      conflicts: [
        conflict({ relativePath: "notes/safe.md" }),
        conflict({
          conflictId: "00000000-0000-4000-8000-0000000000d6",
          relativePath: absolutePath
        })
      ],
      error: {
        code: "repository-sync-failed",
        operation: "repository-sync"
      },
      jobId: "00000000-0000-4000-8000-0000000000d7",
      phase: "failed"
    });
    configureRepositoryStatus(status);

    renderS3Settings();

    const summary = await screen.findByRole("status", { name: "Dejavu background sync" });
    expect(summary).toHaveTextContent("Error code: repository-sync-failed");
    expect(summary).toHaveTextContent("notes/safe.md");
    expect(summary).toHaveTextContent("Unresolved conflicts: 2");
    expect(summary).not.toHaveTextContent(absolutePath);
    expect(summary).not.toHaveTextContent(status.jobId);
    expect(summary).not.toHaveTextContent("secret-value");
  });

  it("keeps an empty S3 region while showing the automatic runtime value", () => {
    const s3Config = {
      ...config,
      provider: "s3" as const,
      s3: { ...config.s3, region: "" }
    };
    const s3Document = document({ config: s3Config });
    render(<SyncSettings {...createProps({
      configDocument: s3Document,
      loadResult: { ...s3Document, status: "loaded" }
    })} />);

    const region = screen.getByRole("textbox", { name: "S3 region" });
    expect(region).toHaveValue("");
    expect(region).toHaveAttribute("placeholder", "auto");
  });

  it("omits the loaded-state sync explanation callouts", () => {
    render(<SyncSettings {...createProps()} />);

    expect(screen.queryByText("Remote two-way sync")).not.toBeInTheDocument();
    expect(screen.queryByText("Plaintext credentials")).not.toBeInTheDocument();
    expect(screen.queryByText(/sync-config\.json stores credentials as plaintext in local application data/)).not.toBeInTheDocument();
  });

  it("creates the single app config when absent even without a primary root", () => {
    const onEnable = vi.fn(async () => undefined);
    render(<SyncSettings {...createProps({
      configDocument: null,
      loadResult: { revision: null, status: "absent" },
      onEnable,
      primaryRoot: null
    })} />);

    fireEvent.click(screen.getByRole("button", { name: "Create sync configuration" }));
    expect(onEnable).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Not selected")).toBeVisible();
  });

  it("keeps a failed optimistic value visible and retries the same field", async () => {
    const onPatch = vi.fn()
      .mockRejectedValueOnce(new Error("disk full"))
      .mockResolvedValueOnce(document({
        config: { ...config, remoteRoot: "qingyu/retry" },
        revision: "rev-2"
      }));
    render(<SyncSettings {...createProps({ onPatch })} />);

    fireEvent.change(screen.getByRole("textbox", { name: "Remote root" }), {
      target: { value: "qingyu/retry" }
    });

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("not saved"));
    expect(screen.getByRole("textbox", { name: "Remote root" })).toHaveValue("qingyu/retry");
    fireEvent.click(screen.getByRole("button", { name: "Retry unsaved changes" }));

    await waitFor(() => expect(onPatch).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByText(/not saved/i)).not.toBeInTheDocument());
    expect(onPatch).toHaveBeenLastCalledWith({ field: "remoteRoot", value: "qingyu/retry" });
  });

  it("resets malformed app config only after confirmation and exposes no file reveal action", () => {
    const onReset = vi.fn(async () => undefined);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<SyncSettings {...createProps({
      configDocument: null,
      loadResult: {
        issue: { code: "sync-config-malformed", message: "malformed" },
        revision: "bad-rev",
        status: "malformed"
      },
      onReset
    })} />);

    fireEvent.click(screen.getByRole("button", { name: "Reset configuration" }));
    expect(onReset).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Open configuration" })).not.toBeInTheDocument();
    confirm.mockRestore();
  });

  it("uses the saved app revision before enabling network actions", () => {
    const incompleteDocument = document({
      issues: [{ code: "sync-remote-root-invalid", field: "remoteRoot", message: "required" }],
      readiness: "incomplete"
    });
    const incomplete = { ...incompleteDocument, status: "loaded" as const };
    render(<SyncSettings {...createProps({
      configDocument: incompleteDocument,
      loadResult: incomplete
    })} />);

    expect(screen.getByRole("button", { name: "Test connection" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Sync now" })).toBeDisabled();
  });

  it.each(["webdav", "s3"] as const)("offers cloud notebook selection for configured %s and a current desktop root", (provider) => {
    const onSelectCloudNotebook = vi.fn(async () => undefined);
    const providerDocument = document({ config: { ...config, provider } });
    render(<SyncSettings {...createProps({
      configDocument: providerDocument,
      loadResult: { ...providerDocument, status: "loaded" },
      onSelectCloudNotebook
    })} />);

    const button = screen.getByRole("button", { name: "Select Cloud Notebook" });
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(onSelectCloudNotebook).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["incomplete provider", {
      configDocument: document({ configured: false, readiness: "incomplete" }),
      loadResult: loaded({ configured: false, readiness: "incomplete" })
    }],
    ["missing current notebook", { primaryRoot: null }],
    ["pending save", { saving: true }],
    ["connection test", { testing: true }],
    ["synchronization", { syncRunning: true }]
  ] as const)("disables cloud notebook selection for %s", (_name, overrides) => {
    render(<SyncSettings {...createProps(overrides)} />);

    expect(screen.getByRole("button", { name: "Select Cloud Notebook" })).toBeDisabled();
  });

  it("disables cloud notebook selection while an optimistic field draft is unresolved", () => {
    let resolvePatch!: (value: SyncConfigDocument) => unknown;
    const onPatch = vi.fn(() => new Promise<SyncConfigDocument>((resolve) => {
      resolvePatch = resolve;
    }));
    render(<SyncSettings {...createProps({ onPatch })} />);

    fireEvent.change(screen.getByRole("textbox", { name: "Remote root" }), {
      target: { value: "qingyu/team" }
    });

    expect(screen.getByRole("button", { name: "Select Cloud Notebook" })).toBeDisabled();
    resolvePatch(document({ revision: "rev-2" }));
  });

  it("allows cloud notebook selection when global synchronization is disabled but configured", () => {
    const disabledConfig = { ...config, enabled: false };
    const disabledDocument = document({
      config: disabledConfig,
      configured: true,
      readiness: "disabled"
    });
    render(<SyncSettings {...createProps({
      configDocument: disabledDocument,
      loadResult: { ...disabledDocument, status: "loaded" }
    })} />);

    expect(screen.getByRole("button", { name: "Select Cloud Notebook" })).toBeEnabled();
  });

  it.each([
    ["absent", { revision: null, status: "absent" }],
    ["malformed", {
      issue: { code: "sync-config-malformed", message: "malformed" },
      revision: "bad-rev",
      status: "malformed"
    }],
    ["unsupported", {
      issue: { code: "sync-config-unsupported", message: "unsupported" },
      revision: "future-rev",
      status: "unsupported",
      version: 2
    }]
  ] as const)("does not offer cloud notebook selection for %s configuration", (_name, loadResult) => {
    render(<SyncSettings {...createProps({ configDocument: null, loadResult })} />);

    expect(screen.queryByRole("button", { name: "Select Cloud Notebook" })).not.toBeInTheDocument();
  });

  it("reports a successful bounded connection test without displaying credentials", async () => {
    render(<SyncSettings {...createProps()} />);
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));

    await waitFor(() => expect(screen.getByText(/Connection succeeded/)).toBeVisible());
    expect(screen.queryByText("password-value")).not.toBeInTheDocument();
    expect(screen.queryByText("secret-value")).not.toBeInTheDocument();
  });
});
