declare const kernelDocumentLocatorBrand: unique symbol;
declare const kernelRevisionBrand: unique symbol;
declare const kernelWorkspaceGenerationBrand: unique symbol;

export type KernelDocumentLocator = string & {
  readonly [kernelDocumentLocatorBrand]: "KernelDocumentLocator";
};

export type KernelRevision = string & {
  readonly [kernelRevisionBrand]: "KernelRevision";
};

export type KernelWorkspaceGeneration = string & {
  readonly [kernelWorkspaceGenerationBrand]: "KernelWorkspaceGeneration";
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

export type KernelDocumentSnapshot = {
  contents: string;
  locator: KernelDocumentLocator;
  modifiedAt: string;
  name: string;
  revision: KernelRevision;
  sizeBytes: number;
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

export type KernelDomainPort = {
  availability: "available" | "unavailable";
  documents: {
    read: (input: KernelReadDocumentInput) => Promise<KernelDocumentSnapshot>;
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
      read: rejectUnavailable,
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
