import type {
  AppSyncConfigRuntime,
  KernelDomainPort,
  KernelRevision,
  KernelSyncConfigChangesInput,
  KernelSyncConfigSnapshot,
  QingYuSyncConfig,
  SyncConfigDocument,
  SyncConfigPatch,
  SyncStatus,
} from "../index";

import { kernelWorkspaceRoot } from "./files";

export function createKernelSyncConfigRuntime(
  kernel: KernelDomainPort,
  options: KernelSyncConfigRuntimeOptions,
): AppSyncConfigRuntime {
  return {
    bindRepository: () => unavailableSyncCapability("bindRepository"),
    cancelApply: options.local.cancelApply,
    changeGlobalKey: () => unavailableSyncCapability("changeGlobalKey"),
    deleteRemoteRepository: () => unavailableSyncCapability("deleteRemoteRepository"),
    enable: async ({ expectedRevision }) => {
      const revision = expectedRevision === null
        ? (await kernel.sync.readConfig()).revision
        : expectedRevision as KernelRevision;
      return mapConfig(await kernel.sync.patchConfig({
        changes: { enabled: true },
        expectedRevision: revision,
      }));
    },
    load: async () => ({
      ...mapConfig(await kernel.sync.readConfig()),
      status: "loaded",
    }),
    exportGlobalKey: () => unavailableSyncCapability("exportGlobalKey"),
    initializeGlobalKey: () => unavailableSyncCapability("initializeGlobalKey"),
    loadEditing: options.local.loadEditing,
    loadKeyState: async () => ({ configured: false }),
    listDejavuConflictHistory: async () => [],
    listNotebooks: () => unavailableSyncCapability("listNotebooks"),
    loadRepositoryStatus: async () => null,
    loadStatus: async () => mapStatus(await kernel.sync.readStatus()),
    patch: async ({ expectedRevision, patch }) => mapConfig(
      await kernel.sync.patchConfig({
        changes: mapPatch(patch),
        expectedRevision: expectedRevision as KernelRevision,
      }),
    ),
    purgeRemoteRepository: () => unavailableSyncCapability("purgeRemoteRepository"),
    recover: async ({ config, expectedRevision }) => mapConfig(
      await kernel.sync.patchConfig({
        changes: mapRecoveredConfig(config),
        expectedRevision: expectedRevision as KernelRevision,
      }),
    ),
    requestApply: options.local.requestApply,
    readDejavuConflictHistory: () => unavailableSyncCapability("readDejavuConflictHistory"),
    rebuildLocalRepository: () => unavailableSyncCapability("rebuildLocalRepository"),
    reset: () => unavailableSyncCapability("reset"),
    setEditing: options.local.setEditing,
    stopRepositorySync: () => unavailableSyncCapability("stopRepositorySync"),
    sync: async (input) => {
      const run = await kernel.sync.trigger(input.revision as KernelRevision);
      if (run.configRevision !== input.revision) {
        throw new KernelSyncRunError("protocol-mismatch");
      }
      const status = await waitForTerminalRun(kernel, run, options);
      if (status.completionState !== "succeeded" || status.summary === null) {
        throw new KernelSyncRunError(
          "run-failed",
          status.error?.code ?? "sync-run-failed",
        );
      }
      const notesRoot = "notesRoot" in input ? input.notesRoot : kernelWorkspaceRoot;
      const notebookName = "notebookName" in input
        ? input.notebookName
        : (await kernel.workspace.read()).displayName;
      return {
        result: {
          notebookName,
          notesRoot,
          provider: status.provider,
          revision: run.configRevision,
          summary: { ...status.summary },
          trigger: input.trigger,
        },
        status: "completed",
      };
    },
    testConnection: async ({ revision }) => {
      const result = await kernel.sync.testConnection({
        changes: {},
        expectedRevision: revision as KernelRevision,
      });
      return {
        checkedTarget: result.checkedTarget,
        provider: result.provider,
      };
    },
  };
}

export interface KernelSyncConfigRuntimeOptions {
  readonly local: Pick<
    AppSyncConfigRuntime,
    "cancelApply" | "loadEditing" | "requestApply" | "setEditing"
  >;
  readonly delay?: (milliseconds: number) => Promise<unknown>;
  readonly maxStatusReads?: number;
  readonly statusPollMilliseconds?: number;
}

function unavailableSyncCapability(name: string): Promise<never> {
  return Promise.reject(new Error(`${name} is unavailable for a Kernel runtime.`));
}

export type KernelSyncRunErrorCode = "protocol-mismatch" | "run-failed" | "timeout";

export class KernelSyncRunError extends Error {
  readonly code: KernelSyncRunErrorCode;
  readonly safeReason: string;

  constructor(code: KernelSyncRunErrorCode, safeReason: string = code) {
    super(`The Kernel sync run did not complete (${safeReason}).`);
    this.name = "KernelSyncRunError";
    this.code = code;
    this.safeReason = safeReason;
  }
}

async function waitForTerminalRun(
  kernel: KernelDomainPort,
  run: Awaited<ReturnType<KernelDomainPort["sync"]["trigger"]>>,
  options: KernelSyncConfigRuntimeOptions,
) {
  const delay = options.delay ?? ((milliseconds: number) => new Promise<undefined>((resolve) => {
    globalThis.setTimeout(() => resolve(undefined), milliseconds);
  }));
  const maxStatusReads = options.maxStatusReads;
  if (
    maxStatusReads !== undefined &&
    (!Number.isSafeInteger(maxStatusReads) || maxStatusReads < 1)
  ) {
    throw new KernelSyncRunError("protocol-mismatch");
  }

  for (let attempt = 0; maxStatusReads === undefined || attempt < maxStatusReads; attempt += 1) {
    const status = await kernel.sync.readStatus();
    if (
      status.configRevision !== run.configRevision ||
      status.lastAttemptAt !== run.acceptedAt
    ) {
      throw new KernelSyncRunError("protocol-mismatch");
    }
    if (status.completionState === "attempting") {
      if (status.activeRunId !== run.runId) {
        throw new KernelSyncRunError("protocol-mismatch");
      }
      await delay(options.statusPollMilliseconds ?? 250);
      continue;
    }
    if (status.activeRunId !== null || status.completionState === "idle") {
      throw new KernelSyncRunError("protocol-mismatch");
    }
    if (
      status.completionState === "failed" &&
      status.error?.runId !== undefined &&
      status.error.runId !== run.runId
    ) {
      throw new KernelSyncRunError("protocol-mismatch");
    }
    return status;
  }
  throw new KernelSyncRunError("timeout");
}

function mapConfig(config: KernelSyncConfigSnapshot): SyncConfigDocument {
  return {
    config: {
      enabled: config.enabled,
      generateConflictDocument: config.generateConflictDocument,
      intervalSeconds: config.intervalSeconds,
      mode: config.mode,
      provider: config.provider,
      remoteRoot: config.remoteRoot,
      s3: {
        accessKeyId: "",
        addressingStyle: config.s3.addressingStyle,
        bucket: config.s3.bucket,
        endpointUrl: config.s3.endpointUrl.value ?? "",
        region: config.s3.region,
        requestTimeoutSeconds: config.s3.requestTimeoutSeconds,
        secretAccessKey: "",
        tlsVerification: config.s3.tlsVerification,
      },
      version: 3,
      webdav: {
        password: "",
        serverUrl: config.webdav.serverUrl.value ?? "",
        username: config.webdav.username,
      },
    },
    configured: config.configured,
    issues: config.issues.map((issue) => ({
      code: issue.code,
      field: issue.field,
      message: issue.message,
    })),
    readiness: config.readiness,
    revision: config.revision,
  };
}

function mapPatch(patch: SyncConfigPatch): KernelSyncConfigChangesInput {
  switch (patch.field) {
    case "enabled": return { enabled: patch.value };
    case "provider": return { provider: patch.value };
    case "remoteRoot": return { remoteRoot: patch.value };
    case "mode": return { mode: patch.value };
    case "intervalSeconds": return { intervalSeconds: patch.value };
    case "generateConflictDocument": return { generateConflictDocument: patch.value };
    case "webdav.serverUrl": return { webdavServerUrl: patch.value };
    case "webdav.username": return { webdavUsername: patch.value };
    case "webdav.password": return {
      webdavPassword: credentialChange(patch.value),
    };
    case "s3.endpointUrl": return { s3EndpointUrl: patch.value };
    case "s3.region": return { s3Region: patch.value };
    case "s3.bucket": return { s3Bucket: patch.value };
    case "s3.accessKeyId": return {
      s3AccessKeyId: credentialChange(patch.value),
    };
    case "s3.secretAccessKey": return {
      s3SecretAccessKey: credentialChange(patch.value),
    };
    case "s3.requestTimeoutSeconds": return { s3RequestTimeoutSeconds: patch.value };
    case "s3.addressingStyle": return { s3AddressingStyle: patch.value };
    case "s3.tlsVerification": return { s3TlsVerification: patch.value };
  }
}

function credentialChange(value: string) {
  return value === ""
    ? { operation: "clear" as const }
    : { operation: "replace" as const, value };
}

function mapRecoveredConfig(config: QingYuSyncConfig): KernelSyncConfigChangesInput {
  return {
    enabled: config.enabled,
    generateConflictDocument: config.generateConflictDocument,
    intervalSeconds: config.intervalSeconds,
    mode: config.mode,
    provider: config.provider,
    remoteRoot: config.remoteRoot,
    s3AccessKeyId: credentialChange(config.s3.accessKeyId),
    s3AddressingStyle: config.s3.addressingStyle,
    s3Bucket: config.s3.bucket,
    s3EndpointUrl: config.s3.endpointUrl,
    s3Region: config.s3.region,
    s3RequestTimeoutSeconds: config.s3.requestTimeoutSeconds,
    s3SecretAccessKey: credentialChange(config.s3.secretAccessKey),
    s3TlsVerification: config.s3.tlsVerification,
    webdavPassword: credentialChange(config.webdav.password),
    webdavServerUrl: config.webdav.serverUrl,
    webdavUsername: config.webdav.username,
  };
}

function mapStatus(
  status: Awaited<ReturnType<KernelDomainPort["sync"]["readStatus"]>>,
): SyncStatus | null {
  if (
    status.completionState === "idle" ||
    status.lastAttemptAt === null ||
    status.lastTrigger === null
  ) {
    return null;
  }
  return {
    completionState: status.completionState,
    error: status.error === null ? null : {
      category: syncErrorCategory(status.error.category),
      code: status.error.code,
      httpStatus: status.error.httpStatus ?? null,
      method: status.error.method ?? null,
      objectId: null,
      operation: status.error.operation,
      provider: status.error.provider,
      providerErrorCode: status.error.providerErrorCode ?? null,
      relativePath: status.error.relativePath ?? null,
      requestId: status.error.requestId ?? null,
      runId: status.error.runId ?? null,
    },
    lastAttemptAt: status.lastAttemptAt,
    lastSuccessfulSyncAt: status.lastSuccessfulSyncAt,
    lastTrigger: status.lastTrigger,
    notebookName: null,
    notesRoot: kernelWorkspaceRoot,
    provider: status.provider,
    revision: status.configRevision,
    summary: status.summary === null ? null : { ...status.summary },
    version: 1,
  };
}

function syncErrorCategory(
  category: string | undefined,
): "http" | "integrity" | "local" | "transport" | null {
  if (
    category === "http" || category === "integrity" ||
    category === "local" || category === "transport"
  ) {
    return category;
  }
  return null;
}
