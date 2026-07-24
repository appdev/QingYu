export type SyncProvider = "s3" | "webdav";
export type S3AddressingStyle = "auto" | "path" | "virtual-hosted";
export type S3TlsVerification = "skip" | "verify";
export type SyncConfigReadiness = "disabled" | "incomplete" | "ready";
export type SyncTrigger = "app-launch" | "interval" | "manual" | "save" | "settings-exit";

export type QingYuSyncConfig = {
  version: 2;
  enabled: boolean;
  provider: SyncProvider;
  remoteRoot: string;
  autoSyncOnSave: boolean;
  intervalMinutes: number;
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
  | { field: "autoSyncOnSave"; value: boolean }
  | { field: "intervalMinutes"; value: number }
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
  if (patch.field === "autoSyncOnSave") return { ...config, autoSyncOnSave: patch.value };
  if (patch.field === "intervalMinutes") return { ...config, intervalMinutes: patch.value };
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

export type AppSyncConfigRuntime = {
  cancelApply(input: SyncApplyIdentity): Promise<SyncApplyWriteResult>;
  enable(input: { expectedRevision: string | null }): Promise<SyncConfigDocument>;
  load(): Promise<SyncConfigLoadResult>;
  listNotebooks(input: { revision: string }): Promise<RemoteNotebookCatalogEntry[]>;
  loadEditing(): Promise<SyncEditingSnapshot>;
  loadStatus(): Promise<SyncStatus | null>;
  patch(input: {
    expectedRevision: string;
    patch: SyncConfigPatch;
  }): Promise<SyncConfigDocument>;
  recover(input: {
    config: QingYuSyncConfig;
    expectedRevision: string;
  }): Promise<SyncConfigDocument>;
  requestApply(input: SyncApplyUpdate): Promise<SyncApplyWriteResult>;
  reset(input: {
    confirmed: true;
    expectedRevision: string | null;
  }): Promise<SyncConfigDocument>;
  setEditing(input: SyncEditingUpdate): Promise<SyncEditingWriteResult>;
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
