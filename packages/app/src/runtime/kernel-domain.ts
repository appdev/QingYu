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

export type KernelDomainPort = {
  availability: "available" | "unavailable";
  documents: {
    create: (input: KernelCreateDocumentInput) => Promise<KernelCreatedDocumentSnapshot>;
    delete: (input: KernelDeleteDocumentInput) => Promise<undefined>;
    history: {
      list: (input: KernelListDocumentHistoryInput) => Promise<KernelHistoryPageSnapshot>;
      restore: (input: KernelRestoreDocumentHistoryInput) => Promise<KernelDocumentSnapshot>;
    };
    list: (input: KernelListDocumentsInput) => Promise<KernelDocumentPageSnapshot>;
    move: (input: KernelMoveDocumentInput) => Promise<KernelDocumentEntrySnapshot>;
    read: (input: KernelReadDocumentInput) => Promise<KernelDocumentSnapshot>;
    search: (input: KernelSearchDocumentsInput) => Promise<KernelSearchPageSnapshot>;
    update: (input: KernelUpdateDocumentInput) => Promise<KernelDocumentSnapshot>;
  };
  runtime: {
    read: () => Promise<KernelRuntimeSnapshot>;
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
        restore: rejectUnavailable,
      },
      list: rejectUnavailable,
      move: rejectUnavailable,
      read: rejectUnavailable,
      search: rejectUnavailable,
      update: rejectUnavailable,
    },
    runtime: {
      read: rejectUnavailable,
    },
    workspace: {
      read: rejectUnavailable,
    },
  };
}
