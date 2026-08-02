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
  generateConflictDocument: false,
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
    dejavuSyncAvailable: true,
    loadResult: { ...configDocument, status: "loaded" },
    primaryRoot: "/Notes",
    saving: false,
    status: null,
    syncRunning: false,
    testing: false,
    translate,
    onEnable: vi.fn(async () => undefined),
    onOpenConflictHistory: vi.fn(),
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
    resolution: "keep-local",
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

const originalClipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");

function restoreClipboard() {
  if (originalClipboardDescriptor) {
    Object.defineProperty(navigator, "clipboard", originalClipboardDescriptor);
    return;
  }
  Reflect.deleteProperty(navigator, "clipboard");
}

function configureKeyExportRuntime(exportedKey: string) {
  const runtime = createDefaultAppRuntime();
  const exportGlobalKey = vi.fn(async () => exportedKey);
  runtime.syncConfig.exportGlobalKey = exportGlobalKey;
  runtime.syncConfig.loadKeyState = vi.fn(async () => ({ configured: true }));
  runtime.syncConfig.loadRepositoryStatus = vi.fn(async () => null);
  configureAppRuntime(runtime);

  return exportGlobalKey;
}

function installClipboard(writeText: (text: string) => Promise<unknown>) {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText }
  });
}

async function configuredKeyAction(name: string) {
  const action = await screen.findByRole("button", { name });
  await waitFor(() => expect(action).toBeEnabled());
  return action;
}

async function blobText(blob: Blob) {
  if (typeof blob.text === "function") return blob.text();

  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("error", () => reject(reader.error));
    reader.addEventListener("load", () => resolve(String(reader.result)));
    reader.readAsText(blob);
  });
}

describe("SyncSettings application scope", () => {
  afterEach(() => {
    resetAppRuntimeForTests();
    restoreClipboard();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

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

  it("keeps basic S3 sync available without loading or exposing Dejavu controls", async () => {
    const runtime = createDefaultAppRuntime();
    const loadKeyState = vi.fn(runtime.syncConfig.loadKeyState);
    const loadRepositoryStatus = vi.fn(runtime.syncConfig.loadRepositoryStatus);
    runtime.syncConfig.loadKeyState = loadKeyState;
    runtime.syncConfig.loadRepositoryStatus = loadRepositoryStatus;
    configureAppRuntime(runtime);
    const s3Document = document({ config: { ...config, provider: "s3" } });

    render(<SyncSettings {...createProps({
      configDocument: s3Document,
      dejavuSyncAvailable: false,
      loadResult: { ...s3Document, status: "loaded" }
    })} />);

    expect(screen.getAllByRole("heading").map((heading) => heading.textContent)).toEqual([
      "Basic settings",
      "Sync schedule",
      "S3 connection",
      "Advanced options",
      "Connection and status"
    ]);
    expect(screen.getByRole("textbox", { name: "S3 endpoint URL" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Test connection" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Sync now" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Select Cloud Notebook" })).toBeEnabled();
    expect(screen.queryByLabelText("Repository key or passphrase")).not.toBeInTheDocument();
    await waitFor(() => {
      expect(loadKeyState).not.toHaveBeenCalled();
      expect(loadRepositoryStatus).not.toHaveBeenCalled();
    });
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

  it("keeps SiYuan-style conflict document generation off by default and persists the switch", () => {
    const onPatch = vi.fn(async (_patch: SyncConfigPatch) => undefined);
    const s3Document = document({ config: { ...config, provider: "s3" } });
    render(<SyncSettings {...createProps({
      configDocument: s3Document,
      loadResult: { ...s3Document, status: "loaded" },
      onPatch
    })} />);

    const preference = screen.getByRole("switch", { name: "Create conflict document" });
    expect(preference).not.toBeChecked();
    fireEvent.click(preference);

    expect(onPatch).toHaveBeenCalledWith({ field: "generateConflictDocument", value: true });
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

  it("copies a confirmed repository key only through the secure-context clipboard", async () => {
    const exportedKey = "test-repository-key-material";
    const exportGlobalKey = configureKeyExportRuntime(exportedKey);
    const writeText = vi.fn(async () => undefined);
    installClipboard(writeText);
    vi.stubGlobal("isSecureContext", true);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const createObjectURL = vi.spyOn(URL, "createObjectURL");

    renderS3Settings();
    fireEvent.click(await configuredKeyAction("Copy key"));

    await waitFor(() => expect(writeText).toHaveBeenCalledWith(exportedKey));
    expect(confirm).toHaveBeenCalledWith(
      "Copy the repository key to the clipboard? Anyone with this key can read the encrypted repository."
    );
    expect(exportGlobalKey).toHaveBeenCalledWith({ confirmed: true });
    expect(createObjectURL).not.toHaveBeenCalled();
    expect(await screen.findByText("Repository key copied.")).toBeVisible();
    expect(globalThis.document.body).not.toHaveTextContent(exportedKey);
  });

  it("downloads a confirmed repository key from a non-secure HTTP context without exposing it in the DOM", async () => {
    const exportedKey = "test-repository-key-material";
    const exportGlobalKey = configureKeyExportRuntime(exportedKey);
    vi.stubGlobal("isSecureContext", false);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const createObjectURL = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:key-export");
    const revokeObjectURL = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    const clickedLink = { current: null as HTMLAnchorElement | null };
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      clickedLink.current = this;
    });

    renderS3Settings();
    fireEvent.click(await configuredKeyAction("Download key"));

    await waitFor(() => expect(createObjectURL).toHaveBeenCalledOnce());
    expect(confirm).toHaveBeenCalledWith(
      "Download the repository key as a plaintext file? Anyone with this file can read the encrypted repository. Store it securely."
    );
    expect(exportGlobalKey).toHaveBeenCalledWith({ confirmed: true });
    const blob = createObjectURL.mock.calls[0]?.[0] as Blob;
    expect(blob.type).toBe("text/plain;charset=utf-8");
    await expect(blobText(blob)).resolves.toBe(exportedKey);
    expect(clickedLink.current).toMatchObject({
      download: "qingyu-repository-key.txt",
      href: "blob:key-export",
      rel: "noopener"
    });
    expect(clickedLink.current?.textContent).toBe("");
    expect(clickedLink.current?.isConnected).toBe(false);
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:key-export");
    expect(await screen.findByText("Repository key downloaded.")).toBeVisible();
    expect(globalThis.document.body).not.toHaveTextContent(exportedKey);
  });

  it.each([
    { action: "Copy key", secure: true },
    { action: "Download key", secure: false }
  ])("does not read or release the repository key when $action confirmation is cancelled", async ({ action, secure }) => {
    const exportedKey = "test-repository-key-material";
    const exportGlobalKey = configureKeyExportRuntime(exportedKey);
    const writeText = vi.fn(async () => undefined);
    installClipboard(writeText);
    vi.stubGlobal("isSecureContext", secure);
    vi.spyOn(window, "confirm").mockReturnValue(false);
    const createObjectURL = vi.spyOn(URL, "createObjectURL");

    renderS3Settings();
    fireEvent.click(await configuredKeyAction(action));

    await Promise.resolve();
    expect(exportGlobalKey).not.toHaveBeenCalled();
    expect(writeText).not.toHaveBeenCalled();
    expect(createObjectURL).not.toHaveBeenCalled();
    expect(screen.queryByText("Repository key copied.")).not.toBeInTheDocument();
    expect(screen.queryByText("Repository key downloaded.")).not.toBeInTheDocument();
    expect(screen.queryByText("The operation could not be started.")).not.toBeInTheDocument();
    expect(globalThis.document.body).not.toHaveTextContent(exportedKey);
  });

  it("fails closed when a secure-context clipboard write is rejected", async () => {
    const exportedKey = "test-repository-key-material";
    const exportGlobalKey = configureKeyExportRuntime(exportedKey);
    const writeText = vi.fn(async () => Promise.reject(
      new DOMException("Clipboard denied", "NotAllowedError")
    ));
    installClipboard(writeText);
    vi.stubGlobal("isSecureContext", true);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const createObjectURL = vi.spyOn(URL, "createObjectURL");
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    renderS3Settings();
    fireEvent.click(await configuredKeyAction("Copy key"));

    expect(await screen.findByText("The operation could not be started.")).toBeVisible();
    expect(exportGlobalKey).toHaveBeenCalledOnce();
    expect(writeText).toHaveBeenCalledWith(exportedKey);
    expect(createObjectURL).not.toHaveBeenCalled();
    expect(globalThis.document.body).not.toHaveTextContent(exportedKey);
    expect(consoleError.mock.calls.flat().join(" ")).not.toContain(exportedKey);
  });

  it("cleans up a non-secure key download that the browser rejects without disclosing the key", async () => {
    const exportedKey = "test-repository-key-material";
    configureKeyExportRuntime(exportedKey);
    vi.stubGlobal("isSecureContext", false);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const createObjectURL = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:key-export");
    const revokeObjectURL = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const clickedLink = { current: null as HTMLAnchorElement | null };
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      clickedLink.current = this;
      throw new Error("Download blocked");
    });

    renderS3Settings();
    fireEvent.click(await configuredKeyAction("Download key"));

    expect(await screen.findByText("The operation could not be started.")).toBeVisible();
    expect(createObjectURL).toHaveBeenCalledOnce();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:key-export");
    expect(clickedLink.current?.isConnected).toBe(false);
    expect(globalThis.document.body.querySelector('a[download="qingyu-repository-key.txt"]')).toBeNull();
    expect(globalThis.document.body).not.toHaveTextContent(exportedKey);
    expect(consoleError.mock.calls.flat().join(" ")).not.toContain(exportedKey);
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

  it("shows Dejavu transfer maintenance and read-only conflict history", async () => {
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
    expect(summary).toHaveTextContent("Conflict history: 3");
    expect(summary).toHaveTextContent("notes/conflicted.md");
    expect(summary).toHaveTextContent("notes/second.md");
    expect(summary).toHaveTextContent("notes/resolved.md");
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

  it("adopts a newly bound Dejavu event only when it belongs to the current root", async () => {
    const events = createEventBus();
    let currentRootStatus: DejavuRepositoryStatus | null = null;
    let initialLoadPending = true;
    let resolveInitialStatus!: (status: DejavuRepositoryStatus | null) => unknown;
    const runtime = createDefaultAppRuntime();
    runtime.events = events.events;
    runtime.syncConfig.loadRepositoryStatus = () => {
      if (!initialLoadPending) return Promise.resolve(currentRootStatus);
      return new Promise<DejavuRepositoryStatus | null>((resolve) => {
        resolveInitialStatus = resolve;
      });
    };
    configureAppRuntime(runtime);
    renderS3Settings();
    await waitFor(() => expect(events.listenerCount("qingyu://dejavu-sync-status-changed")).toBe(1));
    await waitFor(() => expect(resolveInitialStatus).toBeDefined());

    await act(async () => {
      initialLoadPending = false;
      resolveInitialStatus(null);
    });
    expect(screen.getByText("The current notebook is not bound to a Dejavu repository.")).toBeVisible();

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
    expect(screen.queryByRole("status", { name: "Dejavu background sync" })).not.toBeInTheDocument();

    currentRootStatus = repositoryStatus({
      phase: "attempting",
      trigger: "manual"
    });
    await act(async () => {
      await events.emit("qingyu://dejavu-sync-status-changed", currentRootStatus);
    });

    const summary = await screen.findByRole("status", { name: "Dejavu background sync" });
    expect(summary).toHaveTextContent("Syncing");
    expect(summary).toHaveTextContent("Trigger: Manual");
    expect(summary).not.toHaveTextContent("other-repository-failed");

    await act(async () => {
      await events.emit("qingyu://dejavu-sync-status-changed", repositoryStatus({
        error: {
          code: "later-other-repository-failed",
          operation: "repository-sync"
        },
        phase: "failed",
        repositoryId: "00000000-0000-4000-8000-0000000000ff"
      }));
    });
    expect(summary).toHaveTextContent("Syncing");
    expect(summary).not.toHaveTextContent("later-other-repository-failed");
  });

  it("keeps a current-root bind event that arrives before a stale null snapshot", async () => {
    const events = createEventBus();
    let currentRootStatus: DejavuRepositoryStatus | null = null;
    let initialLoadPending = true;
    let resolveInitialStatus!: (status: DejavuRepositoryStatus | null) => unknown;
    const runtime = createDefaultAppRuntime();
    runtime.events = events.events;
    runtime.syncConfig.loadRepositoryStatus = () => {
      if (!initialLoadPending) return Promise.resolve(currentRootStatus);
      return new Promise<DejavuRepositoryStatus | null>((resolve) => {
        resolveInitialStatus = resolve;
      });
    };
    configureAppRuntime(runtime);
    renderS3Settings();
    await waitFor(() => expect(events.listenerCount("qingyu://dejavu-sync-status-changed")).toBe(1));
    await waitFor(() => expect(resolveInitialStatus).toBeDefined());

    currentRootStatus = repositoryStatus({
      phase: "attempting",
      trigger: "manual"
    });
    await act(async () => {
      await events.emit("qingyu://dejavu-sync-status-changed", repositoryStatus({
        phase: "failed",
        repositoryId: "00000000-0000-4000-8000-0000000000ff"
      }));
      await events.emit("qingyu://dejavu-sync-status-changed", currentRootStatus);
      initialLoadPending = false;
      resolveInitialStatus(null);
    });

    const summary = await screen.findByRole("status", { name: "Dejavu background sync" });
    expect(summary).toHaveTextContent("Syncing");
    expect(summary).toHaveTextContent("Trigger: Manual");
    expect(summary).not.toHaveTextContent("Failed");
  });

  it("does not let an older current-root ownership reload replace a newer binding", async () => {
    const events = createEventBus();
    let resolveOlderReload!: (status: DejavuRepositoryStatus) => unknown;
    let resolveNewerReload!: (status: DejavuRepositoryStatus) => unknown;
    let loadCount = 0;
    const olderStatus = repositoryStatus({
      error: {
        code: "stale-older-binding",
        operation: "repository-sync"
      },
      phase: "failed",
      repositoryId: "00000000-0000-4000-8000-0000000000aa"
    });
    const newerStatus = repositoryStatus({
      phase: "attempting",
      repositoryId: "00000000-0000-4000-8000-0000000000bb",
      trigger: "manual"
    });
    const runtime = createDefaultAppRuntime();
    runtime.events = events.events;
    runtime.syncConfig.loadRepositoryStatus = () => {
      loadCount += 1;
      if (loadCount === 1) return Promise.resolve(null);
      if (loadCount === 2) {
        return new Promise<DejavuRepositoryStatus>((resolve) => {
          resolveOlderReload = resolve;
        });
      }
      return new Promise<DejavuRepositoryStatus>((resolve) => {
        resolveNewerReload = resolve;
      });
    };
    configureAppRuntime(runtime);
    renderS3Settings();
    await screen.findByText("The current notebook is not bound to a Dejavu repository.");
    await waitFor(() => expect(events.listenerCount("qingyu://dejavu-sync-status-changed")).toBe(1));

    let olderEvent!: Promise<unknown>;
    let newerEvent!: Promise<unknown>;
    await act(async () => {
      olderEvent = events.emit("qingyu://dejavu-sync-status-changed", olderStatus);
      newerEvent = events.emit("qingyu://dejavu-sync-status-changed", newerStatus);
      await waitFor(() => expect(resolveNewerReload).toBeDefined());
      resolveNewerReload(newerStatus);
      await newerEvent;
    });

    const summary = await screen.findByRole("status", { name: "Dejavu background sync" });
    expect(summary).toHaveTextContent("Syncing");
    await act(async () => {
      resolveOlderReload(olderStatus);
      await olderEvent;
    });
    expect(summary).toHaveTextContent("Syncing");
    expect(summary).not.toHaveTextContent("stale-older-binding");
  });

  it("adopts a new repository binding for the same current root", async () => {
    const events = createEventBus();
    const oldStatus = repositoryStatus({
      repositoryId: "00000000-0000-4000-8000-0000000000aa"
    });
    const newStatus = repositoryStatus({
      repositoryId: "00000000-0000-4000-8000-0000000000bb"
    });
    let currentStatus = oldStatus;
    const onRepositoryIdentityChange = vi.fn();
    const runtime = createDefaultAppRuntime();
    runtime.events = events.events;
    runtime.syncConfig.loadRepositoryStatus = vi.fn(async () => currentStatus);
    configureAppRuntime(runtime);
    const s3Document = document({ config: { ...config, provider: "s3" } });
    render(<SyncSettings {...createProps({
      configDocument: s3Document,
      loadResult: { ...s3Document, status: "loaded" },
      onRepositoryIdentityChange
    })} />);

    await waitFor(() => expect(onRepositoryIdentityChange).toHaveBeenLastCalledWith({
      notesRoot: "/Notes",
      repositoryId: oldStatus.repositoryId
    }));
    currentStatus = newStatus;
    await act(async () => {
      await events.emit("qingyu://dejavu-sync-status-changed", newStatus);
    });

    await waitFor(() => expect(onRepositoryIdentityChange).toHaveBeenLastCalledWith({
      notesRoot: "/Notes",
      repositoryId: newStatus.repositoryId
    }));
  });

  it("renders only safe Dejavu diagnostics and relative conflict paths", async () => {
    const unsafePaths = [
      ["00000000-0000-4000-8000-0000000000e1", "/Users/alice/Private/credentials.md"],
      ["00000000-0000-4000-8000-0000000000e2", "C:Users\\alice\\secret.md"],
      ["00000000-0000-4000-8000-0000000000e3", "C:\\Users\\alice\\secret.md"],
      ["00000000-0000-4000-8000-0000000000e4", "\\\\server\\share\\secret.md"],
      ["00000000-0000-4000-8000-0000000000e5", "file:///Users/alice/secret.md"],
      ["00000000-0000-4000-8000-0000000000e6", "notes/\0secret.md"],
      ["00000000-0000-4000-8000-0000000000e7", "notes/./secret.md"],
      ["00000000-0000-4000-8000-0000000000e8", "notes/../secret.md"],
      ["00000000-0000-4000-8000-0000000000e9", "notes/name:stream"],
      ["00000000-0000-4000-8000-0000000000ea", "notes/trailing."],
      ["00000000-0000-4000-8000-0000000000eb", "notes/trailing "],
      ["00000000-0000-4000-8000-0000000000ec", "notes//secret.md"],
      ["00000000-0000-4000-8000-0000000000ed", "notes/\u0085secret.md"]
    ] as const;
    const status = repositoryStatus({
      conflicts: [
        conflict({ relativePath: "notes/safe.md" }),
        ...unsafePaths.map(([conflictId, relativePath]) => conflict({ conflictId, relativePath }))
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
    expect(summary).toHaveTextContent("Conflict history: 14");
    for (const [, path] of unsafePaths) {
      expect(summary.textContent).not.toContain(path);
    }
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
