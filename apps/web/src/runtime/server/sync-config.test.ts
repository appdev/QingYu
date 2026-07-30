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
    const syncConfig = createServerSyncConfigRuntime(kernel);

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
      job: {
        jobId: "run-1",
        notesRoot: "kernel-workspace://primary",
        repositoryId: "server-workspace",
      },
      status: "accepted",
    });
    expect(kernel.sync.trigger).toHaveBeenCalledWith(revision);
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
      readStatus: unavailable(),
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
