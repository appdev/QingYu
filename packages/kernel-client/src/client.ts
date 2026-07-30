import type { components } from "./generated/kernel-v1.ts";
import {
  KernelHttpTransport,
  type KernelHttpTransportOptions,
} from "./transport.ts";
import {
  isCreatedDocument,
  isDocumentContent,
  isDocumentEntry,
  isDocumentPage,
  isHistoryPage,
  isHistorySnapshot,
  isInventoryPage,
  isLiveHealth,
  isReadyHealth,
  isRuntime,
  isSearchPage,
  isSettingsSnapshot,
  isSyncConfig,
  isSyncConnection,
  isSyncRun,
  isSyncStatus,
  isVersion,
  isWorkspace,
} from "./validation.ts";

type Schemas = components["schemas"];

export interface KernelRequestOptions {
  signal?: AbortSignal;
}

export interface KernelSystemClient {
  live(options?: KernelRequestOptions): Promise<Schemas["LiveHealthResponse"]>;
  ready(options?: KernelRequestOptions): Promise<Schemas["ReadyHealthResponse"]>;
  version(options?: KernelRequestOptions): Promise<Schemas["SystemVersionResponse"]>;
  runtime(options?: KernelRequestOptions): Promise<Schemas["RuntimeStateDto"]>;
}

export interface KernelWorkspaceClient {
  get(options?: KernelRequestOptions): Promise<Schemas["WorkspaceDto"]>;
  search(
    query: Schemas["SearchWorkspaceQuery"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["SearchPageDto"]>;
}

export interface KernelDocumentsClient {
  list(
    query?: Schemas["ListDocumentsQuery"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["DocumentPageDto"]>;
  create(
    request: Schemas["CreateDocumentRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["CreatedDocumentDto"]>;
  get(
    documentId: Schemas["DocumentId"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["DocumentContentDto"]>;
  update(
    documentId: Schemas["DocumentId"],
    request: Schemas["UpdateDocumentRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["DocumentContentDto"]>;
  move(
    documentId: Schemas["DocumentId"],
    request: Schemas["MoveDocumentRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["DocumentEntryDto"]>;
  delete(
    documentId: Schemas["DocumentId"],
    request: Schemas["DeleteDocumentRequest"],
    options?: KernelRequestOptions,
  ): Promise<undefined>;
  listHistory(
    documentId: Schemas["DocumentId"],
    query?: Schemas["PageQuery"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["DocumentHistoryPageDto"]>;
  getHistory(
    documentId: Schemas["DocumentId"],
    snapshotId: Schemas["SnapshotId"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["DocumentHistorySnapshotDto"]>;
  restoreHistory(
    documentId: Schemas["DocumentId"],
    snapshotId: Schemas["SnapshotId"],
    request: Schemas["RestoreDocumentHistoryRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["DocumentContentDto"]>;
}

export interface KernelResourcesClient {
  list(
    query?: Schemas["ListWorkspaceInventoryQuery"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["WorkspaceInventoryPageDto"]>;
  open(
    resourceId: Schemas["ResourceId"],
    kind: Schemas["ResourceKind"],
    options?: KernelRequestOptions,
  ): Promise<Response>;
}

export interface KernelSettingsClient {
  get(options?: KernelRequestOptions): Promise<Schemas["SettingsSnapshotDto"]>;
  patch(
    request: Schemas["PatchSettingsRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["SettingsSnapshotDto"]>;
}

export interface KernelSyncClient {
  getConfig(options?: KernelRequestOptions): Promise<Schemas["SyncConfigViewDto"]>;
  patchConfig(
    request: Schemas["PatchSyncConfigRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["SyncConfigViewDto"]>;
  testConnection(
    request: Schemas["TestSyncConnectionRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["SyncConnectionTestDto"]>;
  getStatus(options?: KernelRequestOptions): Promise<Schemas["SyncStatusDto"]>;
  trigger(
    request: Schemas["TriggerSyncRunRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["SyncRunAcceptedDto"]>;
}

export interface KernelClient {
  readonly system: KernelSystemClient;
  readonly workspace: KernelWorkspaceClient;
  readonly resources: KernelResourcesClient;
  readonly documents: KernelDocumentsClient;
  readonly settings: KernelSettingsClient;
  readonly sync: KernelSyncClient;
}

export type CreateKernelClientOptions = KernelHttpTransportOptions;

export function createKernelClient(options: CreateKernelClientOptions): KernelClient {
  const transport = new KernelHttpTransport(options);
  const documentPath = (documentId: Schemas["DocumentId"]) =>
    `/api/v1/documents/${encodeURIComponent(documentId)}`;
  const resourcePath = (resourceId: Schemas["ResourceId"]) =>
    `/api/v1/resources/${encodeURIComponent(resourceId)}`;

  return {
    system: {
      live: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/health/live",
          authenticated: false,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isLiveHealth }),
      ready: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/health/ready",
          signal: requestOptions?.signal,
        }, { status: 200, validate: isReadyHealth }),
      version: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/system/version",
          signal: requestOptions?.signal,
        }, { status: 200, validate: isVersion }),
      runtime: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/runtime",
          signal: requestOptions?.signal,
        }, { status: 200, validate: isRuntime }),
    },
    workspace: {
      get: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/workspace",
          signal: requestOptions?.signal,
        }, { status: 200, validate: isWorkspace }),
      search: (query, requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/search",
          query,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isSearchPage }),
    },
    resources: {
      list: (query, requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/inventory",
          query,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isInventoryPage }),
      open: (resourceId, kind, requestOptions) =>
        transport.requestBinary({
          method: "GET",
          path: resourcePath(resourceId),
          query: { kind },
          signal: requestOptions?.signal,
        }, {
          status: 200,
          mediaTypes: [
            "application/octet-stream",
            "image/gif",
            "image/jpeg",
            "image/png",
            "image/webp",
          ],
        }),
    },
    documents: {
      list: (query, requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/documents",
          query,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isDocumentPage }),
      create: (request, requestOptions) =>
        transport.request({
          method: "POST",
          path: "/api/v1/documents",
          body: request,
          signal: requestOptions?.signal,
        }, { status: 201, validate: isCreatedDocument }),
      get: (documentId, requestOptions) =>
        transport.request({
          method: "GET",
          path: documentPath(documentId),
          signal: requestOptions?.signal,
        }, { status: 200, validate: isDocumentContent }),
      update: (documentId, request, requestOptions) =>
        transport.request({
          method: "PUT",
          path: documentPath(documentId),
          body: request,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isDocumentContent }),
      move: (documentId, request, requestOptions) =>
        transport.request({
          method: "POST",
          path: `${documentPath(documentId)}/move`,
          body: request,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isDocumentEntry }),
      delete: (documentId, request, requestOptions) =>
        transport.request({
          method: "POST",
          path: `${documentPath(documentId)}/delete`,
          body: request,
          signal: requestOptions?.signal,
        }, { status: 204 }),
      listHistory: (documentId, query, requestOptions) =>
        transport.request({
          method: "GET",
          path: `${documentPath(documentId)}/history`,
          query,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isHistoryPage }),
      getHistory: (documentId, snapshotId, requestOptions) =>
        transport.request({
          method: "GET",
          path: `${documentPath(documentId)}/history/${encodeURIComponent(snapshotId)}`,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isHistorySnapshot }),
      restoreHistory: (documentId, snapshotId, request, requestOptions) =>
        transport.request({
          method: "POST",
          path: `${documentPath(documentId)}/history/${encodeURIComponent(snapshotId)}/restore`,
          body: request,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isDocumentContent }),
    },
    settings: {
      get: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/settings",
          signal: requestOptions?.signal,
        }, { status: 200, validate: isSettingsSnapshot }),
      patch: (request, requestOptions) =>
        transport.request({
          method: "PATCH",
          path: "/api/v1/settings",
          body: request,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isSettingsSnapshot }),
    },
    sync: {
      getConfig: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/sync/config",
          signal: requestOptions?.signal,
        }, { status: 200, validate: isSyncConfig }),
      patchConfig: (request, requestOptions) =>
        transport.request({
          method: "PATCH",
          path: "/api/v1/sync/config",
          body: request,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isSyncConfig }),
      testConnection: (request, requestOptions) =>
        transport.request({
          method: "POST",
          path: "/api/v1/sync/connection-test",
          body: request,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isSyncConnection }),
      getStatus: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/sync/status",
          signal: requestOptions?.signal,
        }, { status: 200, validate: isSyncStatus }),
      trigger: (request, requestOptions) =>
        transport.request({
          method: "POST",
          path: "/api/v1/sync/runs",
          body: request,
          signal: requestOptions?.signal,
        }, { status: 202, validate: isSyncRun }),
    },
  };
}
