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
  KernelInventorySnapshot,
  KernelInvalidationNotice,
  KernelInvalidationScope,
  KernelPageCursor,
  KernelRevision,
  KernelRuntimeCapabilities,
  KernelRuntimeSnapshot,
  KernelResourceSnapshot,
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
import {
  KernelApiError,
  KernelEventError,
  type KernelClient,
  type KernelEventConnection,
  type KernelEventFrame,
  type KernelEventsClient,
  type KernelReloadScope,
  type KernelSnapshotNotice,
} from "@markra/kernel-client";

export type ServerKernelDomainAdapterOptions = {
  readonly events?: KernelEventsClient;
  readonly instanceId: string;
  readonly onAuthenticationRequired: () => unknown;
  readonly workspaceGeneration: string;
  readonly workspaceId: string;
};

export type ServerKernelEventNotice =
  | { readonly frame: KernelEventFrame; readonly kind: "event" }
  | {
      readonly kind: "snapshot-required";
      readonly reason: KernelSnapshotNotice["reason"];
      readonly reloadScopes: readonly KernelReloadScope[];
    };

export type ServerKernelEventSource = {
  readonly available: boolean;
  readonly subscribe: (
    listener: (notice: ServerKernelEventNotice) => unknown,
  ) => () => undefined;
};

export type ServerKernelHistorySnapshot = KernelHistorySnapshot;
export type ServerKernelResourceSnapshot = KernelResourceSnapshot;
export type ServerKernelInventoryEntry = KernelInventoryEntry;
export type ServerKernelInventorySnapshot = KernelInventorySnapshot;

export type ServerKernelDomainPort = Omit<KernelDomainPort, "documents" | "resources"> & {
  readonly documents: Omit<KernelDomainPort["documents"], "history"> & {
    readonly history: KernelDomainPort["documents"]["history"] & {
      readonly read: NonNullable<KernelDomainPort["documents"]["history"]["read"]>;
    };
  };
  readonly invalidations: NonNullable<KernelDomainPort["invalidations"]>;
  readonly resources: NonNullable<KernelDomainPort["resources"]>;
  readonly serverEvents: ServerKernelEventSource;
};

export type ServerKernelDomainAdapter = {
  readonly port: ServerKernelDomainPort;
  readonly release: () => undefined;
};

export type ServerKernelDomainAdapterErrorCode =
  | "authentication-required"
  | "protocol-mismatch"
  | "released"
  | "workspace-generation-mismatch";

const ERROR_MESSAGES: Record<ServerKernelDomainAdapterErrorCode, string> = {
  "authentication-required": "The Server Kernel session is no longer authenticated.",
  "protocol-mismatch": "The Server Kernel no longer matches its bootstrap contract.",
  released: "The Server Kernel adapter has been released.",
  "workspace-generation-mismatch": "The document belongs to a different workspace generation.",
};

export class ServerKernelDomainAdapterError extends Error {
  readonly code: ServerKernelDomainAdapterErrorCode;

  constructor(code: ServerKernelDomainAdapterErrorCode) {
    super(ERROR_MESSAGES[code]);
    this.name = "ServerKernelDomainAdapterError";
    this.code = code;
  }
}

export async function createServerKernelDomainAdapter(
  client: KernelClient,
  options: ServerKernelDomainAdapterOptions,
): Promise<ServerKernelDomainAdapter> {
  const workspaceGeneration = options.workspaceGeneration as KernelWorkspaceGeneration;
  const requests = new AbortController();
  const eventListeners = new Set<(notice: ServerKernelEventNotice) => unknown>();
  const invalidationListeners = new Set<(notice: KernelInvalidationNotice) => unknown>();
  let eventConnection: KernelEventConnection | undefined;
  let eventAvailable = false;
  let active = true;
  let authenticationNoticeSent = false;

  const release = () => {
    if (!active) return undefined;
    active = false;
    requests.abort();
    eventAvailable = false;
    try {
      eventConnection?.close();
    } catch {
      // Closing the optional event stream is best-effort.
    }
    eventConnection = undefined;
    eventListeners.clear();
    invalidationListeners.clear();
    return undefined;
  };
  const assertActive = () => {
    if (!active) throw new ServerKernelDomainAdapterError("released");
  };
  const protocolMismatch = (): never => {
    release();
    throw new ServerKernelDomainAdapterError("protocol-mismatch");
  };
  const authenticationRequired = (): never => {
    notifyAuthenticationRequired();
    throw new ServerKernelDomainAdapterError("authentication-required");
  };
  const notifyAuthenticationRequired = () => {
    release();
    if (!authenticationNoticeSent) {
      authenticationNoticeSent = true;
      try {
        options.onAuthenticationRequired();
      } catch {
        // Authentication state ownership cannot depend on a view callback.
      }
    }
    return undefined;
  };
  const publishEvent = (notice: ServerKernelEventNotice) => {
    if (!active) return undefined;
    for (const listener of [...eventListeners]) {
      try {
        listener(notice);
      } catch {
        // A consumer cannot corrupt the connection owner.
      }
    }
    const invalidation = mapInvalidation(notice);
    if (invalidation !== undefined) {
      for (const listener of [...invalidationListeners]) {
        try {
          listener(invalidation);
        } catch {
          // A consumer cannot corrupt the connection owner.
        }
      }
    }
    return undefined;
  };
  const request = async <T>(operation: () => Promise<T>) => {
    assertActive();
    try {
      const result = await operation();
      assertActive();
      return result;
    } catch (error: unknown) {
      if (error instanceof KernelApiError && error.code === "unauthorized") {
        return authenticationRequired();
      }
      throw error;
    }
  };
  const assertWorkspaceGeneration = (candidate: KernelWorkspaceGeneration) => {
    if (candidate !== workspaceGeneration) {
      throw new ServerKernelDomainAdapterError("workspace-generation-mismatch");
    }
  };
  const confirmWorkspaceIdentity = async () => {
    const workspace = await request(() => client.workspace.get({ signal: requests.signal }));
    if (!matchesServerWorkspace(
      workspace,
      options.workspaceId,
      workspaceGeneration,
    )) {
      protocolMismatch();
    }
    return workspace;
  };
  const prepareDocumentOperation = async (generation: KernelWorkspaceGeneration) => {
    assertWorkspaceGeneration(generation);
    await confirmWorkspaceIdentity();
  };
  const prepareInstanceOperation = async () => {
    await confirmWorkspaceIdentity();
  };
  const assertDocumentIdentity = (actual: string, expected: KernelDocumentLocator) => {
    if (actual !== expected) protocolMismatch();
  };

  const port: ServerKernelDomainPort = {
    availability: "available",
    documents: {
      create: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const document = await request(() => client.documents.create(
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
        ));
        if (
          document.kind !== input.kind ||
          document.name !== input.name ||
          document.parent !== input.parent ||
          document.path !== joinWorkspacePath(input.parent, input.name)
        ) {
          protocolMismatch();
        }
        await confirmWorkspaceIdentity();
        return mapCreatedDocument(document, workspaceGeneration);
      },
      delete: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const result = await request(() => client.documents.delete(
          input.locator,
          {
            deletionPolicy: input.deletionPolicy,
            expectedRevision: input.expectedRevision,
            workspaceGeneration: input.workspaceGeneration,
          },
          { signal: requests.signal },
        ));
        await confirmWorkspaceIdentity();
        return result;
      },
      history: {
        list: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const page = await request(() => client.documents.listHistory(
            input.locator,
            { cursor: input.cursor, limit: input.limit },
            { signal: requests.signal },
          ));
          for (const entry of page.items) {
            assertDocumentIdentity(entry.documentId, input.locator);
          }
          await confirmWorkspaceIdentity();
          return mapHistoryPage(page, workspaceGeneration);
        },
        restore: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const document = await request(() => client.documents.restoreHistory(
            input.locator,
            input.snapshotId,
            {
              expectedRevision: input.expectedRevision,
              workspaceGeneration: input.workspaceGeneration,
            },
            { signal: requests.signal },
          ));
          assertDocumentIdentity(document.id, input.locator);
          await confirmWorkspaceIdentity();
          return mapDocument(document, workspaceGeneration);
        },
        read: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const history = await request(() => client.documents.getHistory(
            input.locator,
            input.snapshotId,
            { signal: requests.signal },
          ));
          assertDocumentIdentity(history.documentId, input.locator);
          if (history.snapshotId !== input.snapshotId) protocolMismatch();
          await confirmWorkspaceIdentity();
          return {
            contents: history.contents,
            documentLocator: history.documentId as KernelDocumentLocator,
            revision: history.revision as KernelRevision,
            snapshotId: history.snapshotId as KernelHistorySnapshotId,
            workspaceGeneration,
          };
        },
      },
      list: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const page = await request(() => client.documents.list(
          { cursor: input.cursor, limit: input.limit, parent: input.parent },
          { signal: requests.signal },
        ));
        await confirmWorkspaceIdentity();
        return mapDocumentPage(page, workspaceGeneration);
      },
      move: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const document = await request(() => client.documents.move(
          input.locator,
          {
            expectedRevision: input.expectedRevision,
            name: input.name,
            targetParent: input.targetParent,
            workspaceGeneration: input.workspaceGeneration,
          },
          { signal: requests.signal },
        ));
        if (
          document.name !== input.name ||
          document.parent !== input.targetParent ||
          document.path !== joinWorkspacePath(input.targetParent, input.name)
        ) {
          protocolMismatch();
        }
        await confirmWorkspaceIdentity();
        return mapDocumentEntry(document, workspaceGeneration);
      },
      read: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const document = await request(() => client.documents.get(
          input.locator,
          { signal: requests.signal },
        ));
        assertDocumentIdentity(document.id, input.locator);
        await confirmWorkspaceIdentity();
        return mapDocument(document, workspaceGeneration);
      },
      search: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const page = await request(() => client.workspace.search(
          { cursor: input.cursor, limit: input.limit, query: input.query },
          { signal: requests.signal },
        ));
        await confirmWorkspaceIdentity();
        return mapSearchPage(page, workspaceGeneration);
      },
      update: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const document = await request(() => client.documents.update(
          input.locator,
          {
            contents: input.contents,
            expectedRevision: input.expectedRevision,
            workspaceGeneration: input.workspaceGeneration,
          },
          { signal: requests.signal },
        ));
        assertDocumentIdentity(document.id, input.locator);
        await confirmWorkspaceIdentity();
        return mapDocument(document, workspaceGeneration);
      },
    },
    invalidations: {
      get available() {
        return active && eventAvailable;
      },
      subscribe: (listener) => {
        assertActive();
        invalidationListeners.add(listener);
        return () => {
          invalidationListeners.delete(listener);
          return undefined;
        };
      },
    },
    runtime: {
      read: async () => {
        const runtime = await request(() => client.system.runtime({ signal: requests.signal }));
        if (!matchesServerRuntime(runtime, options.instanceId)) protocolMismatch();
        return mapRuntime(runtime);
      },
    },
    resources: {
      create: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const resourceRequest = input.kind === "image"
          ? {
              body: input.body,
              folder: input.folder,
              kind: input.kind,
              mediaType: input.mediaType,
              name: input.name,
              workspaceGeneration: input.workspaceGeneration,
            }
          : {
              body: input.body,
              folder: input.folder,
              kind: input.kind,
              mediaType: input.mediaType,
              name: input.name,
              workspaceGeneration: input.workspaceGeneration,
            };
        const resource = await request(() => client.resources.create(
          input.documentLocator,
          resourceRequest,
          { signal: requests.signal },
        ));
        await confirmWorkspaceIdentity();
        return mapResourceEntry(resource, workspaceGeneration);
      },
      list: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const items: ServerKernelInventoryEntry[] = [];
        const seenCursors = new Set<string>();
        let cursor: string | undefined;
        do {
          const page = await request(() => client.resources.list({
            cursor,
            limit: 100,
            parent: input.parent,
          }, { signal: requests.signal }));
          items.push(...page.items.map((entry) => mapInventoryEntry(
            entry,
            workspaceGeneration,
          )));
          const nextCursor = page.nextCursor ?? undefined;
          if (nextCursor !== undefined && seenCursors.has(nextCursor)) protocolMismatch();
          if (nextCursor !== undefined) seenCursors.add(nextCursor);
          cursor = nextCursor;
        } while (cursor !== undefined);
        await confirmWorkspaceIdentity();
        return { items, workspaceGeneration };
      },
      open: async (input) => {
        await prepareDocumentOperation(input.workspaceGeneration);
        const response = await request(() => client.resources.open(
          input.id,
          input.kind,
          { signal: requests.signal },
        ));
        const body = await response.blob();
        assertActive();
        await confirmWorkspaceIdentity();
        return {
          body,
          mediaType: response.headers.get("content-type")?.split(";", 1)[0]?.trim() ?? "",
        };
      },
    },
    serverEvents: {
      get available() {
        return active && eventAvailable;
      },
      subscribe: (listener) => {
        assertActive();
        eventListeners.add(listener);
        return () => {
          eventListeners.delete(listener);
          return undefined;
        };
      },
    },
    settings: {
      patch: async (input) => {
        await prepareInstanceOperation();
        const settings = await request(() => client.settings.patch(
          mapSettingsPatchRequest(input),
          { signal: requests.signal },
        ));
        await confirmWorkspaceIdentity();
        return mapSettings(settings);
      },
      read: async () => {
        await prepareInstanceOperation();
        const settings = await request(() => client.settings.get({ signal: requests.signal }));
        await confirmWorkspaceIdentity();
        return mapSettings(settings);
      },
    },
    sync: {
      patchConfig: async (input) => {
        await prepareInstanceOperation();
        const config = await request(() => client.sync.patchConfig(
          mapSyncPatchRequest(input),
          { signal: requests.signal },
        ));
        await confirmWorkspaceIdentity();
        return mapSyncConfig(config);
      },
      readConfig: async () => {
        await prepareInstanceOperation();
        const config = await request(() => client.sync.getConfig({ signal: requests.signal }));
        await confirmWorkspaceIdentity();
        return mapSyncConfig(config);
      },
      readStatus: async () => {
        await prepareInstanceOperation();
        const status = await request(() => client.sync.getStatus({ signal: requests.signal }));
        await confirmWorkspaceIdentity();
        return mapSyncStatus(status);
      },
      testConnection: async (input) => {
        await prepareInstanceOperation();
        const result = await request(() => client.sync.testConnection(
          mapSyncTestRequest(input),
          { signal: requests.signal },
        ));
        await confirmWorkspaceIdentity();
        return mapSyncConnectionTest(result);
      },
      trigger: async (expectedConfigRevision) => {
        await prepareInstanceOperation();
        const run = await request(() => client.sync.trigger(
          { expectedConfigRevision },
          { signal: requests.signal },
        ));
        await confirmWorkspaceIdentity();
        return mapSyncRun(run);
      },
    },
    workspace: {
      read: async () => mapWorkspace(await confirmWorkspaceIdentity()),
    },
  };

  if (options.events !== undefined) {
    try {
      eventConnection = options.events.connect({
        onError: (error) => {
          if (
            error instanceof KernelEventError &&
            error.kind === "server-error" &&
            error.frameCode === "unauthorized"
          ) {
            notifyAuthenticationRequired();
          }
        },
        onEvent: (frame) => publishEvent({ frame, kind: "event" }),
        onReady: (frame) => {
          if (frame.instanceId !== options.instanceId) notifyAuthenticationRequired();
        },
        onSnapshotRequired: (notice) => publishEvent({
          kind: "snapshot-required",
          reason: notice.reason,
          reloadScopes: [...notice.reloadScopes],
        }),
        onStateChange: (state) => {
          eventAvailable = active && state !== "closed";
        },
      }, { signal: requests.signal });
      eventAvailable = active && eventConnection.state !== "closed";
    } catch {
      eventConnection = undefined;
      eventAvailable = false;
    }
  }

  return { port, release };
}

type RuntimeSource = Awaited<ReturnType<KernelClient["system"]["runtime"]>>;
type InventoryEntrySource = Awaited<ReturnType<KernelClient["resources"]["list"]>>["items"][number];
type ResourceEntrySource = Extract<InventoryEntrySource, { entryType: "resource" }>["resource"];
type WorkspaceSource = Awaited<ReturnType<KernelClient["workspace"]["get"]>>;
type DocumentSource = Awaited<ReturnType<KernelClient["documents"]["get"]>>;
type CreatedDocumentSource = Awaited<ReturnType<KernelClient["documents"]["create"]>>;
type DocumentEntrySource = Awaited<ReturnType<KernelClient["documents"]["move"]>>;
type DocumentPageSource = Awaited<ReturnType<KernelClient["documents"]["list"]>>;
type HistoryPageSource = Awaited<ReturnType<KernelClient["documents"]["listHistory"]>>;
type SearchPageSource = Awaited<ReturnType<KernelClient["workspace"]["search"]>>;
type SettingsSource = Awaited<ReturnType<KernelClient["settings"]["get"]>>;
type SyncConfigSource = Awaited<ReturnType<KernelClient["sync"]["getConfig"]>>;
type SyncConnectionSource = Awaited<ReturnType<KernelClient["sync"]["testConnection"]>>;
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

function matchesServerRuntime(runtime: RuntimeSource, instanceId: string) {
  return runtime.instanceId === instanceId &&
    runtime.profile === "server" &&
    runtime.startupState === "ready" &&
    hasRequiredKernelDomainCapabilities(runtime.capabilities);
}

function matchesServerWorkspace(
  workspace: WorkspaceSource,
  workspaceId: string,
  generation: KernelWorkspaceGeneration,
) {
  return workspace.readiness === "ready" &&
    workspace.id === workspaceId &&
    workspace.generation === generation;
}

function mapCapabilities(capabilities: RuntimeSource["capabilities"]): KernelRuntimeCapabilities {
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
): ServerKernelInventoryEntry {
  if (entry.entryType === "document") {
    return {
      document: mapDocumentEntry(entry.document, workspaceGeneration),
      entryType: entry.entryType,
    };
  }
  return {
    entryType: entry.entryType,
    resource: mapResourceEntry(entry.resource, workspaceGeneration),
  };
}

function mapResourceEntry(
  resource: ResourceEntrySource,
  workspaceGeneration: KernelWorkspaceGeneration,
) {
  return {
    id: resource.id,
    kind: resource.kind,
    mediaType: resource.mediaType,
    modifiedAt: resource.modifiedAt,
    name: resource.name,
    parent: resource.parent as KernelWorkspaceRelativePath,
    previewable: resource.previewable,
    relativePath: resource.path as KernelWorkspaceRelativePath,
    revision: resource.revision as KernelRevision,
    sizeBytes: resource.sizeBytes,
    workspaceGeneration,
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

function mapSettingValue(value: SettingsSource["values"][number]["value"]): KernelSettingValue {
  switch (value.type) {
    case "boolean": return { type: value.type, value: value.value };
    case "integer": return { type: value.type, value: value.value };
    case "number": return { type: value.type, value: value.value };
    case "string": return { type: value.type, value: value.value };
    case "nullable-integer": return { type: value.type, value: value.value };
    case "nullable-string": return { type: value.type, value: value.value };
    case "font-family":
      return {
        type: value.type,
        value: value.value.source === "theme"
          ? { family: value.value.family, source: value.value.source }
          : { family: value.value.family, source: value.value.source },
      };
  }
}

function mapSyncPatchRequest(input: SyncPatchInput): SyncPatchRequest {
  return { changes: mapSyncChanges(input.changes), expectedRevision: input.expectedRevision };
}

function mapSyncTestRequest(input: SyncTestInput): SyncTestRequest {
  return { changes: mapSyncChanges(input.changes), expectedRevision: input.expectedRevision };
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
  if (input.s3AddressingStyle !== undefined) changes.s3AddressingStyle = input.s3AddressingStyle;
  if (input.s3Bucket !== undefined) changes.s3Bucket = input.s3Bucket;
  if (input.s3EndpointUrl !== undefined) changes.s3EndpointUrl = input.s3EndpointUrl;
  if (input.s3Region !== undefined) changes.s3Region = input.s3Region;
  if (input.s3RequestTimeoutSeconds !== undefined) {
    changes.s3RequestTimeoutSeconds = input.s3RequestTimeoutSeconds;
  }
  if (input.s3SecretAccessKey !== undefined) {
    changes.s3SecretAccessKey = mapCredentialChange(input.s3SecretAccessKey);
  }
  if (input.s3TlsVerification !== undefined) changes.s3TlsVerification = input.s3TlsVerification;
  if (input.webdavPassword !== undefined) {
    changes.webdavPassword = mapCredentialChange(input.webdavPassword);
  }
  if (input.webdavServerUrl !== undefined) changes.webdavServerUrl = input.webdavServerUrl;
  if (input.webdavUsername !== undefined) changes.webdavUsername = input.webdavUsername;
  return changes;
}

function mapCredentialChange(
  change: NonNullable<SyncChangesInput["s3AccessKeyId"]>,
): NonNullable<SyncChangesRequest["s3AccessKeyId"]> {
  switch (change.operation) {
    case "keep": return { operation: change.operation };
    case "clear": return { operation: change.operation };
    case "replace": return { operation: change.operation, value: change.value };
  }
}

function mapSyncConfig(config: SyncConfigSource): KernelSyncConfigSnapshot {
  return {
    configured: config.configured,
    enabled: config.enabled,
    generateConflictDocument: config.generateConflictDocument,
    intervalSeconds: config.intervalSeconds,
    issues: config.issues.map((issue) => ({ ...issue })),
    mode: config.mode,
    provider: config.provider,
    readiness: config.readiness,
    remoteRoot: config.remoteRoot,
    revision: config.revision as KernelRevision,
    s3: {
      accessKeyId: { present: config.s3.accessKeyId.present },
      addressingStyle: config.s3.addressingStyle,
      bucket: config.s3.bucket,
      endpointUrl: { ...config.s3.endpointUrl },
      region: config.s3.region,
      requestTimeoutSeconds: config.s3.requestTimeoutSeconds,
      secretAccessKey: { present: config.s3.secretAccessKey.present },
      tlsVerification: config.s3.tlsVerification,
    },
    webdav: {
      password: { present: config.webdav.password.present },
      serverUrl: { ...config.webdav.serverUrl },
      username: config.webdav.username,
    },
  };
}

function mapSyncConnectionTest(result: SyncConnectionSource): KernelSyncConnectionTestSnapshot {
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
    error: status.error === null ? null : {
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
    summary: status.summary === null ? null : { ...status.summary },
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
  return { ...mapDocumentEntry(document, workspaceGeneration), kind: "directory" };
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

function mapInvalidation(notice: ServerKernelEventNotice): KernelInvalidationNotice | undefined {
  if (notice.kind === "snapshot-required") {
    return {
      documentChange: notice.reloadScopes.some((scope) =>
        scope === "documents" || scope === "workspace"
      ) ? "snapshot" : undefined,
      scopes: expandReloadScopes(notice.reloadScopes),
    };
  }
  const event = notice.frame.event;
  switch (event.type) {
    case "workspace-changed":
      return {
        documentChange: "tree",
        scopes: ["workspace", "documents", "resources"],
      };
    case "document-created":
      return {
        documentChange: "tree",
        paths: [event.document.path as KernelWorkspaceRelativePath],
        scopes: ["documents", "resources"],
      };
    case "document-changed":
      return {
        documentChange: "content",
        paths: [event.document.path as KernelWorkspaceRelativePath],
        scopes: ["documents", "resources"],
      };
    case "document-moved":
      return {
        documentChange: "tree",
        paths: [event.previousPath, event.document.path] as KernelWorkspaceRelativePath[],
        scopes: ["documents", "resources"],
      };
    case "document-deleted":
      return {
        documentChange: "tree",
        paths: [event.previousPath as KernelWorkspaceRelativePath],
        scopes: ["documents", "resources"],
      };
    case "settings-changed":
      return { scopes: ["settings"] };
    case "sync-config-changed":
      return { scopes: ["sync-config"] };
    case "sync-status-changed":
      return event.status.completionState === "succeeded"
        ? {
            documentChange: "tree",
            scopes: ["sync-status", "documents", "resources"],
          }
        : { scopes: ["sync-status"] };
  }
}

function expandReloadScopes(scopes: readonly KernelReloadScope[]) {
  const expanded = new Set<KernelInvalidationScope>();
  for (const scope of scopes) {
    if (scope === "workspace") {
      expanded.add("workspace");
      expanded.add("documents");
      expanded.add("resources");
    } else if (scope === "documents") {
      expanded.add("documents");
      expanded.add("resources");
    } else {
      expanded.add(scope);
      if (scope === "sync-status") {
        expanded.add("documents");
        expanded.add("resources");
      }
    }
  }
  return [...expanded];
}

function joinWorkspacePath(parent: string, name: string) {
  return parent === "" ? name : `${parent}/${name}`;
}
