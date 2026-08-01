import type { components } from "./generated/kernel-v1.ts";
import {
  KernelHttpTransport,
  type KernelHttpTransportOptions,
} from "./transport.ts";
import {
  isAppConfigSnapshot,
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
  isResourceEntry,
  isResourceBatchResponse,
  isSearchPage,
  isServerAuthenticationStatus,
  isServerSession,
  isSettingsSnapshot,
  isSyncConfig,
  isSyncConnection,
  isSyncRun,
  isSyncRunStatus,
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

export interface KernelAuthenticationClient {
  status(options?: KernelRequestOptions): Promise<Schemas["ServerAuthenticationStatusDto"]>;
  initialize(
    request: Schemas["InitializeServerOwnerRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["ServerSessionDto"]>;
  login(
    request: Schemas["CreateServerSessionRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["ServerSessionDto"]>;
  getSession(options?: KernelRequestOptions): Promise<Schemas["ServerSessionDto"]>;
  logout(options?: KernelRequestOptions): Promise<undefined>;
  changePassword(
    request: Schemas["ChangeServerOwnerPasswordRequest"],
    options?: KernelRequestOptions,
  ): Promise<undefined>;
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
  create(
    documentId: Schemas["DocumentId"],
    request: KernelCreateResourceRequest,
    options?: KernelRequestOptions,
  ): Promise<Schemas["ResourceEntryDto"]>;
  createBatch(
    documentId: Schemas["DocumentId"],
    request: KernelCreateResourceBatchRequest,
    options?: KernelRequestOptions,
  ): Promise<Schemas["CreateWorkspaceResourceBatchResponse"]>;
}

type KernelCreateResourceMetadata = Schemas["CreateWorkspaceResourceQuery"];

export type KernelCreateResourceRequest = Omit<KernelCreateResourceMetadata, "kind"> & {
  body: Blob;
} & (
  | {
    kind: "image";
    mediaType: KernelImageMediaType;
  }
  | {
    kind: "attachment";
    mediaType: "application/octet-stream";
  }
);

export type KernelImageMediaType =
  | "image/avif"
  | "image/bmp"
  | "image/gif"
  | "image/jpeg"
  | "image/png"
  | "image/svg+xml"
  | "image/webp";

export type KernelCreateResourceBatchRequest = Omit<
  Schemas["CreateWorkspaceResourceBatchRequest"],
  "items"
> & {
  items: Array<{
    body: Blob;
    kind: "image";
    mediaType: KernelImageMediaType;
    name: Schemas["ResourceName"];
  }>;
};

export interface KernelSettingsClient {
  get(options?: KernelRequestOptions): Promise<Schemas["SettingsSnapshotDto"]>;
  patch(
    request: Schemas["PatchSettingsRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["SettingsSnapshotDto"]>;
}

export interface KernelAppConfigClient {
  get(options?: KernelRequestOptions): Promise<Schemas["AppConfigSnapshotDto"]>;
  patchState(
    request: Schemas["PatchAppConfigStateRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["AppConfigSnapshotDto"]>;
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
  getRun(
    runId: Schemas["RunId"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["SyncRunStatusDto"]>;
  trigger(
    request: Schemas["TriggerSyncRunRequest"],
    options?: KernelRequestOptions,
  ): Promise<Schemas["SyncRunAcceptedDto"]>;
}

export interface KernelClient {
  readonly auth: KernelAuthenticationClient;
  readonly system: KernelSystemClient;
  readonly workspace: KernelWorkspaceClient;
  readonly resources: KernelResourcesClient;
  readonly documents: KernelDocumentsClient;
  readonly settings: KernelSettingsClient;
  readonly appConfig: KernelAppConfigClient;
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
    auth: {
      status: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/auth/status",
          authenticated: false,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isServerAuthenticationStatus }),
      initialize: (request, requestOptions) =>
        transport.request({
          method: "POST",
          path: "/api/v1/auth/initialize",
          body: request,
          authenticated: false,
          signal: requestOptions?.signal,
        }, { status: 201, validate: isServerSession }),
      login: (request, requestOptions) =>
        transport.request({
          method: "POST",
          path: "/api/v1/auth/session",
          body: request,
          authenticated: false,
          signal: requestOptions?.signal,
        }, { status: 201, validate: isServerSession }),
      getSession: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/auth/session",
          signal: requestOptions?.signal,
        }, { status: 200, validate: isServerSession }),
      logout: (requestOptions) =>
        transport.request({
          method: "POST",
          path: "/api/v1/auth/logout",
          signal: requestOptions?.signal,
        }, { status: 204 }),
      changePassword: (request, requestOptions) =>
        transport.request({
          method: "PATCH",
          path: "/api/v1/auth/password",
          body: request,
          signal: requestOptions?.signal,
        }, { status: 204 }),
    },
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
          mediaTypes: kind === "image"
            ? ["image/avif", "image/bmp", "image/gif", "image/jpeg", "image/png", "image/svg+xml", "image/webp"]
            : ["application/octet-stream"],
        }),
      create: (documentId, request, requestOptions) =>
        transport.requestRaw({
          method: "POST",
          path: `${documentPath(documentId)}/resources`,
          query: {
            folder: request.folder,
            kind: request.kind,
            name: request.name,
            workspaceGeneration: request.workspaceGeneration,
          },
          rawBody: request.body,
          mediaType: request.mediaType,
          signal: requestOptions?.signal,
        }, { status: 201, validate: isResourceEntry }),
      createBatch: async (documentId, request, requestOptions) => {
        const items = await Promise.all(request.items.map(async (item) => ({
          bodyBase64: bytesToBase64(new Uint8Array(await item.body.arrayBuffer())),
          kind: item.kind,
          mediaType: item.mediaType,
          name: item.name,
        })));
        return transport.request({
          method: "POST",
          path: `${documentPath(documentId)}/resource-batches`,
          body: {
            batchId: request.batchId,
            folder: request.folder,
            items,
            workspaceGeneration: request.workspaceGeneration,
          },
          signal: requestOptions?.signal,
        }, { status: 201, validate: isResourceBatchResponse });
      },
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
    appConfig: {
      get: (requestOptions) =>
        transport.request({
          method: "GET",
          path: "/api/v1/app-config",
          signal: requestOptions?.signal,
        }, { status: 200, validate: isAppConfigSnapshot }),
      patchState: (request, requestOptions) =>
        transport.request({
          method: "PATCH",
          path: "/api/v1/app-config/state",
          body: request,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isAppConfigSnapshot }),
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
      getRun: (runId, requestOptions) =>
        transport.request({
          method: "GET",
          path: `/api/v1/sync/runs/${encodeURIComponent(runId)}`,
          signal: requestOptions?.signal,
        }, { status: 200, validate: isSyncRunStatus }),
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

function bytesToBase64(bytes: Uint8Array) {
  const chunks: string[] = [];
  const chunkSize = 32 * 1024;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    chunks.push(String.fromCharCode(...bytes.subarray(offset, offset + chunkSize)));
  }
  return btoa(chunks.join(""));
}
