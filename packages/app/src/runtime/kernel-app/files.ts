import type {
  AppFileRuntime,
  KernelDocumentEntrySnapshot,
  KernelDocumentLocator,
  KernelDomainPort,
  KernelHistorySnapshotId,
  KernelInvalidationNotice,
  KernelInvalidationSource,
  KernelPageCursor,
  KernelResourceBody,
  KernelResourceSnapshot,
  KernelRevision,
  KernelWorkspaceGeneration,
  KernelWorkspaceRelativePath,
  NativeMarkdownFolderFile,
  WorkspaceSearchResponse,
} from "../index";

export const kernelWorkspaceRoot = "kernel-workspace://primary";

export type KernelFileNativeShellFallback = Partial<Pick<AppFileRuntime,
  | "confirmMarkdownFileDelete"
  | "confirmUnsavedMarkdownDocumentDiscard"
  | "detectPandocPath"
  | "installMarkdownFileDrop"
  | "listenOpenedMarkdownPaths"
  | "openLocalFiles"
  | "openLocalImages"
  | "openMarkdownFile"
  | "openSettingsFile"
  | "readMarkdownTemplateFile"
  | "saveHtmlFile"
  | "savePandocFile"
  | "savePdfFile"
  | "saveSettingsFile"
  | "takeOpenedMarkdownPaths"
>>;

export interface KernelFileRuntimeOptions {
  readonly invalidations?: KernelInvalidationSource;
  readonly isTerminalError?: (error: unknown) => boolean;
  readonly nativeShell?: KernelFileNativeShellFallback;
  readonly pollIntervalMs?: number;
  readonly resolveImageSrc?: (resource: KernelResourceSnapshot) => string | undefined;
  readonly setInterval?: typeof globalThis.setInterval;
  readonly clearInterval?: typeof globalThis.clearInterval;
}

export interface KernelImageSource {
  readonly materialize: (
    resource: KernelResourceSnapshot,
    open: () => Promise<KernelResourceBody>,
  ) => Promise<string | undefined>;
  readonly materializeCreated?: (
    resource: KernelResourceSnapshot,
    body: KernelResourceBody,
  ) => Promise<string | undefined>;
  readonly release: (source: string) => unknown;
}

export interface KernelFileRuntimeOwnerOptions extends KernelFileRuntimeOptions {
  readonly imageSource?: KernelImageSource;
}

export interface KernelFileRuntimeOwner {
  readonly files: AppFileRuntime;
  readonly release: () => undefined;
}

type WorkspaceIdentity = {
  readonly displayName: string;
  readonly generation: KernelWorkspaceGeneration;
};

type CachedEntry = KernelDocumentEntrySnapshot;

type CachedImageResource = KernelResourceSnapshot;

const previewableImageMediaTypes = new Set([
  "image/gif",
  "image/jpeg",
  "image/png",
  "image/webp",
]);

export function createKernelFileRuntime(
  kernel: KernelDomainPort,
  options: KernelFileRuntimeOptions = {},
): AppFileRuntime {
  return createKernelFileRuntimeOwner(kernel, options).files;
}

export function createKernelFileRuntimeOwner(
  kernel: KernelDomainPort,
  options: KernelFileRuntimeOwnerOptions = {},
): KernelFileRuntimeOwner {
  const fallback = createKernelFileFallback(options.nativeShell);
  const entries = new Map<string, CachedEntry>();
  const invalidations = options.invalidations ?? kernel.invalidations;
  const resources = kernel.resources;
  let workspaceIdentity: Promise<WorkspaceIdentity> | undefined;
  let imageResources = new Map<string, CachedImageResource>();
  let imageSources = new Map<string, string>();
  let imageResourceCacheGeneration: KernelWorkspaceGeneration | undefined;
  let imageResourceCacheReady = false;
  let imageResourceEpoch = 0;
  let imageResourcePrewarm: Promise<void> | undefined;
  let released = false;

  const workspace = async () => {
    workspaceIdentity ??= kernel.workspace.read().then((snapshot) => ({
      displayName: snapshot.displayName,
      generation: snapshot.generation,
    }));
    return workspaceIdentity;
  };

  const listDirectory = async (
    parent: KernelWorkspaceRelativePath,
    signal?: AbortSignal | null,
  ) => {
    const identity = await workspace();
    let cursor: KernelPageCursor | undefined;
    const listed: KernelDocumentEntrySnapshot[] = [];
    do {
      assertNotAborted(signal);
      const page = await kernel.documents.list({
        cursor,
        limit: 100,
        parent,
        workspaceGeneration: identity.generation,
      });
      listed.push(...page.items);
      page.items.forEach(cacheEntry);
      cursor = page.nextCursor ?? undefined;
    } while (cursor !== undefined);
    return listed;
  };

  const listTree = async (relativeRoot: string, signal?: AbortSignal | null) => {
    const result: KernelDocumentEntrySnapshot[] = [];
    const pending = [relativeRoot as KernelWorkspaceRelativePath];
    while (pending.length > 0) {
      const parent = pending.shift()!;
      const children = await listDirectory(parent, signal);
      result.push(...children);
      children.forEach((entry) => {
        if (entry.kind === "directory") pending.push(entry.relativePath);
      });
    }
    return result;
  };

  const refreshImageResources = async (
    relativeRoot: string,
    treeEntries: readonly KernelDocumentEntrySnapshot[],
    signal?: AbortSignal | null,
    expectedEpoch = imageResourceEpoch,
  ) => {
    const identity = await workspace();
    if (resources === undefined) {
      if (expectedEpoch === imageResourceEpoch) {
        imageResources = new Map();
        imageResourceCacheGeneration = identity.generation;
        imageResourceCacheReady = true;
      }
      return;
    }
    const directories = new Set<KernelWorkspaceRelativePath>([
      relativeRoot as KernelWorkspaceRelativePath,
      ...treeEntries
        .filter((entry) => entry.kind === "directory")
        .map((entry) => entry.relativePath),
    ]);
    const next = new Map<string, CachedImageResource>();
    for (const parent of directories) {
      assertNotAborted(signal);
      const inventory = await resources.list({
        parent,
        workspaceGeneration: identity.generation,
      });
      if (inventory.workspaceGeneration !== identity.generation) {
        throw new Error("The Kernel workspace generation changed.");
      }
      for (const entry of inventory.items) {
        if (entry.entryType !== "resource") continue;
        const resource = entry.resource;
        if (
          resource.workspaceGeneration !== identity.generation ||
          resource.parent !== parent ||
          resource.kind !== "image" ||
          resource.previewable !== true ||
          !previewableImageMediaTypes.has(resource.mediaType)
        ) continue;
        next.set(resource.relativePath, resource);
      }
    }
    const nextSources = new Map<string, string>();
    const createdSources = new Set<string>();
    if (options.imageSource !== undefined) {
      try {
        for (const [relativePath, resource] of next) {
          assertNotAborted(signal);
          const previous = imageResources.get(relativePath);
          const previousSource = imageSources.get(relativePath);
          if (
            previousSource !== undefined &&
            previous !== undefined &&
            sameResourceVersion(previous, resource)
          ) {
            nextSources.set(relativePath, previousSource);
            continue;
          }
          const source = await options.imageSource.materialize(resource, async () => {
            const body = await resources.open({
              id: resource.id,
              kind: resource.kind,
              workspaceGeneration: identity.generation,
            });
            if (body.mediaType !== resource.mediaType) {
              throw new Error("The Kernel image resource media type changed.");
            }
            return body;
          });
          if (source !== undefined) {
            nextSources.set(relativePath, source);
            createdSources.add(source);
          }
        }
        assertNotAborted(signal);
      } catch (error: unknown) {
        createdSources.forEach(releaseImageSource);
        throw error;
      }
    }
    if (released || expectedEpoch !== imageResourceEpoch) {
      createdSources.forEach(releaseImageSource);
      return;
    }
    for (const [relativePath, source] of imageSources) {
      if (nextSources.get(relativePath) !== source) releaseImageSource(source);
    }
    imageResources = next;
    imageSources = nextSources;
    imageResourceCacheGeneration = identity.generation;
    imageResourceCacheReady = true;
  };

  const loadTreeAndImages = async (
    relativeRoot: string,
    signal?: AbortSignal | null,
  ) => {
    const epoch = imageResourceEpoch;
    const treeEntries = await listTree(relativeRoot, signal);
    await refreshImageResources(relativeRoot, treeEntries, signal, epoch);
    return treeEntries;
  };

  const ensureImageResources = async () => {
    const identity = await workspace();
    if (
      imageResourceCacheReady &&
      imageResourceCacheGeneration === identity.generation
    ) return;
    imageResourcePrewarm ??= (async () => {
      await loadTreeAndImages("");
    })().finally(() => {
      imageResourcePrewarm = undefined;
    });
    await imageResourcePrewarm;
  };

  const invalidateImageResources = () => {
    imageResourceEpoch += 1;
    imageSources.forEach(releaseImageSource);
    imageSources = new Map();
    imageResources = new Map();
    imageResourceCacheGeneration = undefined;
    imageResourceCacheReady = false;
  };

  const stopImageInvalidations = options.imageSource === undefined
    ? undefined
    : subscribeToInvalidations(invalidations, async (notice) => {
        if (notice.scopes.includes("resources") || notice.scopes.includes("workspace")) {
          invalidateImageResources();
        }
      });

  const resolveEntry = async (path: string) => {
    const relativePath = relativePathFromServerPath(path);
    const cached = entries.get(relativePath);
    if (cached !== undefined) return cached;
    await listTree("");
    const listed = entries.get(relativePath);
    if (listed === undefined) throw new Error("The Kernel document is unavailable.");
    return listed;
  };

  const cacheEntry = (entry: CachedEntry) => {
    entries.set(entry.relativePath, entry);
    return entry;
  };

  const removeCachedTree = (relativePath: string) => {
    if (relativePath === "") {
      entries.clear();
      return;
    }
    for (const path of [...entries.keys()]) {
      if (path === relativePath || path.startsWith(`${relativePath}/`)) entries.delete(path);
    }
  };

  const saveResource = async (input: {
    body: Blob;
    documentPath: string | null;
    folder: string;
    name: string;
  } & (
    | {
        kind: "attachment";
        mediaType: "application/octet-stream";
      }
    | {
        kind: "image";
        mediaType: "image/gif" | "image/jpeg" | "image/png" | "image/webp";
      }
  )) => {
    if (input.documentPath === null) {
      throw new Error("Current document must be a saved Markdown file.");
    }
    const identity = await workspace();
    const document = await resolveEntry(input.documentPath);
    if (document.kind !== "file") throw new Error("The Kernel path is not a document.");
    const folder = normalizeResourceFolder(input.folder) as KernelWorkspaceRelativePath;
    const resourceInput = {
      body: input.body,
      documentLocator: document.locator,
      folder,
      name: input.name,
      workspaceGeneration: identity.generation,
    };
    const created = await resources.create(input.kind === "image"
      ? { ...resourceInput, kind: input.kind, mediaType: input.mediaType }
      : { ...resourceInput, kind: input.kind, mediaType: input.mediaType });
    if (created.workspaceGeneration !== identity.generation) {
      throw new Error("The Kernel workspace generation changed.");
    }
    if (created.kind !== input.kind || created.mediaType !== input.mediaType) {
      throw new Error("The Kernel resource metadata changed.");
    }
    if (created.kind === "image") {
      imageResources.set(created.relativePath, created);
      if (options.imageSource !== undefined) {
        const epoch = imageResourceEpoch;
        const body: KernelResourceBody = {
          body: input.body,
          mediaType: created.mediaType,
        };
        const source = options.imageSource.materializeCreated === undefined
          ? await options.imageSource.materialize(created, async () => body)
          : await options.imageSource.materializeCreated(created, body);
        if (source !== undefined) {
          if (released || epoch !== imageResourceEpoch) {
            releaseImageSource(source);
          } else {
            const previous = imageSources.get(created.relativePath);
            if (previous !== undefined && previous !== source) releaseImageSource(previous);
            imageSources.set(created.relativePath, source);
          }
        }
      }
    }
    return {
      document,
      resource: created,
      src: markdownResourcePath(document.parent, created.relativePath),
    };
  };

  const files: AppFileRuntime = {
    ...fallback,
    ...pickNativeShellFallback(options.nativeShell),
    confirmMarkdownFileDelete: async (_fileName, labels) =>
      options.nativeShell?.confirmMarkdownFileDelete?.(_fileName, labels) ?? false,
    confirmUnsavedMarkdownDocumentDiscard: async (_fileName, labels) =>
      options.nativeShell?.confirmUnsavedMarkdownDocumentDiscard?.(_fileName, labels) ?? false,
    createMarkdownTreeFile: async (rootPath, fileName, optionsOrParentPath = null) => {
      assertServerRoot(rootPath);
      const identity = await workspace();
      const parentPath = typeof optionsOrParentPath === "string"
        ? optionsOrParentPath
        : optionsOrParentPath?.parentPath;
      const parent = parentPath === null || parentPath === undefined
        ? ""
        : relativePathFromServerPath(parentPath);
      const document = cacheEntry(await kernel.documents.create({
        contents: typeof optionsOrParentPath === "object"
          ? optionsOrParentPath?.contents ?? ""
          : "",
        kind: "file",
        name: fileName,
        parent: parent as KernelWorkspaceRelativePath,
        workspaceGeneration: identity.generation,
      }));
      return fileTreeEntry(document);
    },
    createMarkdownTreeFolder: async (rootPath, folderName, parentPath = null) => {
      assertServerRoot(rootPath);
      const identity = await workspace();
      const parent = parentPath === null ? "" : relativePathFromServerPath(parentPath);
      const document = cacheEntry(await kernel.documents.create({
        kind: "directory",
        name: folderName,
        parent: parent as KernelWorkspaceRelativePath,
        workspaceGeneration: identity.generation,
      }));
      return fileTreeEntry(document);
    },
    deleteMarkdownTreeFile: async (rootPath, path) => {
      assertServerRoot(rootPath);
      const identity = await workspace();
      const document = await resolveEntry(path);
      await kernel.documents.delete({
        deletionPolicy: "recoverable",
        expectedRevision: document.revision,
        locator: document.locator,
        workspaceGeneration: identity.generation,
      });
      removeCachedTree(document.relativePath);
    },
    listMarkdownFileHistory: async (path) => {
      const identity = await workspace();
      const document = await resolveEntry(path);
      const history = await listAllHistory(kernel, document.locator, identity.generation);
      return history.map((entry) => ({
        createdAt: timestamp(entry.createdAt),
        id: entry.snapshotId,
        sizeBytes: entry.sizeBytes,
      }));
    },
    listMarkdownFilesForPath: async (path) => {
      const relativeRoot = relativePathFromServerPath(path);
      return (await loadTreeAndImages(relativeRoot)).map(fileTreeEntry).sort(compareRelativePath);
    },
    loadMarkdownFilesForPath: async (path, loadOptions = {}) => {
      const relativeRoot = relativePathFromServerPath(path);
      const listed = (await loadTreeAndImages(relativeRoot, loadOptions.signal))
        .map(fileTreeEntry)
        .sort(compareRelativePath);
      assertNotAborted(loadOptions.signal);
      loadOptions.onBatch?.(listed);
      return listed;
    },
    moveMarkdownTreeFile: async (rootPath, path, targetParentPath = null) => {
      assertServerRoot(rootPath);
      const document = await resolveEntry(path);
      return moveEntry(document, document.name, targetParentPath);
    },
    openMarkdownFolder: async () => {
      const identity = await workspace();
      return { name: identity.displayName, path: kernelWorkspaceRoot };
    },
    readLocalImageFile: async (path) => {
      const relativePath = relativePathFromServerPath(path);
      let resource = imageResources.get(relativePath);
      if (resource === undefined) {
        await ensureImageResources();
        resource = imageResources.get(relativePath);
      }
      if (resource === undefined || resources === undefined) {
        throw new Error("The Kernel image resource is unavailable.");
      }
      const identity = await workspace();
      const response = await resources.open({
        id: resource.id,
        kind: "image",
        workspaceGeneration: identity.generation,
      });
      if (response.mediaType !== resource.mediaType) {
        throw new Error("The Kernel image resource media type changed.");
      }
      return new File([response.body], resource.name, { type: resource.mediaType });
    },
    readMarkdownFile: async (path) => {
      await ensureImageResources();
      const identity = await workspace();
      const entry = await resolveEntry(path);
      if (entry.kind !== "file") throw new Error("The Kernel path is not a document.");
      const document = await kernel.documents.read({
        locator: entry.locator,
        workspaceGeneration: identity.generation,
      });
      cacheEntry(document);
      return {
        content: document.contents,
        name: document.name,
        path: serverPathFromRelative(document.relativePath),
        sizeBytes: document.sizeBytes,
      };
    },
    readMarkdownFileHistory: async (path, id) => {
      const identity = await workspace();
      const document = await resolveEntry(path);
      const read = kernelHistoryReader(kernel);
      const history = await read({
        locator: document.locator,
        snapshotId: id as KernelHistorySnapshotId,
        workspaceGeneration: identity.generation,
      });
      return { contents: history.contents, id: history.snapshotId };
    },
    renameMarkdownTreeFile: async (rootPath, path, fileName) => {
      assertServerRoot(rootPath);
      const document = await resolveEntry(path);
      const parent = parentServerPath(document.relativePath);
      return moveEntry(document, fileName, parent);
    },
    resolveMarkdownImageSrc: (documentPath, source) => {
      const relativePath = resolveServerMarkdownImagePath(documentPath, source);
      if (relativePath === null) return undefined;
      const resource = imageResources.get(relativePath);
      if (resource === undefined) return undefined;
      return imageSources.get(relativePath) ?? options.resolveImageSrc?.(resource);
    },
    resolveMarkdownFolder: async (path) => {
      const identity = await workspace();
      if (path === kernelWorkspaceRoot) {
        return { name: identity.displayName, path: kernelWorkspaceRoot };
      }
      const entry = await resolveEntry(path);
      if (entry.kind !== "directory") throw new Error("The Kernel path is not a folder.");
      return { name: entry.name, path: serverPathFromRelative(entry.relativePath) };
    },
    resolveMarkdownPath: async (path) => {
      if (path === kernelWorkspaceRoot) {
        const identity = await workspace();
        return { kind: "folder", name: identity.displayName, path };
      }
      const entry = await resolveEntry(path);
      return {
        kind: entry.kind === "directory" ? "folder" : "file",
        name: entry.name,
        path: serverPathFromRelative(entry.relativePath),
      };
    },
    saveClipboardAttachment: async (input) => {
      if (kernel.availability !== "available") {
        return unavailableFileCapability("saveClipboardAttachment");
      }
      if (input.copyToStorage === false) {
        return {
          label: input.attachment.name.trim() || "attachment",
          src: URL.createObjectURL(input.attachment),
        };
      }
      const saved = await saveResource({
        body: input.attachment,
        documentPath: input.documentPath,
        folder: input.projectRootPath === null || input.projectRootPath === undefined
          ? input.folder
          : "assets",
        kind: "attachment",
        mediaType: "application/octet-stream",
        name: input.attachment.name.trim() || "attachment",
      });
      return {
        label: input.attachment.name.trim() || saved.resource.name,
        src: saved.src,
      };
    },
    saveClipboardImage: async (input) => {
      if (kernel.availability !== "available") {
        return unavailableFileCapability("saveClipboardImage");
      }
      if (input.copyToStorage === false) {
        return {
          alt: imageAltFromFileName(input.image.name),
          src: URL.createObjectURL(input.image),
        };
      }
      if (!previewableImageMediaTypes.has(input.image.type)) {
        throw new Error("The clipboard image media type is unsupported.");
      }
      const saved = await saveResource({
        body: input.image,
        documentPath: input.documentPath,
        folder: input.projectRootPath === null || input.projectRootPath === undefined
          ? input.folder
          : "assets",
        kind: "image",
        mediaType: input.image.type as "image/gif" | "image/jpeg" | "image/png" | "image/webp",
        name: input.fileName,
      });
      return {
        alt: imageAltFromFileName(input.image.name),
        src: saved.src,
      };
    },
    saveMarkdownFile: async (input) => {
      const identity = await workspace();
      if (input.path === null) {
        const parent = input.defaultDirectory === null || input.defaultDirectory === undefined
          ? ""
          : relativePathFromServerPath(input.defaultDirectory);
        const document = cacheEntry(await kernel.documents.create({
          contents: input.contents,
          kind: "file",
          name: input.suggestedName,
          parent: parent as KernelWorkspaceRelativePath,
          workspaceGeneration: identity.generation,
        }));
        return { name: document.name, path: serverPathFromRelative(document.relativePath) };
      }
      const entry = await resolveEntry(input.path);
      if (entry.kind !== "file") throw new Error("The Kernel path is not a document.");
      const document = cacheEntry(await kernel.documents.update({
        contents: input.contents,
        expectedRevision: entry.revision,
        locator: entry.locator,
        workspaceGeneration: identity.generation,
      }));
      return { name: document.name, path: serverPathFromRelative(document.relativePath) };
    },
    searchMarkdownFiles: async (request) => {
      const identity = await workspace();
      const result = await searchServerFiles(kernel, identity.generation, request);
      result.entries.forEach(cacheEntry);
      return result.response;
    },
    watchMarkdownFile: async (path, onChange, onTreeChange) => {
      const relativePath = relativePathFromServerPath(path);
      const eventSubscription = subscribeToInvalidations(invalidations, async (notice) => {
        if (notice.scopes.includes("resources")) invalidateImageResources();
        if (!notice.scopes.some(reloadsDocuments)) return;
        if (
          notice.paths !== undefined &&
          !notice.paths.some((candidate) => pathBelongsToTree(candidate, relativePath))
        ) return;
        if (notice.documentChange === "tree") {
          removeCachedTree(relativePath);
          await onTreeChange?.(path);
          return;
        }
        entries.delete(relativePath);
        await onChange(path);
        if (notice.documentChange === "snapshot") await onTreeChange?.(path);
      });
      if (eventSubscription !== undefined) return eventSubscription;

      let currentRevision = (await resolveEntry(path)).revision;
      return startPolling(async () => {
        entries.delete(relativePath);
        try {
          const next = await resolveEntry(path);
          if (next.revision === currentRevision) return;
          currentRevision = next.revision;
          await onChange(path);
        } catch (error: unknown) {
          if (isTerminalAdapterError(error, options)) throw error;
          await onTreeChange?.(path);
        }
      }, options);
    },
    watchMarkdownTree: async (path, onTreeChange) => {
      const relativePath = relativePathFromServerPath(path);
      const eventSubscription = subscribeToInvalidations(invalidations, async (notice) => {
        if (notice.scopes.includes("resources")) invalidateImageResources();
        if (!notice.scopes.some(reloadsDocuments)) return;
        if (
          notice.paths !== undefined &&
          !notice.paths.some((candidate) => pathBelongsToTree(candidate, relativePath))
        ) return;
        removeCachedTree(relativePath);
        await onTreeChange(path);
      });
      if (eventSubscription !== undefined) return eventSubscription;

      let fingerprint = treeFingerprint(await listTree(relativePath));
      return startPolling(async () => {
        const next = treeFingerprint(await listTree(relativePath));
        if (next === fingerprint) return;
        fingerprint = next;
        await onTreeChange(path);
      }, options);
    },
  };

  async function moveEntry(
    document: CachedEntry,
    name: string,
    targetParentPath: string | null,
  ) {
    const identity = await workspace();
    const targetParent = targetParentPath === null
      ? ""
      : relativePathFromServerPath(targetParentPath);
    const previousPath = document.relativePath;
    const moved = cacheEntry(await kernel.documents.move({
      expectedRevision: document.revision,
      locator: document.locator,
      name,
      targetParent: targetParent as KernelWorkspaceRelativePath,
      workspaceGeneration: identity.generation,
    }));
    removeCachedTree(previousPath);
    cacheEntry(moved);
    return fileTreeEntry(moved);
  }

  return Object.freeze({
    files,
    release: () => {
      if (released) return undefined;
      released = true;
      stopImageInvalidations?.();
      invalidateImageResources();
      entries.clear();
      workspaceIdentity = undefined;
      return undefined;
    },
  });

  function releaseImageSource(source: string) {
    try {
      options.imageSource?.release(source);
    } catch {
      // Resource URL cleanup is best-effort and cannot retain a stale cache entry.
    }
  }
}

function assertServerRoot(path: string) {
  if (path !== kernelWorkspaceRoot) {
    throw new Error("The Kernel workspace root is fixed.");
  }
}

function relativePathFromServerPath(path: string) {
  if (path === kernelWorkspaceRoot) return "";
  const prefix = `${kernelWorkspaceRoot}/`;
  if (!path.startsWith(prefix)) throw new Error("The path is outside the Kernel workspace.");
  const encoded = path.slice(prefix.length);
  if (encoded === "" || encoded.endsWith("/")) {
    throw new Error("The Kernel workspace path is invalid.");
  }
  let segments: string[];
  try {
    segments = encoded.split("/").map(decodeURIComponent);
  } catch {
    throw new Error("The Kernel workspace path is invalid.");
  }
  if (segments.some((segment) =>
    segment === "" || segment === "." || segment === ".." || segment.includes("/") || segment.includes("\\")
  )) {
    throw new Error("The Kernel workspace path is invalid.");
  }
  return segments.join("/");
}

function serverPathFromRelative(relativePath: string) {
  if (relativePath === "") return kernelWorkspaceRoot;
  const encoded = relativePath.split("/").map((segment) => encodeURIComponent(segment)).join("/");
  return `${kernelWorkspaceRoot}/${encoded}`;
}

function resolveServerMarkdownImagePath(documentPath: string, source: string) {
  let documentRelativePath: string;
  try {
    documentRelativePath = relativePathFromServerPath(documentPath);
  } catch {
    return null;
  }
  if (documentRelativePath === "" || source === "") return null;

  const sourceWithoutSuffix = source.split(/[?#]/u, 1)[0] ?? "";
  if (sourceWithoutSuffix === "" || sourceWithoutSuffix.includes("\\")) return null;
  let rawPath: string;
  let resolved = documentRelativePath.split("/").slice(0, -1);
  if (sourceWithoutSuffix === kernelWorkspaceRoot) return null;
  if (sourceWithoutSuffix.startsWith(`${kernelWorkspaceRoot}/`)) {
    rawPath = sourceWithoutSuffix.slice(kernelWorkspaceRoot.length + 1);
    resolved = [];
  } else if (/^[a-zA-Z][a-zA-Z\d+.-]*:/u.test(sourceWithoutSuffix)) {
    return null;
  } else if (sourceWithoutSuffix.startsWith("//")) {
    return null;
  } else if (sourceWithoutSuffix.startsWith("/")) {
    rawPath = sourceWithoutSuffix.slice(1);
    resolved = [];
  } else {
    rawPath = sourceWithoutSuffix;
  }

  const segments = rawPath.split("/");
  for (const encodedSegment of segments) {
    let segment: string;
    try {
      segment = decodeURIComponent(encodedSegment);
    } catch {
      return null;
    }
    if (/[\u0000-\u001f\u007f-\u009f\\/]/u.test(segment)) return null;
    if (segment === ".") continue;
    if (segment === "..") {
      if (resolved.length === 0) return null;
      resolved.pop();
      continue;
    }
    if (segment === "") return null;
    resolved.push(segment);
  }
  return resolved.length === 0 ? null : resolved.join("/");
}

function parentServerPath(relativePath: string) {
  const separator = relativePath.lastIndexOf("/");
  return serverPathFromRelative(separator < 0 ? "" : relativePath.slice(0, separator));
}

function fileTreeEntry(entry: KernelDocumentEntrySnapshot): NativeMarkdownFolderFile {
  return {
    kind: entry.kind === "directory" ? "folder" : undefined,
    modifiedAt: timestamp(entry.modifiedAt),
    name: entry.name,
    path: serverPathFromRelative(entry.relativePath),
    relativePath: entry.relativePath,
    sizeBytes: entry.sizeBytes,
  };
}

function timestamp(value: string) {
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function compareRelativePath(left: NativeMarkdownFolderFile, right: NativeMarkdownFolderFile) {
  return left.relativePath.localeCompare(right.relativePath);
}

function assertNotAborted(signal?: AbortSignal | null) {
  if (signal?.aborted === true) throw new DOMException("The operation was aborted.", "AbortError");
}

async function listAllHistory(
  kernel: KernelDomainPort,
  locator: KernelDocumentLocator,
  workspaceGeneration: KernelWorkspaceGeneration,
) {
  const items: Awaited<ReturnType<KernelDomainPort["documents"]["history"]["list"]>>["items"] = [];
  let cursor: KernelPageCursor | undefined;
  do {
    const page = await kernel.documents.history.list({
      cursor,
      limit: 100,
      locator,
      workspaceGeneration,
    });
    items.push(...page.items);
    cursor = page.nextCursor ?? undefined;
  } while (cursor !== undefined);
  return items;
}

function kernelHistoryReader(kernel: KernelDomainPort) {
  return kernel.documents.history.read;
}

async function searchServerFiles(
  kernel: KernelDomainPort,
  workspaceGeneration: KernelWorkspaceGeneration,
  request: Parameters<NonNullable<AppFileRuntime["searchMarkdownFiles"]>>[0],
) {
  const maxMatches = request.maxMatches === undefined
    ? Number.MAX_SAFE_INTEGER
    : Math.max(0, request.maxMatches);
  const items: Awaited<ReturnType<KernelDomainPort["documents"]["search"]>>["items"] = [];
  let cursor: KernelPageCursor | undefined;
  do {
    const page = await kernel.documents.search({
      cursor,
      limit: Math.min(100, Math.max(1, maxMatches - items.length)),
      query: request.query,
      workspaceGeneration,
    });
    items.push(...page.items.slice(0, Math.max(0, maxMatches - items.length)));
    cursor = page.nextCursor ?? undefined;
  } while (cursor !== undefined && items.length < maxMatches);
  const queryLength = Math.max(1, request.query.length);
  const response: WorkspaceSearchResponse = {
    results: items.map((item, index) => {
      const columnNumber = Math.max(1, item.column);
      const path = serverPathFromRelative(item.document.relativePath);
      return {
        columnNumber,
        file: fileTreeEntry(item.document),
        id: `${path}:${item.line}:${columnNumber}:${index}`,
        lineNumber: Math.max(1, item.line),
        lineText: item.preview,
        match: { from: columnNumber - 1, to: columnNumber - 1 + queryLength },
        matchIndex: index,
        snippet: item.preview,
      };
    }),
    searchedFileCount: new Set(items.map((item) => item.document.locator)).size,
    truncated: cursor !== undefined,
    unreadableFileCount: 0,
  };
  return { entries: items.map((item) => item.document), response };
}

function treeFingerprint(entries: readonly KernelDocumentEntrySnapshot[]) {
  return entries.map((entry) => `${entry.relativePath}\u0000${entry.revision}`).sort().join("\u0001");
}

function startPolling(
  poll: () => Promise<unknown>,
  options: KernelFileRuntimeOptions,
) {
  const schedule = options.setInterval ?? globalThis.setInterval;
  const cancel = options.clearInterval ?? globalThis.clearInterval;
  let stopped = false;
  const interval = schedule(() => {
    poll().catch((error: unknown) => {
      if (isTerminalAdapterError(error, options)) {
        stopped = true;
        cancel(interval);
      }
      return undefined;
    });
  }, options.pollIntervalMs ?? 1_500);
  return () => {
    if (!stopped) cancel(interval);
    stopped = true;
    return undefined;
  };
}

function isTerminalAdapterError(error: unknown, options: KernelFileRuntimeOptions) {
  if (options.isTerminalError?.(error) === true) return true;
  if (typeof error !== "object" || error === null || !("code" in error)) return false;
  return error.code === "authentication-required" || error.code === "released";
}

function subscribeToInvalidations(
  source: KernelInvalidationSource | undefined,
  listener: (notice: KernelInvalidationNotice) => Promise<unknown>,
) {
  if (source?.available !== true) return undefined;
  return source.subscribe((notice) => {
    listener(notice).catch(() => undefined);
  });
}

function reloadsDocuments(scope: KernelInvalidationNotice["scopes"][number]) {
  return scope === "documents" || scope === "workspace";
}

function pathBelongsToTree(candidate: string, relativeRoot: string) {
  return relativeRoot === "" ||
    candidate === relativeRoot ||
    candidate.startsWith(`${relativeRoot}/`);
}

function pickNativeShellFallback(
  shell: KernelFileNativeShellFallback | undefined,
): KernelFileNativeShellFallback {
  if (shell === undefined) return {};
  return {
    ...(shell.confirmMarkdownFileDelete === undefined ? {} : {
      confirmMarkdownFileDelete: shell.confirmMarkdownFileDelete,
    }),
    ...(shell.confirmUnsavedMarkdownDocumentDiscard === undefined ? {} : {
      confirmUnsavedMarkdownDocumentDiscard: shell.confirmUnsavedMarkdownDocumentDiscard,
    }),
    ...(shell.detectPandocPath === undefined ? {} : { detectPandocPath: shell.detectPandocPath }),
    ...(shell.installMarkdownFileDrop === undefined ? {} : {
      installMarkdownFileDrop: shell.installMarkdownFileDrop,
    }),
    ...(shell.listenOpenedMarkdownPaths === undefined ? {} : {
      listenOpenedMarkdownPaths: shell.listenOpenedMarkdownPaths,
    }),
    ...(shell.openLocalFiles === undefined ? {} : { openLocalFiles: shell.openLocalFiles }),
    ...(shell.openLocalImages === undefined ? {} : { openLocalImages: shell.openLocalImages }),
    ...(shell.openMarkdownFile === undefined ? {} : { openMarkdownFile: shell.openMarkdownFile }),
    ...(shell.openSettingsFile === undefined ? {} : { openSettingsFile: shell.openSettingsFile }),
    ...(shell.readMarkdownTemplateFile === undefined ? {} : {
      readMarkdownTemplateFile: shell.readMarkdownTemplateFile,
    }),
    ...(shell.saveHtmlFile === undefined ? {} : { saveHtmlFile: shell.saveHtmlFile }),
    ...(shell.savePandocFile === undefined ? {} : { savePandocFile: shell.savePandocFile }),
    ...(shell.savePdfFile === undefined ? {} : { savePdfFile: shell.savePdfFile }),
    ...(shell.saveSettingsFile === undefined ? {} : { saveSettingsFile: shell.saveSettingsFile }),
    ...(shell.takeOpenedMarkdownPaths === undefined ? {} : {
      takeOpenedMarkdownPaths: shell.takeOpenedMarkdownPaths,
    }),
  };
}

function createKernelFileFallback(
  shell: KernelFileNativeShellFallback | undefined,
): AppFileRuntime {
  const native = pickNativeShellFallback(shell);
  return {
    confirmMarkdownFileDelete: native.confirmMarkdownFileDelete ?? (async () => false),
    confirmWorkspaceResourceTrash: async () => false,
    confirmUnsavedMarkdownDocumentDiscard:
      native.confirmUnsavedMarkdownDocumentDiscard ?? (async () => false),
    createMarkdownTreeFile: () => unavailableFileCapability("createMarkdownTreeFile"),
    createMarkdownTreeFolder: () => unavailableFileCapability("createMarkdownTreeFolder"),
    deleteMarkdownTemplateFile: () => unavailableFileCapability("deleteMarkdownTemplateFile"),
    deleteMarkdownTreeFile: () => unavailableFileCapability("deleteMarkdownTreeFile"),
    detectPandocPath: native.detectPandocPath ?? (async () => null),
    importLocalFile: () => unavailableFileCapability("importLocalFile"),
    installMarkdownFileDrop: native.installMarkdownFileDrop ?? (async () => () => undefined),
    listenOpenedMarkdownPaths:
      native.listenOpenedMarkdownPaths ?? (async () => () => undefined),
    listMarkdownFileHistory: async () => [],
    listMarkdownFilesForPath: async () => [],
    listMarkdownReferenceFilesForPath: async () => [],
    moveMarkdownTreeFile: () => unavailableFileCapability("moveMarkdownTreeFile"),
    openContainingFolder: () => unavailableFileCapability("openContainingFolder"),
    openLocalImages: native.openLocalImages ?? (async () => []),
    openLocalFiles: native.openLocalFiles ?? (async () => []),
    openMarkdownAttachment: () => unavailableFileCapability("openMarkdownAttachment"),
    openMarkdownFile: native.openMarkdownFile ?? (async () => null),
    openMarkdownFileInNewWindow: () => unavailableFileCapability("openMarkdownFileInNewWindow"),
    openMarkdownFolder: async () => null,
    openMarkdownFolderInNewWindow: () => unavailableFileCapability("openMarkdownFolderInNewWindow"),
    openSettingsFile: native.openSettingsFile ?? (async () => null),
    readLocalImageFile: () => unavailableFileCapability("readLocalImageFile"),
    readMarkdownFile: () => unavailableFileCapability("readMarkdownFile"),
    readMarkdownFileHistory: () => unavailableFileCapability("readMarkdownFileHistory"),
    readMarkdownTemplateFile:
      native.readMarkdownTemplateFile ?? (() => unavailableFileCapability("readMarkdownTemplateFile")),
    renameMarkdownTreeFile: () => unavailableFileCapability("renameMarkdownTreeFile"),
    requestPrimaryNotebookSwitch: () => unavailableFileCapability("requestPrimaryNotebookSwitch"),
    resolveMarkdownFolder: () => unavailableFileCapability("resolveMarkdownFolder"),
    resolveMarkdownPath: () => unavailableFileCapability("resolveMarkdownPath"),
    resolveWorkspaceResourceRoot: () => unavailableFileCapability("resolveWorkspaceResourceRoot"),
    saveClipboardAttachment: () => unavailableFileCapability("saveClipboardAttachment"),
    saveClipboardImage: () => unavailableFileCapability("saveClipboardImage"),
    saveHtmlFile: native.saveHtmlFile ?? (async () => null),
    saveMarkdownFile: async () => null,
    savePandocFile: native.savePandocFile ?? (async () => null),
    savePdfFile: native.savePdfFile ?? (async () => null),
    saveSettingsFile: native.saveSettingsFile ?? (async () => null),
    takeOpenedMarkdownPaths: native.takeOpenedMarkdownPaths ?? (async () => []),
    trashMarkdownAssets: () => unavailableFileCapability("trashMarkdownAssets"),
    trashWorkspaceResources: () => unavailableFileCapability("trashWorkspaceResources"),
    watchMarkdownFile: async () => () => undefined,
    watchMarkdownTree: async () => () => undefined,
    writeMarkdownTemplateFile: () => unavailableFileCapability("writeMarkdownTemplateFile"),
  };
}

function unavailableFileCapability(name: string): Promise<never> {
  return Promise.reject(new Error(`${name} is unavailable for a Kernel workspace.`));
}

function normalizeResourceFolder(folder: string) {
  const segments = folder
    .split(/[\\/]+/u)
    .map((segment) => segment.trim())
    .filter((segment) => segment !== "");
  if (segments.some((segment) => segment === "." || segment === "..")) {
    throw new Error("The resource folder is invalid.");
  }
  return segments.join("/");
}

function markdownResourcePath(documentParent: string, resourcePath: string) {
  const prefix = documentParent === "" ? "" : `${documentParent}/`;
  if (!resourcePath.startsWith(prefix) || resourcePath.length <= prefix.length) {
    throw new Error("The Kernel resource path is outside the document folder.");
  }
  return resourcePath
    .slice(prefix.length)
    .split("/")
    .map(encodeMarkdownUrlSegment)
    .join("/");
}

function encodeMarkdownUrlSegment(segment: string) {
  return encodeURIComponent(segment).replace(/[!'()*]/gu, (character) =>
    `%${character.charCodeAt(0).toString(16).toUpperCase()}`
  );
}

function imageAltFromFileName(fileName: string) {
  const trimmed = fileName.trim();
  if (trimmed === "") return "image";
  const withoutExtension = trimmed.replace(/\.[^.]*$/u, "").trim();
  return withoutExtension || "image";
}

function sameResourceVersion(
  left: KernelResourceSnapshot,
  right: KernelResourceSnapshot,
) {
  return (
    left.id === right.id &&
    left.kind === right.kind &&
    left.mediaType === right.mediaType &&
    left.relativePath === right.relativePath &&
    left.revision === right.revision &&
    left.workspaceGeneration === right.workspaceGeneration
  );
}
