import type {
  AppSyncConfigRuntime,
  KernelDomainPort,
  KernelRevision,
  KernelSyncConfigChangesInput,
  KernelSyncConfigSnapshot,
  QingYuSyncConfig,
  SyncApplySettlementInput,
  SyncConfigDocument,
  SyncConfigPatch,
  SyncDispatchResult,
  SyncSafeError,
  SyncStatus,
} from "../index";

import { kernelWorkspaceRoot } from "./files";

export function createKernelSyncConfigRuntime(
  kernel: KernelDomainPort,
  options: KernelSyncConfigRuntimeOptions,
): AppSyncConfigRuntime {
  return {
    bindRepository: async (input) => {
      if (input.notesRoot !== kernelWorkspaceRoot) {
        throw new Error("The repository binding does not address the active Kernel workspace.");
      }
      const binding = await kernel.sync.bindRepository({
        displayName: input.displayName,
        expectedRevision: input.revision as KernelRevision,
        repositoryId: input.repositoryId,
      });
      return { ...binding, notesRoot: kernelWorkspaceRoot };
    },
    cancelApply: options.local.cancelApply,
    changeGlobalKey: async (input) => {
      await kernel.sync.importKey(input.newKey);
      return {
        jobId: "kernel-key-import-completed",
        operation: "change-global-key",
        repositoryId: null,
      };
    },
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
    loadJob: async ({ jobId }) => mapJob(await kernel.sync.readRun(jobId)),
    exportGlobalKey: () => kernel.sync.exportKey(),
    initializeGlobalKey: (input) => kernel.sync.importKey(input.key),
    loadEditing: options.local.loadEditing,
    loadKeyState: () => kernel.sync.readKeyState(),
    listDejavuConflictHistory: async () => [],
    listNotebooks: (input) => kernel.sync.listNotebooks(input.revision as KernelRevision),
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
    settleApply: options.local.settleApply,
    stopRepositorySync: () => unavailableSyncCapability("stopRepositorySync"),
    sync: async (input) => {
      const apply = "applyToken" in input && input.applyToken ? {
        revision: input.revision,
        token: input.applyToken,
      } : null;
      let dispatch: Extract<SyncDispatchResult, { status: "completed" }>;
      try {
        const run = await kernel.sync.trigger(input.revision as KernelRevision);
        if (run.configRevision !== input.revision) {
          throw new KernelSyncRunError("protocol-mismatch");
        }
        const status = await waitForTerminalRun(kernel, run, options);
        if (status.completionState !== "succeeded" || status.summary === null) {
          const safeError = status.error === null ? null : mapSyncSafeError(status.error);
          throw new KernelSyncRunError(
            "run-failed",
            safeError?.code ?? "sync-run-failed",
            { runError: safeError },
          );
        }
        const notesRoot = "notesRoot" in input ? input.notesRoot : kernelWorkspaceRoot;
        const notebookName = "notebookName" in input
          ? input.notebookName
          : (await kernel.workspace.read()).displayName;
        dispatch = {
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
      } catch (error) {
        if (apply) {
          await settleKernelSyncApply(
            options,
            { ...apply, outcome: { status: "failed" } },
            error,
          );
        }
        throw error;
      }
      if (apply) {
        await settleKernelSyncApply(options, {
          ...apply,
          outcome: dispatch,
        });
      }
      return dispatch;
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
    "cancelApply" | "loadEditing" | "requestApply" | "setEditing" | "settleApply"
  >;
  readonly delay?: (milliseconds: number) => Promise<unknown>;
  readonly maxStatusReads?: number;
  readonly statusPollMilliseconds?: number;
}

function unavailableSyncCapability(name: string): Promise<never> {
  return Promise.reject(new Error(`${name} is unavailable for a Kernel runtime.`));
}

export type KernelSyncRunErrorCode =
  | "apply-settlement-failed"
  | "protocol-mismatch"
  | "run-failed"
  | "timeout";

export class KernelSyncRunError extends Error {
  readonly code: KernelSyncRunErrorCode;
  readonly runError: unknown;
  readonly safeReason: string;
  readonly settlementError: unknown;

  constructor(
    code: KernelSyncRunErrorCode,
    safeReason: string = code,
    details: { readonly runError?: unknown; readonly settlementError?: unknown } = {},
  ) {
    super(`The Kernel sync run did not complete (${safeReason}).`);
    this.name = "KernelSyncRunError";
    this.code = code;
    this.runError = details.runError ?? null;
    this.safeReason = safeReason;
    this.settlementError = details.settlementError ?? null;
  }
}

async function settleKernelSyncApply(
  options: KernelSyncConfigRuntimeOptions,
  input: SyncApplySettlementInput,
  runError: unknown = null,
) {
  try {
    await options.local.settleApply(input);
  } catch (settlementError) {
    throw new KernelSyncRunError(
      "apply-settlement-failed",
      "sync-apply-settlement-failed",
      { runError: terminalRunError(runError), settlementError },
    );
  }
}

function terminalRunError(runError: unknown): unknown {
  return runError instanceof KernelSyncRunError ? runError.runError : runError;
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
    error: status.error === null ? null : mapSyncSafeError(status.error),
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

function mapJob(
  status: Awaited<ReturnType<KernelDomainPort["sync"]["readRun"]>>,
) {
  return {
    acceptedAt: status.acceptedAt,
    completionState: status.completionState,
    error: status.error === null ? null : mapSyncSafeError(status.error),
    finishedAt: status.finishedAt,
    jobId: status.runId,
    provider: status.provider,
    revision: status.configRevision,
    summary: status.summary === null ? null : { ...status.summary },
  };
}

function mapSyncSafeError(
  error: NonNullable<Awaited<ReturnType<KernelDomainPort["sync"]["readStatus"]>>["error"]>,
): SyncSafeError {
  return {
    category: syncErrorCategory(error.category),
    code: error.code,
    httpStatus: error.httpStatus ?? null,
    method: error.method ?? null,
    objectId: null,
    operation: error.operation,
    provider: error.provider,
    providerErrorCode: error.providerErrorCode ?? null,
    relativePath: error.relativePath ?? null,
    requestId: error.requestId ?? null,
    runId: error.runId ?? null,
  };
}

function syncErrorCategory(
  category: string | undefined,
): SyncSafeError["category"] {
  if (
    category === "authentication" || category === "authorization" ||
    category === "configuration" || category === "conflict" ||
    category === "http" || category === "integrity" || category === "local" ||
    category === "network" || category === "provider" || category === "storage" ||
    category === "transport"
  ) {
    return category;
  }
  return null;
}
