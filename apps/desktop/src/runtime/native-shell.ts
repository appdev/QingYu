import type {
  NativeAbsolutePath,
  NativeShellPort,
  NativeStandaloneDocumentHandle,
  NativeStandaloneDocumentSnapshot,
  NativeStandaloneRevision
} from "@markra/app/runtime";
import * as desktopFiles from "./tauri/file/desktop";
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
  readMarkdownFile: (path: string) => Promise<NativeMarkdownFile>;
  resolveMarkdownFolder: (path: string) => Promise<NativeMarkdownFolder>;
  resolveMarkdownPath: (path: string) => Promise<NativeResolvedPath>;
  saveMarkdownFile: (input: {
    contents: string;
    path: string | null;
    suggestedName: string;
  }) => Promise<{ name: string; path: string } | null>;
};

type StandaloneRecord = {
  displayName: string;
  path: string;
};

const defaultDependencies: DesktopNativeShellDependencies = {
  newHandle: () => globalThis.crypto.randomUUID(),
  openContainingFolder: desktopFiles.openNativeContainingFolder,
  openExternalUrl: openNativeExternalUrl,
  openMarkdownFile: desktopFiles.openNativeMarkdownFile,
  openMarkdownFolder: desktopFiles.openNativeMarkdownFolder,
  readMarkdownFile: desktopFiles.readNativeMarkdownFile,
  resolveMarkdownFolder: desktopFiles.resolveNativeMarkdownFolder,
  resolveMarkdownPath: desktopFiles.resolveNativeMarkdownPath,
  saveMarkdownFile: desktopFiles.saveNativeMarkdownFile
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

async function standaloneRevision(contents: string): Promise<NativeStandaloneRevision> {
  const digest = await globalThis.crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(contents)
  );
  const fingerprint = Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, "0")
  ).join("");
  return `native-v1-${fingerprint}` as NativeStandaloneRevision;
}

export function createDesktopNativeShellPort(
  dependencies: DesktopNativeShellDependencies = defaultDependencies
): NativeShellPort {
  const records = new Map<NativeStandaloneDocumentHandle, StandaloneRecord>();
  const writeQueues = new Map<NativeStandaloneDocumentHandle, Promise<unknown>>();

  function register(file: NativeMarkdownFile): NativeStandaloneDocumentHandle {
    for (let attempt = 0; attempt < 16; attempt += 1) {
      const handle = dependencies.newHandle() as NativeStandaloneDocumentHandle;
      if (!handle.trim() || records.has(handle)) continue;
      records.set(handle, { displayName: file.name, path: file.path });
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
    const file = await dependencies.readMarkdownFile(record.path).catch(() => {
      throw new NativeStandaloneDocumentUnavailableError();
    });
    record.displayName = file.name;
    return {
      contents: file.content,
      displayName: file.name,
      handle,
      revision: await standaloneRevision(file.content)
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
        const file = await dependencies.readMarkdownFile(target.path).catch(() => null);
        return file
          ? { kind: "standalone-document", handle: register(file) }
          : { kind: "unsupported" };
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
      write: (input) => writeSerially(input.handle, async () => {
        const record = recordFor(input.handle);
        const current = await read(input.handle);
        if (current.revision !== input.expectedRevision) {
          throw new NativeStandaloneConflictError();
        }
        const saved = await dependencies.saveMarkdownFile({
          contents: input.contents,
          path: record.path,
          suggestedName: record.displayName
        }).catch(() => {
          throw new NativeStandaloneDocumentUnavailableError();
        });
        if (!saved) throw new NativeStandaloneDocumentUnavailableError();
        record.displayName = saved.name;
        record.path = saved.path;
        return read(input.handle);
      })
    }
  };
}
