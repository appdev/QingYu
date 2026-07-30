import type {
  NativeAbsolutePath,
  NativeShellPort,
  NativeStandaloneDocumentHandle,
  NativeStandaloneDocumentSnapshot,
  NativeStandaloneRevision
} from "@markra/app/runtime";
import * as desktopFiles from "./tauri/file/desktop";
import { invokeNative } from "./tauri/invoke";
import { openNativeExternalUrl } from "./tauri/opener";

type NativeMarkdownFile = {
  content: string;
  name: string;
  path: string;
  sizeBytes?: number;
};

type NativeMarkdownFolder = {
  name: string;
  path: string;
};

type NativeResolvedPath = {
  kind: "file" | "folder" | "image";
  name: string;
  path: string;
};

export type DesktopNativeShellDependencies = {
  newHandle: () => string;
  openContainingFolder: (path: string) => Promise<unknown>;
  openExternalUrl: (url: string) => Promise<unknown>;
  openMarkdownFile: () => Promise<NativeMarkdownFile | null>;
  openMarkdownFolder: () => Promise<NativeMarkdownFolder | null>;
  readStandaloneDocument: (path: string) => Promise<NativeStandaloneDocumentData>;
  resolveMarkdownFolder: (path: string) => Promise<NativeMarkdownFolder>;
  resolveMarkdownPath: (path: string) => Promise<NativeResolvedPath>;
  writeStandaloneDocumentCas: (input: {
    contents: string;
    expectedRevision: NativeStandaloneRevision;
    path: string;
  }) => Promise<NativeStandaloneDocumentData>;
};

type NativeStandaloneDocumentData = {
  contents: string;
  displayName: string;
  revision: NativeStandaloneRevision;
};

type NativeStandaloneDocumentResponse = {
  contents: unknown;
  displayName: unknown;
  revision: unknown;
};

type StandaloneRecord = {
  displayName: string;
  path: string;
};

const maxStandaloneDocumentHandles = 256;

function standaloneDocumentDataFromResponse(
  response: NativeStandaloneDocumentResponse
): NativeStandaloneDocumentData {
  if (
    typeof response.contents !== "string" ||
    typeof response.displayName !== "string" ||
    typeof response.revision !== "string" ||
    !/^native-v2-[0-9a-f]{64}$/.test(response.revision)
  ) {
    throw new NativeStandaloneDocumentUnavailableError();
  }
  return {
    contents: response.contents,
    displayName: response.displayName,
    revision: response.revision as NativeStandaloneRevision
  };
}

async function readStandaloneDocument(path: string): Promise<NativeStandaloneDocumentData> {
  const response = await invokeNative<NativeStandaloneDocumentResponse>(
    "read_standalone_document",
    { path }
  );
  return standaloneDocumentDataFromResponse(response);
}

async function writeStandaloneDocumentCas(input: {
  contents: string;
  expectedRevision: NativeStandaloneRevision;
  path: string;
}): Promise<NativeStandaloneDocumentData> {
  const response = await invokeNative<NativeStandaloneDocumentResponse>(
    "write_standalone_document_cas",
    input
  );
  return standaloneDocumentDataFromResponse(response);
}

const defaultDependencies: DesktopNativeShellDependencies = {
  newHandle: () => globalThis.crypto.randomUUID(),
  openContainingFolder: desktopFiles.openNativeContainingFolder,
  openExternalUrl: openNativeExternalUrl,
  openMarkdownFile: desktopFiles.openNativeMarkdownFile,
  openMarkdownFolder: desktopFiles.openNativeMarkdownFolder,
  readStandaloneDocument,
  resolveMarkdownFolder: desktopFiles.resolveNativeMarkdownFolder,
  resolveMarkdownPath: desktopFiles.resolveNativeMarkdownPath,
  writeStandaloneDocumentCas
};

export class NativeStandaloneDocumentUnavailableError extends Error {
  constructor() {
    super("The standalone document is unavailable.");
    this.name = "NativeStandaloneDocumentUnavailableError";
  }
}

export class NativeStandaloneConflictError extends Error {
  constructor() {
    super("The standalone document changed before it could be saved.");
    this.name = "NativeStandaloneConflictError";
  }
}

function nativeWriteError(error: unknown): Error {
  if (error === "standalone-document-conflict") {
    return new NativeStandaloneConflictError();
  }
  return new NativeStandaloneDocumentUnavailableError();
}

export function createDesktopNativeShellPort(
  dependencies: DesktopNativeShellDependencies = defaultDependencies
): NativeShellPort {
  const records = new Map<NativeStandaloneDocumentHandle, StandaloneRecord>();
  const handlesByPath = new Map<string, NativeStandaloneDocumentHandle>();
  const writeQueues = new Map<NativeStandaloneDocumentHandle, Promise<unknown>>();

  function register(file: NativeMarkdownFile): NativeStandaloneDocumentHandle {
    const existing = handlesByPath.get(file.path);
    if (existing && records.has(existing)) return existing;
    if (records.size >= maxStandaloneDocumentHandles) {
      throw new NativeStandaloneDocumentUnavailableError();
    }
    for (let attempt = 0; attempt < 16; attempt += 1) {
      const handle = dependencies.newHandle() as NativeStandaloneDocumentHandle;
      if (!handle.trim() || records.has(handle)) continue;
      records.set(handle, { displayName: file.name, path: file.path });
      handlesByPath.set(file.path, handle);
      return handle;
    }
    throw new NativeStandaloneDocumentUnavailableError();
  }

  function recordFor(handle: NativeStandaloneDocumentHandle): StandaloneRecord {
    const record = records.get(handle);
    if (!record) throw new NativeStandaloneDocumentUnavailableError();
    return record;
  }

  async function read(handle: NativeStandaloneDocumentHandle): Promise<NativeStandaloneDocumentSnapshot> {
    const record = recordFor(handle);
    const file = await dependencies.readStandaloneDocument(record.path).catch(() => {
      throw new NativeStandaloneDocumentUnavailableError();
    });
    record.displayName = file.displayName;
    return {
      contents: file.contents,
      displayName: file.displayName,
      handle,
      revision: file.revision
    };
  }

  async function writeSerially<T>(
    handle: NativeStandaloneDocumentHandle,
    operation: () => Promise<T>
  ): Promise<T> {
    const previous = writeQueues.get(handle) ?? Promise.resolve();
    const current = previous.catch(() => undefined).then(operation);
    writeQueues.set(handle, current);
    try {
      return await current;
    } finally {
      if (writeQueues.get(handle) === current) writeQueues.delete(handle);
    }
  }

  return {
    capabilities: {
      absolutePathClassification: "available",
      operatingSystemShell: "available",
      pickers: "available",
      standaloneDocuments: "available"
    },
    operatingSystem: {
      openExternalUrl: dependencies.openExternalUrl,
      revealAbsolutePath: (path) => dependencies.openContainingFolder(path)
    },
    paths: {
      async classify(path: NativeAbsolutePath) {
        const target = await dependencies.resolveMarkdownPath(path).catch(() => null);
        if (!target) return { kind: "unsupported" };
        if (target.kind === "folder") {
          const folder = await dependencies.resolveMarkdownFolder(target.path).catch(() => null);
          return folder
            ? { kind: "workspace-directory", absolutePath: folder.path as NativeAbsolutePath }
            : { kind: "unsupported" };
        }
        if (target.kind !== "file") return { kind: "unsupported" };
        return {
          kind: "standalone-document",
          handle: register({ content: "", name: target.name, path: target.path })
        };
      }
    },
    pickers: {
      async pickStandaloneDocument() {
        const file = await dependencies.openMarkdownFile();
        if (!file) return null;
        return { displayName: file.name, handle: register(file) };
      },
      async pickWorkspaceDirectory() {
        const folder = await dependencies.openMarkdownFolder();
        if (!folder) return null;
        return {
          absolutePath: folder.path as NativeAbsolutePath,
          displayName: folder.name
        };
      }
    },
    standalone: {
      read,
      async release(handle) {
        const record = records.get(handle);
        if (!record) return;
        records.delete(handle);
        if (handlesByPath.get(record.path) === handle) handlesByPath.delete(record.path);
      },
      write: (input) => writeSerially(input.handle, async () => {
        const record = recordFor(input.handle);
        const saved = await dependencies.writeStandaloneDocumentCas({
          contents: input.contents,
          expectedRevision: input.expectedRevision,
          path: record.path
        }).catch((error: unknown) => {
          throw nativeWriteError(error);
        });
        record.displayName = saved.displayName;
        return {
          ...saved,
          handle: input.handle
        };
      })
    }
  };
}
