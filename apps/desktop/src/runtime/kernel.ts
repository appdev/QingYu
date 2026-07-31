import type {
  KernelCreatedDocumentSnapshot,
  KernelDocumentEntrySnapshot,
  KernelDocumentLocator,
  KernelDocumentPageSnapshot,
  KernelDocumentSnapshot,
  KernelDomainPort,
  KernelHistoryPageSnapshot,
  KernelHistorySnapshot,
  KernelHistorySnapshotId,
  KernelInventoryEntry,
  KernelInvalidationSource,
  KernelPageCursor,
  KernelRevision,
  KernelRuntimeCapabilities,
  KernelRuntimeSnapshot,
  KernelSearchPageSnapshot,
  KernelSettingValue,
  KernelSettingsSnapshot,
  KernelSyncConfigSnapshot,
  KernelSyncConnectionTestSnapshot,
  KernelSyncRunSnapshot,
  KernelSyncStatusSnapshot,
  KernelWorkspaceGeneration,
  KernelWorkspaceRelativePath,
  KernelWorkspaceSnapshot,
} from "@markra/app/runtime";
import { hasRequiredKernelDomainCapabilities } from "@markra/app/runtime";
import type {
  FetchLike,
  KernelClient,
  NativeBearerAuthentication,
} from "@markra/kernel-client";
import { createKernelClient } from "@markra/kernel-client";

interface DesktopKernelConnectionBase {
  readonly authentication: NativeBearerAuthentication;
  readonly baseUrl: string;
  readonly instanceId: string;
  readonly release?: () => unknown;
}

export type DesktopKernelConnection = DesktopKernelConnectionBase & (
  | {
      readonly generation: string;
      readonly processGeneration?: never;
    }
  | {
      readonly generation?: never;
      readonly processGeneration: string;
    }
);

export interface DesktopKernelDomainAdapter {
  readonly port: KernelDomainPort;
  readonly release: () => undefined;
}

export interface DesktopKernelDomainAdapterOptions {
  readonly fetch?: FetchLike;
  readonly invalidations?: KernelInvalidationSource;
  readonly profile?: "desktop" | "mobile";
}

export type DesktopKernelDomainAdapterErrorCode =
  | "initialization-failed"
  | "protocol-mismatch"
  | "released"
  | "workspace-generation-mismatch";

const ERROR_MESSAGES: Record<DesktopKernelDomainAdapterErrorCode, string> = {
  "initialization-failed": "The desktop Kernel adapter could not be initialized.",
  "protocol-mismatch": "The desktop Kernel no longer matches its bootstrap contract.",
  released: "The desktop Kernel adapter has been released.",
  "workspace-generation-mismatch": "The document belongs to a different workspace generation.",
};

const unavailableInvalidations: KernelInvalidationSource = Object.freeze({
  available: false,
  subscribe: () => () => undefined,
});

export class DesktopKernelDomainAdapterError extends Error {
  readonly code: DesktopKernelDomainAdapterErrorCode;

  constructor(code: DesktopKernelDomainAdapterErrorCode) {
    super(ERROR_MESSAGES[code]);
    this.name = "DesktopKernelDomainAdapterError";
    this.code = code;
  }
}

export async function createDesktopKernelDomainAdapter(
  connection: DesktopKernelConnection,
  options: DesktopKernelDomainAdapterOptions = {},
): Promise<DesktopKernelDomainAdapter> {
  const { baseUrl, instanceId, release } = connection;
  const invalidations = options.invalidations ?? unavailableInvalidations;
  const profile = options.profile ?? "desktop";
  const processGeneration = connection.processGeneration ?? connection.generation;
  let authentication: NativeBearerAuthentication | undefined = connection.authentication;
  let lifecycle: "initializing" | "active" | "closed" = "initializing";
  let ownershipReleased = false;
  const requests = new AbortController();
  const releaseOwnership = () => {
    if (ownershipReleased) return undefined;
    ownershipReleased = true;
    authentication = undefined;
    try {
      release?.();
    } catch {
      // Releasing credentials is best-effort and must not expose provider errors.
    }
    return undefined;
  };
  const close = () => {
    if (lifecycle === "closed") return undefined;
    lifecycle = "closed";
    requests.abort();
    releaseOwnership();
    return undefined;
  };
  const assertActive = () => {
    if (lifecycle !== "active") {
      throw new DesktopKernelDomainAdapterError("released");
    }
  };
  const protocolMismatch = (): never => {
    close();
    throw new DesktopKernelDomainAdapterError("protocol-mismatch");
  };

  try {
    if (!isCanonicalProcessGeneration(processGeneration)) {
      throw new DesktopKernelDomainAdapterError("initialization-failed");
    }
    const client = createKernelClient({
      auth: {
        kind: "native-bearer",
        getCredential: () => {
          if (lifecycle === "closed" || authentication === undefined) {
            throw new Error("credential unavailable");
          }
          return authentication.getCredential();
        },
      },
      baseUrl,
      fetch: options.fetch ?? globalThis.fetch.bind(globalThis),
    });

    const ready = await client.system.ready({ signal: requests.signal });
    if (ready.instanceId !== instanceId) {
      throw new DesktopKernelDomainAdapterError("initialization-failed");
    }
    const runtime = await client.system.runtime({ signal: requests.signal });
    if (!matchesKernelRuntime(runtime, instanceId, profile)) {
      throw new DesktopKernelDomainAdapterError("initialization-failed");
    }
    const workspace = await client.workspace.get({ signal: requests.signal });
    if (!matchesReadyWorkspace(workspace)) {
      throw new DesktopKernelDomainAdapterError("initialization-failed");
    }
    const workspaceGeneration = workspace.generation as KernelWorkspaceGeneration;
    const workspaceId = workspace.id;

    lifecycle = "active";
    const assertWorkspaceGeneration = (candidate: KernelWorkspaceGeneration) => {
      if (candidate !== workspaceGeneration) {
        throw new DesktopKernelDomainAdapterError("workspace-generation-mismatch");
      }
    };
    const assertDocumentIdentity = (
      actual: string,
      expected: KernelDocumentLocator,
    ) => {
      if (actual !== expected) protocolMismatch();
    };
    const confirmWorkspaceIdentity = async () => {
      const current = await client.workspace.get({ signal: requests.signal });
      assertActive();
      if (!matchesDesktopWorkspace(current, workspaceId, workspaceGeneration)) {
        protocolMismatch();
      }
    };
    const prepareDocumentOperation = async (candidate: KernelWorkspaceGeneration) => {
      assertActive();
      assertWorkspaceGeneration(candidate);
      await confirmWorkspaceIdentity();
    };
    const prepareInstanceOperation = async () => {
      assertActive();
      await confirmWorkspaceIdentity();
    };
    const port: KernelDomainPort = {
      availability: "available",
      invalidations,
      documents: {
        create: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const document = await client.documents.create(
            input.kind === "file"
              ? {
                  contents: input.contents,
                  kind: input.kind,
                  name: input.name,
                  parent: input.parent,
                  workspaceGeneration: input.workspaceGeneration,
                }
              : {
                  kind: input.kind,
                  name: input.name,
                  parent: input.parent,
                  workspaceGeneration: input.workspaceGeneration,
                },
            { signal: requests.signal },
          );
          assertActive();
          if (
            document.kind !== input.kind ||
            document.name !== input.name ||
            document.parent !== input.parent ||
            document.path !== joinWorkspacePath(input.parent, input.name)
          ) {
            protocolMismatch();
          }
          await confirmWorkspaceIdentity();
          return mapCreatedDocument(document, input.workspaceGeneration);
        },
        delete: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const deleted = await client.documents.delete(
            input.locator,
            {
              deletionPolicy: input.deletionPolicy,
              expectedRevision: input.expectedRevision,
              workspaceGeneration: input.workspaceGeneration,
            },
            { signal: requests.signal },
          );
          assertActive();
          await confirmWorkspaceIdentity();
          return deleted;
        },
        history: {
          list: async (input) => {
            await prepareDocumentOperation(input.workspaceGeneration);
            const page = await client.documents.listHistory(
              input.locator,
              { cursor: input.cursor, limit: input.limit },
              { signal: requests.signal },
            );
            assertActive();
            for (const entry of page.items) {
              assertDocumentIdentity(entry.documentId, input.locator);
            }
            await confirmWorkspaceIdentity();
            return mapHistoryPage(page, input.workspaceGeneration);
          },
          read: async (input) => {
            await prepareDocumentOperation(input.workspaceGeneration);
            const history = await client.documents.getHistory(
              input.locator,
              input.snapshotId,
              { signal: requests.signal },
            );
            assertActive();
            assertDocumentIdentity(history.documentId, input.locator);
            if (history.snapshotId !== input.snapshotId) protocolMismatch();
            await confirmWorkspaceIdentity();
            return {
              contents: history.contents,
              documentLocator: history.documentId as KernelDocumentLocator,
              revision: history.revision as KernelRevision,
              snapshotId: history.snapshotId as KernelHistorySnapshotId,
              workspaceGeneration: input.workspaceGeneration,
            } satisfies KernelHistorySnapshot;
          },
          restore: async (input) => {
            await prepareDocumentOperation(input.workspaceGeneration);
            const document = await client.documents.restoreHistory(
              input.locator,
              input.snapshotId,
              {
                expectedRevision: input.expectedRevision,
                workspaceGeneration: input.workspaceGeneration,
              },
              { signal: requests.signal },
            );
            assertActive();
            assertDocumentIdentity(document.id, input.locator);
            await confirmWorkspaceIdentity();
            return mapDocument(document, input.workspaceGeneration);
          },
        },
        list: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const page = await client.documents.list(
            { cursor: input.cursor, limit: input.limit, parent: input.parent },
            { signal: requests.signal },
          );
          assertActive();
          await confirmWorkspaceIdentity();
          return mapDocumentPage(page, input.workspaceGeneration);
        },
        move: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const document = await client.documents.move(
            input.locator,
            {
              expectedRevision: input.expectedRevision,
              name: input.name,
              targetParent: input.targetParent,
              workspaceGeneration: input.workspaceGeneration,
            },
            { signal: requests.signal },
          );
          assertActive();
          // Document locators are signed for their relative path. A successful
          // move therefore returns a newly issued locator for the target path.
          if (
            document.name !== input.name ||
            document.parent !== input.targetParent ||
            document.path !== joinWorkspacePath(input.targetParent, input.name)
          ) {
            protocolMismatch();
          }
          await confirmWorkspaceIdentity();
          return mapDocumentEntry(document, input.workspaceGeneration);
        },
        read: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const document = await client.documents.get(input.locator, {
            signal: requests.signal,
          });
          assertActive();
          assertDocumentIdentity(document.id, input.locator);
          await confirmWorkspaceIdentity();
          return mapDocument(document, input.workspaceGeneration);
        },
        search: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const page = await client.workspace.search(
            {
              cursor: input.cursor,
              limit: input.limit,
              query: input.query,
            },
            { signal: requests.signal },
          );
          assertActive();
          await confirmWorkspaceIdentity();
          return mapSearchPage(page, input.workspaceGeneration);
        },
        update: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const document = await client.documents.update(
            input.locator,
            {
              contents: input.contents,
              expectedRevision: input.expectedRevision,
              workspaceGeneration: input.workspaceGeneration,
            },
            { signal: requests.signal },
          );
          assertActive();
          assertDocumentIdentity(document.id, input.locator);
          await confirmWorkspaceIdentity();
          return mapDocument(document, input.workspaceGeneration);
        },
      },
      resources: {
        list: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const items: KernelInventoryEntry[] = [];
          const seenCursors = new Set<string>();
          let cursor: string | undefined;
          do {
            const page = await client.resources.list({
              cursor,
              limit: 100,
              parent: input.parent,
            }, { signal: requests.signal });
            assertActive();
            items.push(...page.items.map((entry) => mapInventoryEntry(
              entry,
              input.workspaceGeneration,
            )));
            const nextCursor = page.nextCursor ?? undefined;
            if (nextCursor !== undefined && seenCursors.has(nextCursor)) protocolMismatch();
            if (nextCursor !== undefined) seenCursors.add(nextCursor);
            cursor = nextCursor;
          } while (cursor !== undefined);
          await confirmWorkspaceIdentity();
          return { items, workspaceGeneration: input.workspaceGeneration };
        },
        open: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const response = await client.resources.open(
            input.id,
            input.kind,
            { signal: requests.signal },
          );
          assertActive();
          const body = await response.blob();
          assertActive();
          await confirmWorkspaceIdentity();
          return {
            body,
            mediaType: response.headers.get("content-type")?.split(";", 1)[0]?.trim() ?? "",
          };
        },
      },
      runtime: {
        read: async () => {
          assertActive();
          const current = await client.system.runtime({ signal: requests.signal });
          assertActive();
          if (!matchesKernelRuntime(current, instanceId, profile)) {
            return protocolMismatch();
          }
          return mapRuntime(current);
        },
      },
      settings: {
        patch: async (input) => {
          await prepareInstanceOperation();
          const settings = await client.settings.patch(mapSettingsPatchRequest(input), {
            signal: requests.signal,
          });
          assertActive();
          await confirmWorkspaceIdentity();
          return mapSettings(settings);
        },
        read: async () => {
          await prepareInstanceOperation();
          const settings = await client.settings.get({ signal: requests.signal });
          assertActive();
          await confirmWorkspaceIdentity();
          return mapSettings(settings);
        },
      },
      sync: {
        patchConfig: async (input) => {
          await prepareInstanceOperation();
          const config = await client.sync.patchConfig(mapSyncPatchRequest(input), {
            signal: requests.signal,
          });
          assertActive();
          await confirmWorkspaceIdentity();
          return mapSyncConfig(config);
        },
        readConfig: async () => {
          await prepareInstanceOperation();
          const config = await client.sync.getConfig({ signal: requests.signal });
          assertActive();
          await confirmWorkspaceIdentity();
          return mapSyncConfig(config);
        },
        readStatus: async () => {
          await prepareInstanceOperation();
          const status = await client.sync.getStatus({ signal: requests.signal });
          assertActive();
          await confirmWorkspaceIdentity();
          return mapSyncStatus(status);
        },
        testConnection: async (input) => {
          await prepareInstanceOperation();
          const result = await client.sync.testConnection(mapSyncTestRequest(input), {
            signal: requests.signal,
          });
          assertActive();
          await confirmWorkspaceIdentity();
          return mapSyncConnectionTest(result);
        },
        trigger: async (expectedConfigRevision) => {
          await prepareInstanceOperation();
          const run = await client.sync.trigger(
            { expectedConfigRevision },
            { signal: requests.signal },
          );
          assertActive();
          await confirmWorkspaceIdentity();
          return mapSyncRun(run);
        },
      },
      workspace: {
        read: async () => {
          assertActive();
          const current = await client.workspace.get({ signal: requests.signal });
          assertActive();
          if (!matchesDesktopWorkspace(current, workspaceId, workspaceGeneration)) {
            return protocolMismatch();
          }
          return mapWorkspace(current);
        },
      },
    };

    return {
      port,
      release: close,
    };
  } catch {
    close();
    throw new DesktopKernelDomainAdapterError("initialization-failed");
  }
}

type RuntimeSource = Awaited<ReturnType<KernelClient["system"]["runtime"]>>;
type InventoryEntrySource = Awaited<ReturnType<KernelClient["resources"]["list"]>>["items"][number];
type WorkspaceSource = Awaited<ReturnType<KernelClient["workspace"]["get"]>>;
type DocumentSource = Awaited<ReturnType<KernelClient["documents"]["get"]>>;
type CreatedDocumentSource = Awaited<ReturnType<KernelClient["documents"]["create"]>>;
type DocumentEntrySource = Awaited<ReturnType<KernelClient["documents"]["move"]>>;
type DocumentPageSource = Awaited<ReturnType<KernelClient["documents"]["list"]>>;
type HistoryPageSource = Awaited<ReturnType<KernelClient["documents"]["listHistory"]>>;
type SearchPageSource = Awaited<ReturnType<KernelClient["workspace"]["search"]>>;
type SettingsSource = Awaited<ReturnType<KernelClient["settings"]["get"]>>;
type SyncConfigSource = Awaited<ReturnType<KernelClient["sync"]["getConfig"]>>;
type SyncConnectionTestSource = Awaited<ReturnType<KernelClient["sync"]["testConnection"]>>;
type SyncRunSource = Awaited<ReturnType<KernelClient["sync"]["trigger"]>>;
type SyncStatusSource = Awaited<ReturnType<KernelClient["sync"]["getStatus"]>>;
type SettingsPatchInput = Parameters<KernelDomainPort["settings"]["patch"]>[0];
type SettingsPatchRequest = Parameters<KernelClient["settings"]["patch"]>[0];
type SyncPatchInput = Parameters<KernelDomainPort["sync"]["patchConfig"]>[0];
type SyncPatchRequest = Parameters<KernelClient["sync"]["patchConfig"]>[0];
type SyncTestInput = Parameters<KernelDomainPort["sync"]["testConnection"]>[0];
type SyncTestRequest = Parameters<KernelClient["sync"]["testConnection"]>[0];
type SyncChangesInput = SyncPatchInput["changes"];
type SyncChangesRequest = SyncPatchRequest["changes"];

function matchesKernelRuntime(
  runtime: RuntimeSource,
  instanceId: string,
  profile: "desktop" | "mobile",
) {
  return (
    runtime.instanceId === instanceId &&
    runtime.profile === profile &&
    runtime.startupState === "ready" &&
    hasRequiredKernelDomainCapabilities(runtime.capabilities)
  );
}

function matchesReadyWorkspace(workspace: WorkspaceSource) {
  return workspace.readiness === "ready" && workspace.generation.length > 0;
}

function matchesDesktopWorkspace(
  workspace: WorkspaceSource,
  workspaceId: string,
  generation: KernelWorkspaceGeneration,
) {
  return (
    matchesReadyWorkspace(workspace) &&
    workspace.id === workspaceId &&
    workspace.generation === generation
  );
}

function isCanonicalProcessGeneration(value: string) {
  if (!/^(?:0|[1-9][0-9]*)$/u.test(value) || value.length > 20) return false;
  return BigInt(value) <= BigInt("18446744073709551615");
}

function mapCapabilities(
  capabilities: RuntimeSource["capabilities"],
): KernelRuntimeCapabilities {
  return {
    documents: capabilities.documents,
    history: capabilities.history,
    portableSettings: capabilities.portableSettings,
    resources: capabilities.resources,
    s3: capabilities.s3,
    search: capabilities.search,
    settings: capabilities.settings,
    sync: capabilities.sync,
    webdav: capabilities.webdav,
  };
}

function mapRuntime(runtime: RuntimeSource): KernelRuntimeSnapshot {
  return {
    capabilities: mapCapabilities(runtime.capabilities),
    instanceId: runtime.instanceId,
    profile: runtime.profile,
    startupState: runtime.startupState,
  };
}

function mapWorkspace(workspace: WorkspaceSource): KernelWorkspaceSnapshot {
  return {
    displayName: workspace.displayName,
    generation: workspace.generation as KernelWorkspaceGeneration,
    id: workspace.id,
    readiness: workspace.readiness,
    revision: workspace.revision as KernelRevision,
  };
}

function mapInventoryEntry(
  entry: InventoryEntrySource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelInventoryEntry {
  if (entry.entryType === "document") {
    return {
      document: mapDocumentEntry(entry.document, workspaceGeneration),
      entryType: entry.entryType,
    };
  }
  return {
    entryType: entry.entryType,
    resource: {
      id: entry.resource.id,
      kind: entry.resource.kind,
      mediaType: entry.resource.mediaType,
      modifiedAt: entry.resource.modifiedAt,
      name: entry.resource.name,
      parent: entry.resource.parent as KernelWorkspaceRelativePath,
      previewable: entry.resource.previewable,
      relativePath: entry.resource.path as KernelWorkspaceRelativePath,
      revision: entry.resource.revision as KernelRevision,
      sizeBytes: entry.resource.sizeBytes,
      workspaceGeneration,
    },
  };
}

function mapSettings(settings: SettingsSource): KernelSettingsSnapshot {
  return {
    revision: settings.revision as KernelRevision,
    values: settings.values.map((entry) => ({
      key: entry.key,
      value: mapSettingValue(entry.value),
    })),
  };
}

function mapSettingsPatchRequest(input: SettingsPatchInput): SettingsPatchRequest {
  return {
    expectedRevision: input.expectedRevision,
    values: input.values.map((entry) => ({
      key: entry.key,
      value: mapSettingValue(entry.value),
    })),
  };
}

function mapSyncPatchRequest(input: SyncPatchInput): SyncPatchRequest {
  return {
    changes: mapSyncChanges(input.changes),
    expectedRevision: input.expectedRevision,
  };
}

function mapSyncTestRequest(input: SyncTestInput): SyncTestRequest {
  return {
    changes: mapSyncChanges(input.changes),
    expectedRevision: input.expectedRevision,
  };
}

function mapSyncChanges(input: SyncChangesInput): SyncChangesRequest {
  const changes: SyncChangesRequest = {};
  if (input.enabled !== undefined) changes.enabled = input.enabled;
  if (input.generateConflictDocument !== undefined) {
    changes.generateConflictDocument = input.generateConflictDocument;
  }
  if (input.intervalSeconds !== undefined) changes.intervalSeconds = input.intervalSeconds;
  if (input.mode !== undefined) changes.mode = input.mode;
  if (input.provider !== undefined) changes.provider = input.provider;
  if (input.remoteRoot !== undefined) changes.remoteRoot = input.remoteRoot;
  if (input.s3AccessKeyId !== undefined) {
    changes.s3AccessKeyId = mapCredentialChange(input.s3AccessKeyId);
  }
  if (input.s3AddressingStyle !== undefined) {
    changes.s3AddressingStyle = input.s3AddressingStyle;
  }
  if (input.s3Bucket !== undefined) changes.s3Bucket = input.s3Bucket;
  if (input.s3EndpointUrl !== undefined) changes.s3EndpointUrl = input.s3EndpointUrl;
  if (input.s3Region !== undefined) changes.s3Region = input.s3Region;
  if (input.s3RequestTimeoutSeconds !== undefined) {
    changes.s3RequestTimeoutSeconds = input.s3RequestTimeoutSeconds;
  }
  if (input.s3SecretAccessKey !== undefined) {
    changes.s3SecretAccessKey = mapCredentialChange(input.s3SecretAccessKey);
  }
  if (input.s3TlsVerification !== undefined) {
    changes.s3TlsVerification = input.s3TlsVerification;
  }
  if (input.webdavPassword !== undefined) {
    changes.webdavPassword = mapCredentialChange(input.webdavPassword);
  }
  if (input.webdavServerUrl !== undefined) {
    changes.webdavServerUrl = input.webdavServerUrl;
  }
  if (input.webdavUsername !== undefined) changes.webdavUsername = input.webdavUsername;
  return changes;
}

function mapCredentialChange(
  change: NonNullable<SyncChangesInput["s3AccessKeyId"]>,
): NonNullable<SyncChangesRequest["s3AccessKeyId"]> {
  switch (change.operation) {
    case "keep":
      return { operation: change.operation };
    case "clear":
      return { operation: change.operation };
    case "replace":
      return { operation: change.operation, value: change.value };
  }
}

function mapSettingValue(value: SettingsSource["values"][number]["value"]): KernelSettingValue {
  switch (value.type) {
    case "boolean":
      return { type: value.type, value: value.value };
    case "integer":
      return { type: value.type, value: value.value };
    case "number":
      return { type: value.type, value: value.value };
    case "string":
      return { type: value.type, value: value.value };
    case "nullable-integer":
      return { type: value.type, value: value.value };
    case "nullable-string":
      return { type: value.type, value: value.value };
    case "font-family":
      return {
        type: value.type,
        value:
          value.value.source === "theme"
            ? {
                family: value.value.family,
                source: value.value.source,
              }
            : {
                family: value.value.family,
                source: value.value.source,
              },
      };
  }
}

function mapSyncConfig(config: SyncConfigSource): KernelSyncConfigSnapshot {
  return {
    configured: config.configured,
    enabled: config.enabled,
    generateConflictDocument: config.generateConflictDocument,
    intervalSeconds: config.intervalSeconds,
    issues: config.issues.map((issue) => ({
      code: issue.code,
      field: issue.field,
      message: issue.message,
    })),
    mode: config.mode,
    provider: config.provider,
    readiness: config.readiness,
    remoteRoot: config.remoteRoot,
    revision: config.revision as KernelRevision,
    s3: {
      accessKeyId: { present: config.s3.accessKeyId.present },
      addressingStyle: config.s3.addressingStyle,
      bucket: config.s3.bucket,
      endpointUrl: {
        redacted: config.s3.endpointUrl.redacted,
        value: config.s3.endpointUrl.value,
      },
      region: config.s3.region,
      requestTimeoutSeconds: config.s3.requestTimeoutSeconds,
      secretAccessKey: { present: config.s3.secretAccessKey.present },
      tlsVerification: config.s3.tlsVerification,
    },
    webdav: {
      password: { present: config.webdav.password.present },
      serverUrl: {
        redacted: config.webdav.serverUrl.redacted,
        value: config.webdav.serverUrl.value,
      },
      username: config.webdav.username,
    },
  };
}

function mapSyncConnectionTest(
  result: SyncConnectionTestSource,
): KernelSyncConnectionTestSnapshot {
  return {
    checkedTarget: result.checkedTarget,
    configRevision: result.configRevision as KernelRevision,
    provider: result.provider,
  };
}

function mapSyncRun(run: SyncRunSource): KernelSyncRunSnapshot {
  return {
    acceptedAt: run.acceptedAt,
    configRevision: run.configRevision as KernelRevision,
    runId: run.runId,
  };
}

function mapSyncStatus(status: SyncStatusSource): KernelSyncStatusSnapshot {
  return {
    activeRunId: status.activeRunId,
    completionState: status.completionState,
    configRevision: status.configRevision as KernelRevision | null,
    error:
      status.error === null
        ? null
        : {
            category: status.error.category,
            code: status.error.code,
            httpStatus: status.error.httpStatus,
            method: status.error.method,
            operation: status.error.operation,
            provider: status.error.provider,
            providerErrorCode: status.error.providerErrorCode,
            relativePath: status.error.relativePath as KernelWorkspaceRelativePath | undefined,
            requestId: status.error.requestId,
            runId: status.error.runId,
          },
    lastAttemptAt: status.lastAttemptAt,
    lastSuccessfulSyncAt: status.lastSuccessfulSyncAt,
    lastTrigger: status.lastTrigger,
    provider: status.provider,
    summary:
      status.summary === null
        ? null
        : {
            bytesDownloaded: status.summary.bytesDownloaded,
            bytesUploaded: status.summary.bytesUploaded,
            conflictFiles: status.summary.conflictFiles,
            downloadedFiles: status.summary.downloadedFiles,
            scannedFiles: status.summary.scannedFiles,
            skippedFiles: status.summary.skippedFiles,
            uploadedFiles: status.summary.uploadedFiles,
          },
  };
}

function mapDocument(
  document: DocumentSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelDocumentSnapshot {
  return {
    contents: document.contents,
    ...mapDocumentEntry(document, workspaceGeneration),
    kind: "file",
  };
}

function mapCreatedDocument(
  document: CreatedDocumentSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelCreatedDocumentSnapshot {
  if (document.kind === "file") {
    return {
      contents: document.contents,
      ...mapDocumentEntry(document, workspaceGeneration),
      kind: "file",
    };
  }
  return {
    ...mapDocumentEntry(document, workspaceGeneration),
    kind: "directory",
  };
}

function mapDocumentEntry(
  document: DocumentEntrySource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelDocumentEntrySnapshot {
  return {
    kind: document.kind,
    locator: document.id as KernelDocumentLocator,
    modifiedAt: document.modifiedAt,
    name: document.name,
    parent: document.parent as KernelWorkspaceRelativePath,
    relativePath: document.path as KernelWorkspaceRelativePath,
    revision: document.revision as KernelRevision,
    sizeBytes: document.sizeBytes,
    workspaceGeneration,
  };
}

function mapDocumentPage(
  page: DocumentPageSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelDocumentPageSnapshot {
  return {
    items: page.items.map((document) => mapDocumentEntry(document, workspaceGeneration)),
    nextCursor: page.nextCursor as KernelPageCursor | null,
    workspaceGeneration,
  };
}

function mapSearchPage(
  page: SearchPageSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelSearchPageSnapshot {
  return {
    items: page.items.map((item) => ({
      column: item.column,
      document: mapDocumentEntry(item.document, workspaceGeneration),
      line: item.line,
      preview: item.preview,
    })),
    nextCursor: page.nextCursor as KernelPageCursor | null,
    workspaceGeneration,
  };
}

function mapHistoryPage(
  page: HistoryPageSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelHistoryPageSnapshot {
  return {
    items: page.items.map((entry) => ({
      createdAt: entry.createdAt,
      documentLocator: entry.documentId as KernelDocumentLocator,
      revision: entry.revision as KernelRevision,
      sizeBytes: entry.sizeBytes,
      snapshotId: entry.snapshotId as KernelHistorySnapshotId,
      workspaceGeneration,
    })),
    nextCursor: page.nextCursor as KernelPageCursor | null,
    workspaceGeneration,
  };
}

function joinWorkspacePath(parent: string, name: string) {
  return parent === "" ? name : `${parent}/${name}`;
}
