import {
  createDefaultAppRuntime,
  type AppSyncConfigRuntime,
  type KernelDomainPort,
  type KernelRevision,
  type KernelSyncConfigChangesInput,
  type KernelSyncConfigSnapshot,
  type QingYuSyncConfig,
  type SyncConfigDocument,
  type SyncConfigPatch,
  type SyncStatus,
} from "@markra/app/runtime";

import { serverWorkspaceRoot } from "./files";

export function createServerSyncConfigRuntime(
  kernel: KernelDomainPort,
): AppSyncConfigRuntime {
  const fallback = createDefaultAppRuntime().syncConfig;
  return {
    ...fallback,
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
    loadStatus: async () => mapStatus(await kernel.sync.readStatus()),
    patch: async ({ expectedRevision, patch }) => mapConfig(
      await kernel.sync.patchConfig({
        changes: mapPatch(patch),
        expectedRevision: expectedRevision as KernelRevision,
      }),
    ),
    recover: async ({ config, expectedRevision }) => mapConfig(
      await kernel.sync.patchConfig({
        changes: mapRecoveredConfig(config),
        expectedRevision: expectedRevision as KernelRevision,
      }),
    ),
    sync: async (input) => {
      const run = await kernel.sync.trigger(input.revision as KernelRevision);
      return {
        job: {
          jobId: run.runId,
          notesRoot: "notesRoot" in input ? input.notesRoot : serverWorkspaceRoot,
          repositoryId: "server-workspace",
        },
        status: "accepted",
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
    notesRoot: serverWorkspaceRoot,
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
