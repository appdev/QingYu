declare const nativeAbsolutePathBrand: unique symbol;
declare const nativeStandaloneDocumentHandleBrand: unique symbol;
declare const nativeStandaloneRevisionBrand: unique symbol;

export type NativeAbsolutePath = string & {
  readonly [nativeAbsolutePathBrand]: "NativeAbsolutePath";
};

export type NativeStandaloneDocumentHandle = string & {
  readonly [nativeStandaloneDocumentHandleBrand]: "NativeStandaloneDocumentHandle";
};

export type NativeStandaloneRevision = string & {
  readonly [nativeStandaloneRevisionBrand]: "NativeStandaloneRevision";
};

export type NativeCapabilityAvailability = "available" | "unavailable";

export type NativeShellCapabilities = {
  absolutePathClassification: NativeCapabilityAvailability;
  operatingSystemShell: NativeCapabilityAvailability;
  pickers: NativeCapabilityAvailability;
  standaloneDocuments: NativeCapabilityAvailability;
};

export type NativeWorkspaceDirectorySelection = {
  absolutePath: NativeAbsolutePath;
  displayName: string;
};

export type NativeStandaloneDocumentSelection = {
  displayName: string;
  handle: NativeStandaloneDocumentHandle;
};

export type NativeAbsolutePathClassification =
  | {
      kind: "standalone-document";
      handle: NativeStandaloneDocumentHandle;
    }
  | {
      kind: "workspace-directory";
      absolutePath: NativeAbsolutePath;
    }
  | {
      kind: "unsupported";
    };

export type NativeStandaloneDocumentSnapshot = {
  contents: string;
  displayName: string;
  handle: NativeStandaloneDocumentHandle;
  revision: NativeStandaloneRevision;
};

export type NativeStandaloneWriteInput = {
  contents: string;
  expectedRevision: NativeStandaloneRevision;
  handle: NativeStandaloneDocumentHandle;
};

export type NativeShellPort = {
  capabilities: NativeShellCapabilities;
  operatingSystem: {
    openExternalUrl: (url: string) => Promise<unknown>;
    revealAbsolutePath: (path: NativeAbsolutePath) => Promise<unknown>;
  };
  paths: {
    classify: (path: NativeAbsolutePath) => Promise<NativeAbsolutePathClassification>;
  };
  pickers: {
    pickStandaloneDocument: () => Promise<NativeStandaloneDocumentSelection | null>;
    pickWorkspaceDirectory: () => Promise<NativeWorkspaceDirectorySelection | null>;
  };
  standalone: {
    read: (handle: NativeStandaloneDocumentHandle) => Promise<NativeStandaloneDocumentSnapshot>;
    write: (input: NativeStandaloneWriteInput) => Promise<NativeStandaloneDocumentSnapshot>;
  };
};

export class NativeShellUnavailableError extends Error {
  constructor() {
    super("Native shell capabilities are unavailable without an installed adapter.");
    this.name = "NativeShellUnavailableError";
  }
}

function rejectUnavailable<T>(): Promise<T> {
  return Promise.reject(new NativeShellUnavailableError());
}

export function createUnavailableNativeShellPort(): NativeShellPort {
  return {
    capabilities: {
      absolutePathClassification: "unavailable",
      operatingSystemShell: "unavailable",
      pickers: "unavailable",
      standaloneDocuments: "unavailable",
    },
    operatingSystem: {
      openExternalUrl: rejectUnavailable,
      revealAbsolutePath: rejectUnavailable,
    },
    paths: {
      classify: rejectUnavailable,
    },
    pickers: {
      pickStandaloneDocument: rejectUnavailable,
      pickWorkspaceDirectory: rejectUnavailable,
    },
    standalone: {
      read: rejectUnavailable,
      write: rejectUnavailable,
    },
  };
}
