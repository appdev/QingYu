declare const kernelDocumentLocatorBrand: unique symbol;
declare const kernelHistorySnapshotIdBrand: unique symbol;
declare const kernelPageCursorBrand: unique symbol;
declare const kernelRevisionBrand: unique symbol;
declare const kernelWorkspaceGenerationBrand: unique symbol;
declare const kernelWorkspaceRelativePathBrand: unique symbol;

export type KernelDocumentLocator = string & {
  readonly [kernelDocumentLocatorBrand]: "KernelDocumentLocator";
};

export type KernelHistorySnapshotId = string & {
  readonly [kernelHistorySnapshotIdBrand]: "KernelHistorySnapshotId";
};

export type KernelPageCursor = string & {
  readonly [kernelPageCursorBrand]: "KernelPageCursor";
};

export type KernelRevision = string & {
  readonly [kernelRevisionBrand]: "KernelRevision";
};

export type KernelWorkspaceGeneration = string & {
  readonly [kernelWorkspaceGenerationBrand]: "KernelWorkspaceGeneration";
};

export type KernelWorkspaceRelativePath = string & {
  readonly [kernelWorkspaceRelativePathBrand]: "KernelWorkspaceRelativePath";
};

export type KernelRuntimeCapabilities = {
  documents: boolean;
  history: boolean;
  portableSettings: boolean;
  resources: boolean;
  s3: boolean;
  search: boolean;
  settings: boolean;
  sync: boolean;
  webdav: boolean;
};

export type KernelRuntimeSnapshot = {
  capabilities: KernelRuntimeCapabilities;
  instanceId: string;
  profile: "desktop" | "mobile" | "server";
  startupState:
    | "starting"
    | "needs-owner"
    | "needs-workspace-initialization"
    | "needs-cloud-binding"
    | "ready"
    | "recoverable-error"
    | "fatal-error";
};

export type KernelWorkspaceSnapshot = {
  displayName: string;
  generation: KernelWorkspaceGeneration;
  id: string;
  readiness: "ready" | "initializing" | "unavailable" | "locked";
  revision: KernelRevision;
};

export type KernelDocumentEntrySnapshot = {
  kind: "file" | "directory";
  locator: KernelDocumentLocator;
  modifiedAt: string;
  name: string;
  parent: KernelWorkspaceRelativePath;
  relativePath: KernelWorkspaceRelativePath;
  revision: KernelRevision;
  sizeBytes: number;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelDocumentSnapshot = KernelDocumentEntrySnapshot & {
  contents: string;
  kind: "file";
};

export type KernelCreatedDocumentSnapshot =
  | KernelDocumentSnapshot
  | (KernelDocumentEntrySnapshot & { kind: "directory" });

export type KernelDocumentPageSnapshot = {
  items: KernelDocumentEntrySnapshot[];
  nextCursor: KernelPageCursor | null;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelSearchMatchSnapshot = {
  column: number;
  document: KernelDocumentEntrySnapshot;
  line: number;
  preview: string;
};

export type KernelSearchPageSnapshot = {
  items: KernelSearchMatchSnapshot[];
  nextCursor: KernelPageCursor | null;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelHistoryEntrySnapshot = {
  createdAt: string;
  documentLocator: KernelDocumentLocator;
  revision: KernelRevision;
  sizeBytes: number;
  snapshotId: KernelHistorySnapshotId;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelHistoryPageSnapshot = {
  items: KernelHistoryEntrySnapshot[];
  nextCursor: KernelPageCursor | null;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelReadDocumentHistoryInput = {
  locator: KernelDocumentLocator;
  snapshotId: KernelHistorySnapshotId;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelHistorySnapshot = {
  contents: string;
  documentLocator: KernelDocumentLocator;
  revision: KernelRevision;
  snapshotId: KernelHistorySnapshotId;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelResourceKind = "attachment" | "image";

export type KernelResourceSnapshot = {
  id: string;
  kind: KernelResourceKind;
  mediaType: string;
  modifiedAt: string;
  name: string;
  parent: KernelWorkspaceRelativePath;
  previewable: boolean;
  relativePath: KernelWorkspaceRelativePath;
  revision: KernelRevision;
  sizeBytes: number;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelInventoryEntry =
  | { document: KernelDocumentEntrySnapshot; entryType: "document" }
  | { entryType: "resource"; resource: KernelResourceSnapshot };

export type KernelInventorySnapshot = {
  items: readonly KernelInventoryEntry[];
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelListResourcesInput = {
  parent?: KernelWorkspaceRelativePath;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelOpenResourceInput = {
  id: string;
  kind: KernelResourceKind;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelResourceBody = {
  body: Blob;
  mediaType: string;
};

export type KernelInvalidationScope =
  | "workspace"
  | "documents"
  | "resources"
  | "settings"
  | "sync-config"
  | "sync-status";

export type KernelInvalidationNotice = {
  documentChange?: "content" | "snapshot" | "tree";
  paths?: readonly KernelWorkspaceRelativePath[];
  scopes: readonly KernelInvalidationScope[];
};

export type KernelInvalidationSource = {
  readonly available: boolean;
  readonly subscribe: (
    listener: (notice: KernelInvalidationNotice) => unknown,
  ) => () => unknown;
};

export type KernelPageInput = {
  cursor?: KernelPageCursor;
  limit?: number;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelListDocumentsInput = KernelPageInput & {
  parent?: KernelWorkspaceRelativePath;
};

export type KernelSearchDocumentsInput = KernelPageInput & {
  query: string;
};

export type KernelCreateDocumentInput =
  | {
      contents: string;
      kind: "file";
      name: string;
      parent: KernelWorkspaceRelativePath;
      workspaceGeneration: KernelWorkspaceGeneration;
    }
  | {
      kind: "directory";
      name: string;
      parent: KernelWorkspaceRelativePath;
      workspaceGeneration: KernelWorkspaceGeneration;
    };

export type KernelReadDocumentInput = {
  locator: KernelDocumentLocator;
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelUpdateDocumentInput = KernelReadDocumentInput & {
  contents: string;
  expectedRevision: KernelRevision;
};

export type KernelMoveDocumentInput = KernelReadDocumentInput & {
  expectedRevision: KernelRevision;
  name: string;
  targetParent: KernelWorkspaceRelativePath;
};

export type KernelDeleteDocumentInput = KernelReadDocumentInput & {
  deletionPolicy: "recoverable" | "permanent";
  expectedRevision: KernelRevision;
};

export type KernelListDocumentHistoryInput = KernelPageInput & {
  locator: KernelDocumentLocator;
};

export type KernelRestoreDocumentHistoryInput = KernelReadDocumentInput & {
  expectedRevision: KernelRevision;
  snapshotId: KernelHistorySnapshotId;
};

export type KernelFontFamilyValue =
  | { family: string | null; source: "theme" }
  | { family: string; source: "system" };

export type KernelSettingKey =
  | "appearance.mode"
  | "appearance.lightTheme"
  | "appearance.darkTheme"
  | "theme.customCss.light"
  | "theme.customCss.dark"
  | "language"
  | "editor.bodyFontSize"
  | "editor.contentWidth"
  | "editor.contentWidthPx"
  | "editor.fontFamily"
  | "editor.lineHeight"
  | "editor.paragraphSpacingPx"
  | "editor.showWordCount"
  | "editor.wrapCodeBlocks"
  | "editor.viewMode"
  | "files.ignoreRules"
  | "export.fontFamily"
  | "export.pdfAuthor"
  | "export.pdfFooter"
  | "export.pdfHeader"
  | "export.pdfHeightMm"
  | "export.pdfWidthMm"
  | "export.pdfMarginMm"
  | "export.pdfMarginPreset"
  | "export.pdfPageBreakOnH1"
  | "export.pdfPageSize";

export type KernelSettingValue =
  | { type: "boolean"; value: boolean }
  | { type: "integer"; value: number }
  | { type: "number"; value: number }
  | { type: "string"; value: string }
  | { type: "nullable-integer"; value: number | null }
  | { type: "nullable-string"; value: string | null }
  | { type: "font-family"; value: KernelFontFamilyValue };

export type KernelSettingEntrySnapshot = {
  key: KernelSettingKey;
  value: KernelSettingValue;
};

export type KernelSettingsSnapshot = {
  revision: KernelRevision;
  values: KernelSettingEntrySnapshot[];
};

export type KernelPatchSettingsInput = {
  expectedRevision: KernelRevision;
  values: KernelSettingEntrySnapshot[];
};

export type KernelCredentialChange =
  | { operation: "keep" | "clear" }
  | { operation: "replace"; value: string };

export type KernelSyncProvider = "s3" | "webdav";
export type KernelSyncMode = "automatic" | "startup-exit" | "fully-manual";

export type KernelSyncConfigChangesInput = {
  enabled?: boolean;
  generateConflictDocument?: boolean;
  intervalSeconds?: number;
  mode?: KernelSyncMode;
  provider?: KernelSyncProvider;
  remoteRoot?: string;
  s3AccessKeyId?: KernelCredentialChange;
  s3AddressingStyle?: "auto" | "path" | "virtual-hosted";
  s3Bucket?: string;
  s3EndpointUrl?: string;
  s3Region?: string;
  s3RequestTimeoutSeconds?: number;
  s3SecretAccessKey?: KernelCredentialChange;
  s3TlsVerification?: "verify" | "skip";
  webdavPassword?: KernelCredentialChange;
  webdavServerUrl?: string;
  webdavUsername?: string;
};

export type KernelSyncIssueSnapshot = {
  code: "required" | "invalid-url" | "unsafe-url-components" | "out-of-range" | "invalid-path";
  field: string;
  message: string;
};

export type KernelSyncConfigSnapshot = {
  configured: boolean;
  enabled: boolean;
  generateConflictDocument: boolean;
  intervalSeconds: number;
  issues: KernelSyncIssueSnapshot[];
  mode: KernelSyncMode;
  provider: KernelSyncProvider;
  readiness: "disabled" | "incomplete" | "ready";
  remoteRoot: string;
  revision: KernelRevision;
  s3: {
    accessKeyId: { present: boolean };
    addressingStyle: "auto" | "path" | "virtual-hosted";
    bucket: string;
    endpointUrl: { redacted: boolean; value: string | null };
    region: string;
    requestTimeoutSeconds: number;
    secretAccessKey: { present: boolean };
    tlsVerification: "verify" | "skip";
  };
  webdav: {
    password: { present: boolean };
    serverUrl: { redacted: boolean; value: string | null };
    username: string;
  };
};

export type KernelPatchSyncConfigInput = {
  changes: KernelSyncConfigChangesInput;
  expectedRevision: KernelRevision;
};

export type KernelTestSyncConnectionInput = KernelPatchSyncConfigInput;

export type KernelSyncConnectionTestSnapshot = {
  checkedTarget: string;
  configRevision: KernelRevision;
  provider: KernelSyncProvider;
};

export type KernelSyncSummarySnapshot = {
  bytesDownloaded: number;
  bytesUploaded: number;
  conflictFiles: number;
  downloadedFiles: number;
  scannedFiles: number;
  skippedFiles: number;
  uploadedFiles: number;
};

export type KernelSyncSafeErrorSnapshot = {
  category?: string;
  code: string;
  httpStatus?: number;
  method?: string;
  operation: string;
  provider: KernelSyncProvider;
  providerErrorCode?: string;
  relativePath?: KernelWorkspaceRelativePath;
  requestId?: string;
  runId?: string;
};

export type KernelSyncStatusSnapshot = {
  activeRunId: string | null;
  completionState: "idle" | "attempting" | "failed" | "succeeded";
  configRevision: KernelRevision | null;
  error: KernelSyncSafeErrorSnapshot | null;
  lastAttemptAt: string | null;
  lastSuccessfulSyncAt: string | null;
  lastTrigger: "app-launch" | "interval" | "manual" | "save" | "settings-exit" | null;
  provider: KernelSyncProvider;
  summary: KernelSyncSummarySnapshot | null;
};

export type KernelSyncRunSnapshot = {
  acceptedAt: string;
  configRevision: KernelRevision;
  runId: string;
};

export type KernelDomainPort = {
  availability: "available" | "unavailable";
  documents: {
    create: (input: KernelCreateDocumentInput) => Promise<KernelCreatedDocumentSnapshot>;
    delete: (input: KernelDeleteDocumentInput) => Promise<undefined>;
    history: {
      list: (input: KernelListDocumentHistoryInput) => Promise<KernelHistoryPageSnapshot>;
      read?: (input: KernelReadDocumentHistoryInput) => Promise<KernelHistorySnapshot>;
      restore: (input: KernelRestoreDocumentHistoryInput) => Promise<KernelDocumentSnapshot>;
    };
    list: (input: KernelListDocumentsInput) => Promise<KernelDocumentPageSnapshot>;
    move: (input: KernelMoveDocumentInput) => Promise<KernelDocumentEntrySnapshot>;
    read: (input: KernelReadDocumentInput) => Promise<KernelDocumentSnapshot>;
    search: (input: KernelSearchDocumentsInput) => Promise<KernelSearchPageSnapshot>;
    update: (input: KernelUpdateDocumentInput) => Promise<KernelDocumentSnapshot>;
  };
  invalidations?: KernelInvalidationSource;
  resources?: {
    list: (input: KernelListResourcesInput) => Promise<KernelInventorySnapshot>;
    open: (input: KernelOpenResourceInput) => Promise<KernelResourceBody>;
  };
  runtime: {
    read: () => Promise<KernelRuntimeSnapshot>;
  };
  settings: {
    patch: (input: KernelPatchSettingsInput) => Promise<KernelSettingsSnapshot>;
    read: () => Promise<KernelSettingsSnapshot>;
  };
  sync: {
    patchConfig: (input: KernelPatchSyncConfigInput) => Promise<KernelSyncConfigSnapshot>;
    readConfig: () => Promise<KernelSyncConfigSnapshot>;
    readStatus: () => Promise<KernelSyncStatusSnapshot>;
    testConnection: (
      input: KernelTestSyncConnectionInput,
    ) => Promise<KernelSyncConnectionTestSnapshot>;
    trigger: (expectedConfigRevision: KernelRevision) => Promise<KernelSyncRunSnapshot>;
  };
  workspace: {
    read: () => Promise<KernelWorkspaceSnapshot>;
  };
};

export class KernelDomainUnavailableError extends Error {
  constructor() {
    super("The Kernel domain is unavailable without an installed adapter.");
    this.name = "KernelDomainUnavailableError";
  }
}

function rejectUnavailable<T>(): Promise<T> {
  return Promise.reject(new KernelDomainUnavailableError());
}

export function createUnavailableKernelDomainPort(): KernelDomainPort {
  return {
    availability: "unavailable",
    documents: {
      create: rejectUnavailable,
      delete: rejectUnavailable,
      history: {
        list: rejectUnavailable,
        read: rejectUnavailable,
        restore: rejectUnavailable,
      },
      list: rejectUnavailable,
      move: rejectUnavailable,
      read: rejectUnavailable,
      search: rejectUnavailable,
      update: rejectUnavailable,
    },
    invalidations: {
      available: false,
      subscribe: () => () => undefined,
    },
    resources: {
      list: rejectUnavailable,
      open: rejectUnavailable,
    },
    runtime: {
      read: rejectUnavailable,
    },
    settings: {
      patch: rejectUnavailable,
      read: rejectUnavailable,
    },
    sync: {
      patchConfig: rejectUnavailable,
      readConfig: rejectUnavailable,
      readStatus: rejectUnavailable,
      testConnection: rejectUnavailable,
      trigger: rejectUnavailable,
    },
    workspace: {
      read: rejectUnavailable,
    },
  };
}
