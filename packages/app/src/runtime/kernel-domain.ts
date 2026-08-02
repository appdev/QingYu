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

export function hasRequiredKernelDomainCapabilities(
  capabilities: KernelRuntimeCapabilities,
): boolean {
  return (
    capabilities.documents === true &&
    capabilities.history === true &&
    capabilities.portableSettings === true &&
    capabilities.resources === true &&
    capabilities.search === true &&
    capabilities.settings === true &&
    capabilities.sync === true
  );
}

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
export type KernelImageMediaType =
  | "image/avif"
  | "image/bmp"
  | "image/gif"
  | "image/jpeg"
  | "image/png"
  | "image/svg+xml"
  | "image/webp";

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

export type KernelCreateResourceInput = {
  body: Blob;
  documentLocator: KernelDocumentLocator;
  folder: KernelWorkspaceRelativePath;
  name: string;
  workspaceGeneration: KernelWorkspaceGeneration;
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

export type KernelCreateResourceBatchInput = {
  batchId: string;
  documentLocator: KernelDocumentLocator;
  folder: KernelWorkspaceRelativePath;
  items: readonly {
    body: Blob;
    kind: "image";
    mediaType: KernelImageMediaType;
    name: string;
  }[];
  workspaceGeneration: KernelWorkspaceGeneration;
};

export type KernelResourceBody = {
  body: Blob;
  mediaType: string;
  revision?: KernelRevision;
};

export type KernelInvalidationScope =
  | "workspace"
  | "documents"
  | "resources"
  | "settings"
  | "app-config"
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

export type KernelStoredWorkspaceDraft = {
  content: string;
  creationDirectory?: KernelWorkspaceRelativePath | null;
  id: string;
  name: string;
  path: KernelWorkspaceRelativePath | null;
};

export type KernelStoredWorkspaceSplitGroup = {
  primaryFilePath: KernelWorkspaceRelativePath;
  sideFilePath: KernelWorkspaceRelativePath;
};

export type KernelStoredWorkspaceWindowState = {
  activeDraftId: string | null;
  draftTabs: readonly KernelStoredWorkspaceDraft[];
  filePath: KernelWorkspaceRelativePath | null;
  fileTreeAssetsVisible: boolean;
  fileTreeOpen: boolean;
  folderName: string | null;
  folderPath: KernelWorkspaceRelativePath | null;
  openFilePaths: readonly KernelWorkspaceRelativePath[];
  sideBySideGroup: KernelStoredWorkspaceSplitGroup | null;
};

export type KernelStoredWorkspaceWindow = {
  filePath: KernelWorkspaceRelativePath | null;
  label: string;
  openFilePaths: readonly KernelWorkspaceRelativePath[];
};

export type KernelStoredWorkspaceLayout = {
  schemaVersion: 1;
  windowStates: Readonly<Record<string, KernelStoredWorkspaceWindowState>>;
  openWindows: readonly KernelStoredWorkspaceWindow[];
};

export type KernelRecentMarkdownFile = {
  name: string;
  path: KernelWorkspaceRelativePath;
};

export type KernelStoredFileTreeSort = {
  direction: "ascending" | "descending";
  key: "createdAt" | "modifiedAt" | "name";
};

export type KernelWorkspaceLayoutPatch = Partial<{
  activeDraftId: string | null;
  draftTabs: readonly KernelStoredWorkspaceDraft[];
  filePath: KernelWorkspaceRelativePath | null;
  fileTreeAssetsVisible: boolean;
  fileTreeOpen: boolean;
  folderName: string | null;
  folderPath: KernelWorkspaceRelativePath | null;
  openFilePaths: readonly KernelWorkspaceRelativePath[];
  openWindows: readonly KernelStoredWorkspaceWindow[];
  sideBySideGroup: KernelStoredWorkspaceSplitGroup | null;
}>;

export type KernelAppConfigSnapshot = {
  appConfigVersion: 1;
  workspace: { id: string; generation: KernelWorkspaceGeneration };
  settings: KernelSettingsSnapshot;
  localState: {
    revision: KernelRevision;
    uiLayout: KernelStoredWorkspaceLayout;
    recentMarkdownFiles: readonly KernelRecentMarkdownFile[];
    fileTreeSort: KernelStoredFileTreeSort;
    pandocPath: string | null;
  };
};

export type KernelAppConfigStateOperation =
  | {
      patch: KernelWorkspaceLayoutPatch;
      type: "patch-ui-layout";
      windowLabel: string;
    }
  | { file: KernelRecentMarkdownFile; type: "remember-recent-file" }
  | { path: KernelWorkspaceRelativePath; type: "remove-recent-file" }
  | { type: "clear-recent-files" }
  | { sort: KernelStoredFileTreeSort; type: "set-file-tree-sort" }
  | { path: string | null; type: "set-pandoc-path" };

export type KernelPatchAppConfigStateInput = {
  operations: readonly KernelAppConfigStateOperation[];
  workspaceGeneration: KernelWorkspaceGeneration;
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

export type KernelSyncRunStatusSnapshot = {
  acceptedAt: string;
  completionState: "attempting" | "failed" | "succeeded";
  configRevision: KernelRevision;
  error: KernelSyncSafeErrorSnapshot | null;
  finishedAt: string | null;
  provider: KernelSyncProvider;
  runId: string;
  summary: KernelSyncSummarySnapshot | null;
};

export type KernelRemoteNotebookSnapshot = {
  available: boolean;
  disabledReason: string | null;
  displayName: string;
  name: string;
  provider: "s3";
  repositoryId: string;
};

export type KernelSyncRepositoryBindingSnapshot = {
  jobId: string;
  repositoryId: string;
};

export type KernelDomainPort = {
  appConfig: {
    readonly bootstrap: KernelAppConfigSnapshot;
    read: () => Promise<KernelAppConfigSnapshot>;
    patchState: (input: KernelPatchAppConfigStateInput) => Promise<KernelAppConfigSnapshot>;
  };
  availability: "available" | "unavailable";
  documents: {
    create: (input: KernelCreateDocumentInput) => Promise<KernelCreatedDocumentSnapshot>;
    delete: (input: KernelDeleteDocumentInput) => Promise<undefined>;
    history: {
      list: (input: KernelListDocumentHistoryInput) => Promise<KernelHistoryPageSnapshot>;
      read: (input: KernelReadDocumentHistoryInput) => Promise<KernelHistorySnapshot>;
      restore: (input: KernelRestoreDocumentHistoryInput) => Promise<KernelDocumentSnapshot>;
    };
    list: (input: KernelListDocumentsInput) => Promise<KernelDocumentPageSnapshot>;
    move: (input: KernelMoveDocumentInput) => Promise<KernelDocumentEntrySnapshot>;
    read: (input: KernelReadDocumentInput) => Promise<KernelDocumentSnapshot>;
    search: (input: KernelSearchDocumentsInput) => Promise<KernelSearchPageSnapshot>;
    update: (input: KernelUpdateDocumentInput) => Promise<KernelDocumentSnapshot>;
  };
  invalidations: KernelInvalidationSource;
  resources: {
    create: (input: KernelCreateResourceInput) => Promise<KernelResourceSnapshot>;
    createBatch: (input: KernelCreateResourceBatchInput) => Promise<readonly KernelResourceSnapshot[]>;
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
    bindRepository: (input: {
      displayName: string;
      expectedRevision: KernelRevision;
      repositoryId: string;
    }) => Promise<KernelSyncRepositoryBindingSnapshot>;
    exportKey: () => Promise<string>;
    importKey: (key: string) => Promise<{ configured: boolean }>;
    listNotebooks: (expectedRevision: KernelRevision) => Promise<KernelRemoteNotebookSnapshot[]>;
    readKeyState: () => Promise<{ configured: boolean }>;
    patchConfig: (input: KernelPatchSyncConfigInput) => Promise<KernelSyncConfigSnapshot>;
    readConfig: () => Promise<KernelSyncConfigSnapshot>;
    readRun: (runId: string) => Promise<KernelSyncRunStatusSnapshot>;
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
    appConfig: {
      get bootstrap(): never {
        throw new KernelDomainUnavailableError();
      },
      patchState: rejectUnavailable,
      read: rejectUnavailable,
    },
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
      create: rejectUnavailable,
      createBatch: rejectUnavailable,
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
      bindRepository: rejectUnavailable,
      exportKey: rejectUnavailable,
      importKey: rejectUnavailable,
      listNotebooks: rejectUnavailable,
      patchConfig: rejectUnavailable,
      readKeyState: rejectUnavailable,
      readConfig: rejectUnavailable,
      readRun: rejectUnavailable,
      readStatus: rejectUnavailable,
      testConnection: rejectUnavailable,
      trigger: rejectUnavailable,
    },
    workspace: {
      read: rejectUnavailable,
    },
  };
}
