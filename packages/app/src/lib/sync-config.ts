export type SyncProvider = "s3" | "webdav";
export type SyncMode = "automatic" | "startup-exit" | "fully-manual";
export type S3AddressingStyle = "auto" | "path" | "virtual-hosted";
export type S3TlsVerification = "skip" | "verify";
export type SyncConfigReadiness = "disabled" | "incomplete" | "ready";
export type SyncTrigger = "app-launch" | "interval" | "manual" | "save" | "settings-exit";

export type QingYuSyncConfig = {
  version: 3;
  enabled: boolean;
  provider: SyncProvider;
  remoteRoot: string;
  mode: SyncMode;
  intervalSeconds: number;
  webdav: {
    serverUrl: string;
    username: string;
    password: string;
  };
  s3: {
    endpointUrl: string;
    region: string;
    bucket: string;
    accessKeyId: string;
    secretAccessKey: string;
    requestTimeoutSeconds: number;
    addressingStyle: S3AddressingStyle;
    tlsVerification: S3TlsVerification;
  };
};

export type SyncConfigPatch =
  | { field: "enabled"; value: boolean }
  | { field: "provider"; value: SyncProvider }
  | { field: "remoteRoot"; value: string }
  | { field: "mode"; value: SyncMode }
  | { field: "intervalSeconds"; value: number }
  | { field: "webdav.serverUrl" | "webdav.username" | "webdav.password"; value: string }
  | {
      field:
        | "s3.endpointUrl"
        | "s3.region"
        | "s3.bucket"
        | "s3.accessKeyId"
        | "s3.secretAccessKey";
      value: string;
    }
  | { field: "s3.requestTimeoutSeconds"; value: number }
  | { field: "s3.addressingStyle"; value: S3AddressingStyle }
  | { field: "s3.tlsVerification"; value: S3TlsVerification };

export function applySyncConfigPatch(
  config: QingYuSyncConfig,
  patch: SyncConfigPatch
): QingYuSyncConfig {
  if (patch.field === "enabled") return { ...config, enabled: patch.value };
  if (patch.field === "provider") return { ...config, provider: patch.value };
  if (patch.field === "remoteRoot") return { ...config, remoteRoot: patch.value };
  if (patch.field === "mode") return { ...config, mode: patch.value };
  if (patch.field === "intervalSeconds") return { ...config, intervalSeconds: patch.value };
  if (patch.field === "webdav.serverUrl") {
    return { ...config, webdav: { ...config.webdav, serverUrl: patch.value } };
  }
  if (patch.field === "webdav.username") {
    return { ...config, webdav: { ...config.webdav, username: patch.value } };
  }
  if (patch.field === "webdav.password") {
    return { ...config, webdav: { ...config.webdav, password: patch.value } };
  }
  if (patch.field === "s3.endpointUrl") {
    return { ...config, s3: { ...config.s3, endpointUrl: patch.value } };
  }
  if (patch.field === "s3.region") return { ...config, s3: { ...config.s3, region: patch.value } };
  if (patch.field === "s3.bucket") return { ...config, s3: { ...config.s3, bucket: patch.value } };
  if (patch.field === "s3.accessKeyId") {
    return { ...config, s3: { ...config.s3, accessKeyId: patch.value } };
  }
  if (patch.field === "s3.secretAccessKey") {
    return { ...config, s3: { ...config.s3, secretAccessKey: patch.value } };
  }
  if (patch.field === "s3.requestTimeoutSeconds") {
    return { ...config, s3: { ...config.s3, requestTimeoutSeconds: patch.value } };
  }
  if (patch.field === "s3.addressingStyle") {
    return { ...config, s3: { ...config.s3, addressingStyle: patch.value } };
  }
  if (patch.field === "s3.tlsVerification") {
    return { ...config, s3: { ...config.s3, tlsVerification: patch.value } };
  }
  return config;
}

export type SyncConfigIssue = {
  code: string;
  field: string;
  message: string;
};

export type SyncConfigLoadIssue = {
  code: string;
  message: string;
};

export type SyncConfigDocument = {
  config: QingYuSyncConfig;
  configured: boolean;
  issues: SyncConfigIssue[];
  readiness: SyncConfigReadiness;
  revision: string;
};

export type SyncConfigLoadResult =
  | { status: "absent"; revision: null }
  | ({ status: "loaded" } & SyncConfigDocument)
  | {
      status: "malformed";
      issue: SyncConfigLoadIssue;
      revision: string;
    }
  | {
      status: "unsupported";
      issue: SyncConfigLoadIssue;
      revision: string;
      version: number;
    };

export type SyncSummary = {
  bytesDownloaded: number;
  bytesUploaded: number;
  conflictFiles: number;
  downloadedFiles: number;
  scannedFiles: number;
  skippedFiles: number;
  uploadedFiles: number;
};

export type SyncSafeError = {
  category: "http" | "integrity" | "local" | "transport" | null;
  code: string;
  httpStatus: number | null;
  method: string | null;
  objectId: string | null;
  operation: string;
  provider: SyncProvider;
  providerErrorCode: string | null;
  relativePath: string | null;
  requestId: string | null;
  runId: string | null;
};

export type SyncStatus = {
  completionState: "attempting" | "failed" | "succeeded";
  error: SyncSafeError | null;
  lastAttemptAt: string;
  lastSuccessfulSyncAt: string | null;
  lastTrigger: SyncTrigger;
  notebookName: string | null;
  notesRoot: string | null;
  provider: SyncProvider;
  revision: string | null;
  summary: SyncSummary | null;
  version: 1;
};

export type NormalSyncRunRequest = {
  applyToken?: string;
  bootstrap?: false;
  notebookName: string;
  notesRoot: string;
  revision: string;
  trigger: SyncTrigger;
};

export type DesktopBootstrapSyncRunRequest = {
  bootstrap: true;
  preparedTargetLease: string;
  revision: string;
  trigger: "manual";
};

export type ManagedBootstrapSyncRunRequest = {
  bootstrap: true;
  notesRoot: string;
  revision: string;
  trigger: "manual";
};

export type BootstrapSyncRunRequest =
  | DesktopBootstrapSyncRunRequest
  | ManagedBootstrapSyncRunRequest;

export type SyncRunRequest = NormalSyncRunRequest | BootstrapSyncRunRequest;

export type SyncRunResult = {
  notebookName: string;
  notesRoot: string;
  provider: SyncProvider;
  revision: string;
  summary: SyncSummary;
  trigger: SyncTrigger;
};

export function notebookNameFromRoot(root: string): string {
  const withoutTrailingSeparators = root.replace(/[\\/]+$/u, "");
  return withoutTrailingSeparators.split(/[\\/]/u).at(-1) ?? "";
}

export type SyncEditingUpdate = {
  active: boolean;
  revision: string | null;
  sessionId: string;
};

export type SyncEditingEvent = SyncEditingUpdate & {
  counter: number;
};

export type SyncApplyUpdate = {
  exitReason: "category-leave" | "window-close";
  revision: string;
  sessionId: string;
  source: "settings-exit";
  token: string;
};

export type SyncApplyIdentity = Pick<SyncApplyUpdate, "revision" | "sessionId" | "token">;

export type SyncPendingApply = SyncApplyUpdate & {
  counter: number;
  state: "claimed" | "completed" | "pending";
};

export type SyncEditingSnapshot = {
  counter: number;
  pendingApply: SyncPendingApply | null;
  state: Omit<SyncEditingUpdate, "active"> | null;
};

export type SyncEditingWriteResult = {
  broadcasted: boolean;
  event: SyncEditingEvent;
};

export type SyncApplyWriteResult = {
  broadcasted: boolean;
  event: SyncPendingApply;
};

export type SyncConnectionTestResult = {
  checkedTarget: string;
  provider: SyncProvider;
};

export type RemoteNotebookCatalogEntry = {
  available: boolean;
  disabledReason: string | null;
  name: string;
};

export type AcceptedSyncJob = {
  jobId: string;
  notesRoot: string;
  repositoryId: string;
};

export type AcceptedMaintenanceJob = {
  jobId: string;
  operation:
    | "rebuild-local-repository"
    | "stop-repository-sync"
    | "change-global-key"
    | "purge-remote-repository"
    | "delete-remote-repository";
  repositoryId: string | null;
};

export type DejavuKeyState = { configured: boolean };

export type ConflictResolutionKind = "keep-local" | "use-remote" | "keep-both";

export type SyncConflictRecord = {
  conflictId: string;
  occurredAt: string;
  relativePath: string;
  repositoryId: string;
  resolution: ConflictResolutionKind | null;
};

export type ConflictVersion = {
  byteSize: number;
  text: string | null;
};

export type ConflictVersions = {
  conflict: SyncConflictRecord;
  local: ConflictVersion | null;
  remote: ConflictVersion;
};

export type ConflictResolution =
  | { kind: "keep-local" }
  | { kind: "use-remote" }
  | { destinationRelativePath: string; kind: "keep-both" };

export type DejavuRepositoryStatus = {
  attempt: number;
  automaticFailureCount: number;
  conflicts: SyncConflictRecord[];
  error: { code: string; operation: string } | null;
  jobId: string;
  lastAttemptAt: string;
  lastDnsRetryAt: string | null;
  lastSuccessfulSyncAt: string | null;
  maintenance: {
    lastLocalPurgeAt: string | null;
    nextLocalPurgeAt: string | null;
  };
  nextScheduledAt: string | null;
  phase: "attempting" | "failed" | "succeeded";
  repositoryId: string;
  sameCount: number;
  transfer: {
    downloadBytes: number;
    downloadChunks: number;
    downloadFiles: number;
    uploadBytes: number;
    uploadChunks: number;
    uploadFiles: number;
  };
  trigger: SyncTrigger;
  version: 1;
};

export type AppSyncConfigRuntime = {
  cancelApply(input: SyncApplyIdentity): Promise<SyncApplyWriteResult>;
  changeGlobalKey(input: { confirmed: true; newKey: string }): Promise<AcceptedMaintenanceJob>;
  deleteRemoteRepository(input: {
    confirmed: true;
    repositoryId: string;
  }): Promise<AcceptedMaintenanceJob>;
  enable(input: { expectedRevision: string | null }): Promise<SyncConfigDocument>;
  exportGlobalKey(input: { confirmed: true }): Promise<string>;
  initializeGlobalKey(input: { key: string }): Promise<DejavuKeyState>;
  load(): Promise<SyncConfigLoadResult>;
  loadKeyState(): Promise<DejavuKeyState>;
  listNotebooks(input: { revision: string }): Promise<RemoteNotebookCatalogEntry[]>;
  listConflicts(input: { repositoryId: string }): Promise<SyncConflictRecord[]>;
  loadEditing(): Promise<SyncEditingSnapshot>;
  loadRepositoryStatus(input: { notesRoot: string }): Promise<DejavuRepositoryStatus | null>;
  loadStatus(): Promise<SyncStatus | null>;
  patch(input: {
    expectedRevision: string;
    patch: SyncConfigPatch;
  }): Promise<SyncConfigDocument>;
  purgeRemoteRepository(input: {
    confirmed: true;
    repositoryId: string;
  }): Promise<AcceptedMaintenanceJob>;
  recover(input: {
    config: QingYuSyncConfig;
    expectedRevision: string;
  }): Promise<SyncConfigDocument>;
  rebuildLocalRepository(input: {
    confirmed: true;
    repositoryId: string;
  }): Promise<AcceptedMaintenanceJob>;
  requestApply(input: SyncApplyUpdate): Promise<SyncApplyWriteResult>;
  readConflict(input: {
    conflictId: string;
    repositoryId: string;
  }): Promise<ConflictVersions>;
  reset(input: {
    confirmed: true;
    expectedRevision: string | null;
  }): Promise<SyncConfigDocument>;
  setEditing(input: SyncEditingUpdate): Promise<SyncEditingWriteResult>;
  stopRepositorySync(input: {
    confirmed: true;
    repositoryId: string;
  }): Promise<AcceptedMaintenanceJob>;
  resolveConflict(input: {
    conflictId: string;
    repositoryId: string;
    resolution: ConflictResolution;
  }): Promise<AcceptedSyncJob>;
  sync(input: SyncRunRequest): Promise<SyncRunResult>;
  testConnection(input: { revision: string }): Promise<SyncConnectionTestResult>;
};

export function normalizeSyncConfigLoadResult(result: SyncConfigLoadResult): SyncConfigLoadResult {
  if (result.status === "loaded") {
    return {
      config: result.config,
      configured: result.configured,
      issues: result.issues,
      readiness: result.readiness,
      revision: result.revision,
      status: result.status
    };
  }
  if (result.status === "unsupported") {
    return {
      issue: { code: result.issue.code, message: result.issue.message },
      revision: result.revision,
      status: result.status,
      version: result.version
    };
  }
  if (result.status === "malformed") {
    return {
      issue: { code: result.issue.code, message: result.issue.message },
      revision: result.revision,
      status: result.status
    };
  }
  return { revision: null, status: "absent" };
}
