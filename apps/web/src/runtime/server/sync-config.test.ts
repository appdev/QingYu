import type {
  KernelDomainPort,
  KernelRevision,
  KernelSyncConfigSnapshot,
} from "@markra/app/runtime";

import { createServerSyncConfigRuntime } from "./sync-config";

const revision = "sync-revision-1" as KernelRevision;

describe("server sync config facade", () => {
  it("loads and patches the basic sync contract through Kernel", async () => {
    const kernel = kernelPort();
    const syncConfig = createServerSyncConfigRuntime(kernel, {
      delay: async () => undefined,
    });

    await expect(syncConfig.load()).resolves.toMatchObject({
      status: "loaded",
      configured: true,
      readiness: "ready",
      revision,
      config: {
        enabled: true,
        provider: "s3",
        s3: {
          accessKeyId: "",
          endpointUrl: "https://s3.example.test",
          secretAccessKey: "",
        },
      },
    });
    await syncConfig.patch({
      expectedRevision: revision,
      patch: { field: "enabled", value: false },
    });
    expect(kernel.sync.patchConfig).toHaveBeenCalledWith({
      changes: { enabled: false },
      expectedRevision: revision,
    });
  });

  it("tests and triggers the stored server configuration without client secrets", async () => {
    const kernel = kernelPort();
    const syncConfig = createServerSyncConfigRuntime(kernel);

    await expect(syncConfig.testConnection({ revision })).resolves.toEqual({
      checkedTarget: "bucket/notes",
      provider: "s3",
    });
    expect(kernel.sync.testConnection).toHaveBeenCalledWith({
      changes: {},
      expectedRevision: revision,
    });
    await expect(syncConfig.sync({
      notebookName: "Notes",
      notesRoot: "kernel-workspace://primary",
      revision,
      trigger: "manual",
    })).resolves.toEqual({
      result: {
        notebookName: "Notes",
        notesRoot: "kernel-workspace://primary",
        provider: "s3",
        revision,
        summary: {
          bytesDownloaded: 0,
          bytesUploaded: 1,
          conflictFiles: 0,
          downloadedFiles: 0,
          scannedFiles: 1,
          skippedFiles: 0,
          uploadedFiles: 1,
        },
        trigger: "manual",
      },
      status: "completed",
    });
    expect(kernel.sync.trigger).toHaveBeenCalledWith(revision);
  });

  it("lists and binds S3 notebooks through the Kernel repository contract", async () => {
    const kernel = kernelPort();
    const listNotebooks = vi.fn(async () => [{
      available: true,
      disabledReason: null,
      displayName: "Shared notes",
      name: "Shared notes",
      provider: "s3" as const,
      repositoryId: "323df833-764a-44b3-a534-492640c258f2",
    }]);
    const bindRepository = vi.fn(async () => ({
      jobId: "bind-1",
      notesRoot: "kernel-workspace://primary",
      repositoryId: "323df833-764a-44b3-a534-492640c258f2",
    }));
    Object.assign(kernel.sync, { bindRepository, listNotebooks });
    const syncConfig = createServerSyncConfigRuntime(kernel);

    await expect(syncConfig.listNotebooks({ revision })).resolves.toEqual([
      expect.objectContaining({
        displayName: "Shared notes",
        repositoryId: "323df833-764a-44b3-a534-492640c258f2",
      }),
    ]);
    await expect(syncConfig.bindRepository({
      displayName: "Shared notes",
      notesRoot: "kernel-workspace://primary",
      repositoryId: "323df833-764a-44b3-a534-492640c258f2",
      revision,
    })).resolves.toMatchObject({ jobId: "bind-1" });
    expect(listNotebooks).toHaveBeenCalledWith(revision);
    expect(bindRepository).toHaveBeenCalledWith({
      displayName: "Shared notes",
      expectedRevision: revision,
      repositoryId: "323df833-764a-44b3-a534-492640c258f2",
    });
  });

  it("reads the persisted active-workspace repository binding through the production facade", async () => {
    const kernel = kernelPort();
    const readRepositoryBinding = vi.fn(async () => ({
      repositoryId: "5223e8c9-1346-4d59-8c22-12d68ce16fcf",
    }));
    Object.assign(kernel.sync, { readRepositoryBinding });
    const syncConfig = createServerSyncConfigRuntime(kernel);

    await expect(syncConfig.loadRepositoryBinding({
      notesRoot: "kernel-workspace://primary",
    })).resolves.toEqual({
      repositoryId: "5223e8c9-1346-4d59-8c22-12d68ce16fcf",
    });
    expect(readRepositoryBinding).toHaveBeenCalledOnce();
  });

  it("correlates an attempting run by run id before accepting its terminal status", async () => {
    const kernel = kernelPort();
    vi.mocked(kernel.sync.readStatus).mockResolvedValueOnce({
      ...successfulStatus(),
      activeRunId: "run-1",
      completionState: "attempting",
      lastSuccessfulSyncAt: null,
      summary: null,
    });
    const delay = vi.fn(async () => undefined);
    const syncConfig = createServerSyncConfigRuntime(kernel, { delay });

    await expect(syncConfig.sync({
      notebookName: "Notes",
      notesRoot: "kernel-workspace://primary",
      revision,
      trigger: "manual",
    })).resolves.toMatchObject({ status: "completed" });

    expect(delay).toHaveBeenCalledWith(250);
    expect(kernel.sync.readStatus).toHaveBeenCalledTimes(2);
  });

  it("does not impose the container drain window as a production sync timeout", async () => {
    const kernel = kernelPort();
    let reads = 0;
    vi.mocked(kernel.sync.readStatus).mockImplementation(async () => {
      reads += 1;
      return reads <= 121
        ? {
            ...successfulStatus(),
            activeRunId: "run-1",
            completionState: "attempting",
            lastSuccessfulSyncAt: null,
            summary: null,
          }
        : successfulStatus();
    });
    const syncConfig = createServerSyncConfigRuntime(kernel, {
      delay: async () => undefined,
    });

    await expect(syncConfig.sync({
      notebookName: "Notes",
      notesRoot: "kernel-workspace://primary",
      revision,
      trigger: "manual",
    })).resolves.toMatchObject({ status: "completed" });
    expect(reads).toBe(122);
  });

  it("settles a settings apply in the Web host before accepting a new token", async () => {
    const syncConfig = createServerSyncConfigRuntime(kernelPort());
    await syncConfig.setEditing({
      active: true,
      revision,
      sessionId: "session-1",
    });
    await syncConfig.requestApply({
      exitReason: "window-close",
      revision,
      sessionId: "session-1",
      source: "settings-exit",
      token: "apply-1",
    });

    await syncConfig.sync({
      applyToken: "apply-1",
      notebookName: "Notes",
      notesRoot: "kernel-workspace://primary",
      revision,
      trigger: "settings-exit",
    });

    await expect(syncConfig.loadEditing()).resolves.toMatchObject({
      pendingApply: { state: "completed", token: "apply-1" },
    });
    await syncConfig.setEditing({
      active: true,
      revision,
      sessionId: "session-2",
    });
    await expect(syncConfig.requestApply({
      exitReason: "category-leave",
      revision,
      sessionId: "session-2",
      source: "settings-exit",
      token: "apply-2",
    })).resolves.toMatchObject({
      event: { state: "pending", token: "apply-2" },
    });
  });
});

function config(enabled = true): KernelSyncConfigSnapshot {
  return {
    configured: true,
    enabled,
    generateConflictDocument: true,
    intervalSeconds: 60,
    issues: [],
    mode: "automatic",
    provider: "s3",
    readiness: "ready",
    remoteRoot: "notes",
    revision,
    s3: {
      accessKeyId: { present: true },
      addressingStyle: "auto",
      bucket: "bucket",
      endpointUrl: { redacted: false, value: "https://s3.example.test" },
      region: "us-east-1",
      requestTimeoutSeconds: 30,
      secretAccessKey: { present: true },
      tlsVerification: "verify",
    },
    webdav: {
      password: { present: false },
      serverUrl: { redacted: false, value: null },
      username: "",
    },
  };
}

function kernelPort() {
  const unavailable = () => vi.fn(async () => {
    throw new Error("not used");
  });
  const readConfig = vi.fn(async () => config());
  return {
    availability: "available",
    documents: {
      create: unavailable(), delete: unavailable(),
      history: { list: unavailable(), restore: unavailable() },
      list: unavailable(), move: unavailable(), read: unavailable(),
      search: unavailable(), update: unavailable(),
    },
    runtime: { read: unavailable() },
    settings: { patch: unavailable(), read: unavailable() },
    sync: {
      patchConfig: vi.fn(async (input) => config(input.changes.enabled ?? true)),
      readConfig,
      readStatus: vi.fn(async () => successfulStatus()),
      testConnection: vi.fn(async () => ({
        checkedTarget: "bucket/notes",
        configRevision: revision,
        provider: "s3" as const,
      })),
      trigger: vi.fn(async () => ({
        acceptedAt: "2026-07-30T00:00:00Z",
        configRevision: revision,
        runId: "run-1",
      })),
    },
    workspace: { read: unavailable() },
  } as unknown as KernelDomainPort;
}

function successfulStatus() {
  return {
    activeRunId: null,
    completionState: "succeeded" as const,
    configRevision: revision,
    error: null,
    lastAttemptAt: "2026-07-30T00:00:00Z",
    lastSuccessfulSyncAt: "2026-07-30T00:00:01Z",
    lastTrigger: "manual" as const,
    provider: "s3" as const,
    summary: {
      bytesDownloaded: 0,
      bytesUploaded: 1,
      conflictFiles: 0,
      downloadedFiles: 0,
      scannedFiles: 1,
      skippedFiles: 0,
      uploadedFiles: 1,
    },
  };
}
