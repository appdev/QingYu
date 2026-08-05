import {
  createUnavailableKernelDomainPort,
  type KernelDocumentEntrySnapshot,
  type KernelPageCursor,
  type KernelDomainPort,
  type KernelInvalidationNotice,
  type KernelRevision,
  type KernelWorkspaceGeneration,
} from "@markra/app/runtime";

import {
  createServerFileRuntime,
  createServerFileRuntimeOwner,
  serverWorkspaceRoot,
} from "./files";
import type {
  ServerKernelDomainPort,
} from "./kernel";
import { ServerKernelDomainAdapterError } from "./kernel";

const generation = "generation-1" as KernelWorkspaceGeneration;
const revision = "revision-1" as KernelRevision;

describe("server file facade", () => {
  it("opens, lists, reads, and saves the fixed Kernel workspace without local owners", async () => {
    const showDirectoryPicker = vi.fn(() => {
      throw new Error("directory picker must be unreachable");
    });
    const indexedDbOpen = vi.fn(() => {
      throw new Error("IndexedDB owner must be unreachable");
    });
    const kernel = kernelPort();
    const files = createServerFileRuntime(kernel, {
      indexedDB: { open: indexedDbOpen },
      showDirectoryPicker,
    });

    await expect(files.resolveMarkdownFolder(serverWorkspaceRoot)).resolves.toEqual({
      name: "Notes",
      path: serverWorkspaceRoot,
    });
    await expect(files.listMarkdownFilesForPath(serverWorkspaceRoot)).resolves.toEqual([
      expect.objectContaining({
        name: "note.md",
        path: `${serverWorkspaceRoot}/note.md`,
        relativePath: "note.md",
      }),
    ]);
    await expect(files.readMarkdownFile(`${serverWorkspaceRoot}/note.md`)).resolves.toEqual({
      content: "first",
      name: "note.md",
      path: `${serverWorkspaceRoot}/note.md`,
      sizeBytes: 5,
    });
    await expect(files.saveMarkdownFile({
      contents: "second",
      path: `${serverWorkspaceRoot}/note.md`,
      suggestedName: "note.md",
    })).resolves.toEqual({
      name: "note.md",
      path: `${serverWorkspaceRoot}/note.md`,
    });
    await files.saveMarkdownFile({
      contents: "third",
      path: `${serverWorkspaceRoot}/note.md`,
      suggestedName: "note.md",
    });

    expect(kernel.documents.update).toHaveBeenNthCalledWith(1, {
      contents: "second",
      expectedRevision: revision,
      locator: "document-1",
      workspaceGeneration: generation,
    });
    expect(kernel.documents.update).toHaveBeenNthCalledWith(2, expect.objectContaining({
      contents: "third",
      expectedRevision: "revision-2",
      locator: "document-1",
    }));
    expect(showDirectoryPicker).not.toHaveBeenCalled();
    expect(indexedDbOpen).not.toHaveBeenCalled();
  });

  it("returns exact server rename identity and preserves both files on collision without numbering", async () => {
    const sourceContents = "# A\n\0original source";
    const targetContents = "# B\noriginal target 🐉";
    const source = entry({ locator: "document-a", name: "A.md", relativePath: "A.md" });
    const target = entry({ locator: "document-b", name: "B.md", relativePath: "B.md" });
    const entries = new Map([
      [source.locator, { contents: sourceContents, entry: source }],
      [target.locator, { contents: targetContents, entry: target }],
    ]);
    const kernel = kernelPort();
    vi.mocked(kernel.documents.list).mockImplementation(async () => ({
      items: [...entries.values()].map(({ entry }) => entry),
      nextCursor: null,
      workspaceGeneration: generation,
    }));
    vi.mocked(kernel.documents.read).mockImplementation(async ({ locator }) => {
      const document = entries.get(locator);
      if (document === undefined) throw new Error("document unavailable");
      return { ...document.entry, contents: document.contents, kind: "file" as const };
    });
    vi.mocked(kernel.documents.move).mockImplementation(async (input) => {
      const current = entries.get(input.locator);
      if (current === undefined) throw new Error("document unavailable");
      const relativePath = input.targetParent === ""
        ? input.name
        : `${input.targetParent}/${input.name}`;
      if ([...entries.values()].some(({ entry }) => entry.relativePath === relativePath)) {
        throw Object.assign(new Error("document already exists"), {
          code: "document_already_exists",
        });
      }
      const moved = {
        ...current.entry,
        modifiedAt: "2026-07-30T00:00:01Z",
        name: input.name,
        parent: input.targetParent,
        relativePath: relativePath as never,
        revision: "revision-2" as KernelRevision,
      } satisfies KernelDocumentEntrySnapshot;
      entries.delete(input.locator);
      entries.set(moved.locator, { contents: current.contents, entry: moved });
      return moved;
    });
    const files = createServerFileRuntime(kernel);

    await expect(files.renameMarkdownTreeFile(
      serverWorkspaceRoot,
      `${serverWorkspaceRoot}/A.md`,
      "B.md",
    )).rejects.toMatchObject({ code: "document_already_exists" });
    expect(kernel.documents.move).toHaveBeenCalledOnce();
    expect(kernel.documents.move).toHaveBeenLastCalledWith(expect.objectContaining({ name: "B.md" }));
    const sourceAfterCollision = await files.readMarkdownFile(`${serverWorkspaceRoot}/A.md`);
    const targetAfterCollision = await files.readMarkdownFile(`${serverWorkspaceRoot}/B.md`);
    expect(new TextEncoder().encode(sourceAfterCollision.content))
      .toEqual(new TextEncoder().encode(sourceContents));
    expect(new TextEncoder().encode(targetAfterCollision.content))
      .toEqual(new TextEncoder().encode(targetContents));

    vi.mocked(kernel.documents.move).mockClear();
    const renamed = await files.renameMarkdownTreeFile(
      serverWorkspaceRoot,
      `${serverWorkspaceRoot}/A.md`,
      "C.md",
    );
    expect({
      name: renamed.name,
      path: renamed.path,
      relativePath: renamed.relativePath,
    }).toEqual({
      name: "C.md",
      path: `${serverWorkspaceRoot}/C.md`,
      relativePath: "C.md",
    });
    expect(kernel.documents.move).toHaveBeenCalledOnce();
  });

  it("recursively exhausts every directory page and keeps nested paths stable", async () => {
    const kernel = kernelPort();
    const rootFolder = entry({
      kind: "directory",
      locator: "folder-1",
      name: "nested",
      relativePath: "nested",
    });
    const rootFile = entry({ locator: "document-root", name: "root.md", relativePath: "root.md" });
    const nestedFile = entry({
      locator: "document-nested",
      name: "inside.md",
      parent: "nested",
      relativePath: "nested/inside.md",
    });
    vi.mocked(kernel.documents.list).mockImplementation(async (input) => {
      if (input.parent === "nested") {
        return { items: [nestedFile], nextCursor: null, workspaceGeneration: generation };
      }
      if (input.cursor === undefined) {
        return {
          items: [rootFolder],
          nextCursor: "root-page-2" as KernelPageCursor,
          workspaceGeneration: generation,
        };
      }
      return { items: [rootFile], nextCursor: null, workspaceGeneration: generation };
    });

    const files = createServerFileRuntime(kernel);
    await expect(files.listMarkdownFilesForPath(serverWorkspaceRoot)).resolves.toEqual([
      expect.objectContaining({ relativePath: "nested", kind: "folder" }),
      expect.objectContaining({ relativePath: "nested/inside.md" }),
      expect.objectContaining({ relativePath: "root.md" }),
    ]);

    expect(kernel.documents.list).toHaveBeenCalledWith(expect.objectContaining({
      cursor: "root-page-2",
      parent: "",
    }));
    expect(kernel.documents.list).toHaveBeenCalledWith(expect.objectContaining({
      cursor: undefined,
      parent: "nested",
    }));
  });

  it("publishes same-origin Kernel media URLs without opening preview bytes", async () => {
    const kernel = kernelPort();
    const imageUrl = vi.fn(({ id, revision }: { id: string; revision: KernelRevision }) =>
      `/media/v1/images/${encodeURIComponent(id)}?revision=${encodeURIComponent(revision)}`
    );
    Object.assign(kernel.resources, { imageUrl });
    vi.mocked(kernel.resources.list).mockResolvedValue({
      items: [resource({
        id: "image/root capability.signature",
        name: "cover image.png",
        relativePath: "cover image.png",
      })],
      workspaceGeneration: generation,
    });
    const files = createServerFileRuntime(kernel);
    const documentPath = `${serverWorkspaceRoot}/note.md`;

    await files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);

    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover%20image.png")).toBe(
      "/media/v1/images/image%2Froot%20capability.signature?revision=resource-revision-1",
    );
    expect(imageUrl).toHaveBeenCalledWith({
      id: "image/root capability.signature",
      revision: "resource-revision-1",
    });
    expect(kernel.resources.open).not.toHaveBeenCalled();
  });

  it("indexes nested images for media URLs and keeps authenticated byte reads separate", async () => {
    const kernel = kernelPort();
    const notesFolder = entry({
      kind: "directory",
      locator: "folder-notes",
      name: "notes",
      relativePath: "notes",
    });
    const assetsFolder = entry({
      kind: "directory",
      locator: "folder-assets",
      name: "assets",
      relativePath: "assets",
    });
    const nestedDocument = entry({
      locator: "document-nested",
      name: "today.md",
      parent: "notes",
      relativePath: "notes/today.md",
    });
    vi.mocked(kernel.documents.list).mockImplementation(async (input) => ({
      items: input.parent === "notes"
        ? [nestedDocument]
        : input.parent === "assets" ? [] : [notesFolder, assetsFolder],
      nextCursor: null,
      workspaceGeneration: generation,
    }));
    vi.mocked(kernel.documents.read).mockResolvedValue({
      ...nestedDocument,
      contents: "![Cover](../assets/cover%20image.png)",
      kind: "file",
    });
    vi.mocked(kernel.resources.list).mockImplementation(async (input) => ({
      items: input.parent === "assets"
        ? [
            resource({
              id: "image/payload.signature",
              mediaType: "image/png",
              name: "cover image.png",
              relativePath: "assets/cover image.png",
            }),
            resource({
              id: "attachment/payload.signature",
              kind: "attachment",
              mediaType: "application/octet-stream",
              name: "manual.pdf",
              previewable: false,
              relativePath: "assets/manual.pdf",
            }),
            resource({
              id: "image-svg/payload.signature",
              mediaType: "image/svg+xml",
              name: "untrusted.svg",
              relativePath: "assets/untrusted.svg",
            }),
          ]
        : [],
      workspaceGeneration: generation,
    }));
    vi.mocked(kernel.resources.open).mockImplementation(async ({ id }) => {
      const mediaType = id === "image-svg/payload.signature" ? "image/svg+xml" : "image/png";
      return { body: new Blob(["image bytes"], { type: mediaType }), mediaType };
    });
    const files = createServerFileRuntime(kernel);
    const documentPath = `${serverWorkspaceRoot}/notes/today.md`;

    expect(files.resolveMarkdownImageSrc?.(documentPath, "../assets/cover%20image.png"))
      .toBeNull();
    await files.readMarkdownFile(documentPath);

    const imageUrl =
      "/media/v1/images/image%2Fpayload.signature?revision=resource-revision-1";
    const svgUrl =
      "/media/v1/images/image-svg%2Fpayload.signature?revision=resource-revision-1";
    expect(files.resolveMarkdownImageSrc?.(documentPath, "../assets/cover%20image.png"))
      .toBe(imageUrl);
    expect(files.resolveMarkdownImageSrc?.(documentPath, "/assets/cover%20image.png"))
      .toBe(imageUrl);
    expect(files.resolveMarkdownImageSrc?.(
      documentPath,
      `${serverWorkspaceRoot}/assets/cover%20image.png`,
    )).toBe(imageUrl);
    expect(files.resolveMarkdownImageSrc?.(documentPath, "../assets/untrusted.svg"))
      .toBe(svgUrl);
    expect(kernel.resources.open).not.toHaveBeenCalled();
    const inventoryCallCount = vi.mocked(kernel.resources.list).mock.calls.length;
    await files.readMarkdownFile(documentPath);
    expect(kernel.resources.list).toHaveBeenCalledTimes(inventoryCallCount);
    const preview = await files.readLocalImageFile(
      `${serverWorkspaceRoot}/assets/cover%20image.png`,
    );
    expect(preview).toMatchObject({ name: "cover image.png", type: "image/png" });
    expect(await preview.text()).toBe("image bytes");
    expect(kernel.resources.open).toHaveBeenCalledWith({
      id: "image/payload.signature",
      kind: "image",
      workspaceGeneration: generation,
    });
    expect(kernel.resources.open).toHaveBeenCalledOnce();
    expect(files.resolveMarkdownImageSrc?.(documentPath, "../assets/manual.pdf")).toBeNull();
    for (const source of [
      "",
      "../../outside.png",
      "%2e%2e/%2e%2e/outside.png",
      "../assets/cover%2Fimage.png",
      "../assets/%5Cevil.png",
      "../assets\\evil.png",
      "../assets/%00evil.png",
      "../assets/bad%encoding.png",
      "../assets//cover.png",
    ]) {
      expect(files.resolveMarkdownImageSrc?.(documentPath, source)).toBeNull();
    }
    for (const source of [
      "https://example.test/cover.png",
      "data:image/png;base64,abc",
      "blob:https://example.test/id",
      "file:///etc/passwd",
      "//example.test/cover.png",
    ]) {
      expect(files.resolveMarkdownImageSrc?.(documentPath, source)).toBeUndefined();
    }
  });

  it("replaces revision-bound media URLs after refresh and invalidates them on sync completion", async () => {
    const listeners = new Set<(notice: KernelInvalidationNotice) => unknown>();
    const kernel = Object.assign(kernelPort(), {
      invalidations: {
        available: true,
        subscribe: (listener: (notice: KernelInvalidationNotice) => unknown) => {
          listeners.add(listener);
          return () => {
            listeners.delete(listener);
            return undefined;
          };
        },
      },
    });
    let imageId = "image-old.signature";
    vi.mocked(kernel.resources.list).mockImplementation(async () => ({
      items: [resource({ id: imageId, name: "cover.png", relativePath: "cover.png" })],
      workspaceGeneration: generation,
    }));
    const files = createServerFileRuntime(kernel);
    const documentPath = `${serverWorkspaceRoot}/note.md`;

    await files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png"))
      .toBe("/media/v1/images/image-old.signature?revision=resource-revision-1");
    imageId = "image-new.signature";
    await files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png"))
      .toBe("/media/v1/images/image-new.signature?revision=resource-revision-1");

    const onTreeChange = vi.fn();
    await files.watchMarkdownTree(serverWorkspaceRoot, onTreeChange);
    publish(listeners, syncSucceededNotice());

    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledOnce());
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png"))
      .toBe("/media/v1/images/image-new.signature?revision=resource-revision-1");

    await files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png"))
      .toBe("/media/v1/images/image-new.signature?revision=resource-revision-1");
    publish(listeners, {
      documentChange: "snapshot",
      scopes: ["documents", "workspace", "resources"],
    });

    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledTimes(2));
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png"))
      .toBe("/media/v1/images/image-new.signature?revision=resource-revision-1");
    expect(kernel.resources.open).not.toHaveBeenCalled();
  });

  it("routes history reads and search through exact Server Kernel capabilities", async () => {
    const kernel = kernelPort() as ServerKernelDomainPort;
    const historyPage = {
      items: [{
        createdAt: "2026-07-29T00:00:00Z",
        documentLocator: "document-1" as never,
        revision: "history-revision-1" as KernelRevision,
        sizeBytes: 4,
        snapshotId: "snapshot-1" as never,
        workspaceGeneration: generation,
      }],
      nextCursor: null,
      workspaceGeneration: generation,
    };
    vi.mocked(kernel.documents.history.list).mockImplementation(async () => historyPage);
    const readHistory = vi.fn<ServerKernelDomainPort["documents"]["history"]["read"]>(
      async () => ({
        contents: "past",
        documentLocator: "document-1" as never,
        revision: "history-revision-1" as KernelRevision,
        snapshotId: "snapshot-1" as never,
        workspaceGeneration: generation,
      }),
    );
    Object.assign(kernel.documents.history, { read: readHistory });
    vi.mocked(kernel.documents.search).mockResolvedValue({
      items: [{
        column: 2,
        document: entry({ locator: "document-1", name: "note.md", relativePath: "note.md" }),
        line: 3,
        preview: "a first line",
      }],
      nextCursor: null,
      workspaceGeneration: generation,
    });
    const files = createServerFileRuntime(kernel);

    await expect(files.listMarkdownFileHistory(`${serverWorkspaceRoot}/note.md`)).resolves.toEqual([{
      createdAt: Date.parse("2026-07-29T00:00:00Z"),
      id: "snapshot-1",
      sizeBytes: 4,
    }]);
    await expect(files.readMarkdownFileHistory(
      `${serverWorkspaceRoot}/note.md`,
      "snapshot-1",
    )).resolves.toEqual({ contents: "past", id: "snapshot-1" });
    await expect(files.searchMarkdownFiles?.({
      path: serverWorkspaceRoot,
      query: "first",
    })).resolves.toMatchObject({
      results: [{
        columnNumber: 2,
        file: { path: `${serverWorkspaceRoot}/note.md` },
        lineNumber: 3,
      }],
      truncated: false,
      unreadableFileCount: 0,
    });
  });

  it("uses freshly issued locator and revision values after moving a directory", async () => {
    const kernel = kernelPort();
    const folder = entry({
      kind: "directory",
      locator: "folder-old",
      name: "drafts",
      relativePath: "drafts",
    });
    const child = entry({
      locator: "child-old",
      name: "child.md",
      parent: "drafts",
      relativePath: "drafts/child.md",
    });
    vi.mocked(kernel.documents.list).mockImplementation(async (input) => ({
      items: input.parent === "drafts" ? [child] : [folder],
      nextCursor: null,
      workspaceGeneration: generation,
    }));
    vi.mocked(kernel.documents.move).mockResolvedValue(entry({
      kind: "directory",
      locator: "folder-new",
      name: "archive",
      relativePath: "archive",
    }));
    vi.mocked(kernel.documents.delete).mockResolvedValue(undefined);
    const files = createServerFileRuntime(kernel);
    await files.listMarkdownFilesForPath(serverWorkspaceRoot);

    await expect(files.renameMarkdownTreeFile(
      serverWorkspaceRoot,
      `${serverWorkspaceRoot}/drafts`,
      "archive",
    )).resolves.toMatchObject({ path: `${serverWorkspaceRoot}/archive` });
    await files.deleteMarkdownTreeFile(serverWorkspaceRoot, `${serverWorkspaceRoot}/archive`);

    expect(kernel.documents.move).toHaveBeenCalledWith(expect.objectContaining({
      expectedRevision: revision,
      locator: "folder-old",
      name: "archive",
    }));
    expect(kernel.documents.delete).toHaveBeenCalledWith(expect.objectContaining({
      expectedRevision: revision,
      locator: "folder-new",
    }));
  });

  it("stops file polling after authentication is lost instead of swallowing it as a tree change", async () => {
    const kernel = kernelPort();
    let poll: (() => unknown) | undefined;
    const clearInterval = vi.fn();
    const files = createServerFileRuntime(kernel, {
      clearInterval: clearInterval as never,
      setInterval: ((handler: () => unknown) => {
        poll = handler;
        return 42;
      }) as never,
    });
    const onChange = vi.fn();
    const onTreeChange = vi.fn();
    await files.watchMarkdownFile(
      `${serverWorkspaceRoot}/note.md`,
      onChange,
      onTreeChange,
    );
    vi.mocked(kernel.documents.list).mockRejectedValue(
      new ServerKernelDomainAdapterError("authentication-required"),
    );

    poll?.();

    await vi.waitFor(() => expect(clearInterval).toHaveBeenCalledWith(42));
    expect(onChange).not.toHaveBeenCalled();
    expect(onTreeChange).not.toHaveBeenCalled();
  });

  it("prefers Kernel events and reloads document snapshots after reconnect or gaps", async () => {
    const listeners = new Set<(notice: KernelInvalidationNotice) => unknown>();
    const invalidations = {
      available: true,
      subscribe: vi.fn((listener: (notice: KernelInvalidationNotice) => unknown) => {
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
          return undefined;
        };
      }),
    };
    const kernel = Object.assign(kernelPort(), { invalidations });
    const schedule = vi.fn(() => {
      throw new Error("polling must be unreachable while Kernel events are available");
    });
    const files = createServerFileRuntime(kernel, { setInterval: schedule as never });
    const onFileChange = vi.fn();
    const onFileTreeChange = vi.fn();
    const onTreeChange = vi.fn();
    const stopFile = await files.watchMarkdownFile(
      `${serverWorkspaceRoot}/note.md`,
      onFileChange,
      onFileTreeChange,
    );
    const stopTree = await files.watchMarkdownTree(serverWorkspaceRoot, onTreeChange);

    publish(listeners, documentChangedNotice("revision-2"));
    await vi.waitFor(() => expect(onFileChange).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(onFileTreeChange).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledOnce());

    publish(listeners, {
      documentChange: "snapshot",
      scopes: ["documents", "workspace", "resources"],
    });
    await vi.waitFor(() => expect(onFileChange).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(onFileTreeChange).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledTimes(2));
    expect(schedule).not.toHaveBeenCalled();

    publish(listeners, {
      documentChange: "snapshot",
      scopes: ["documents", "resources"],
    });
    await vi.waitFor(() => expect(onFileChange).toHaveBeenCalledTimes(3));
    await vi.waitFor(() => expect(onFileTreeChange).toHaveBeenCalledTimes(3));
    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledTimes(3));

    stopFile();
    stopTree();
    // The runtime-lifetime inventory listener remains so signed capabilities
    // are marked stale when resource or workspace authority changes.
    expect(listeners).toHaveLength(1);
    publish(listeners, documentChangedNotice("revision-3"));
    expect(onFileChange).toHaveBeenCalledTimes(3);
    expect(onTreeChange).toHaveBeenCalledTimes(3);
  });
});

function publish(
  listeners: ReadonlySet<(notice: KernelInvalidationNotice) => unknown>,
  notice: KernelInvalidationNotice,
) {
  [...listeners].forEach((listener) => listener(notice));
}

function documentChangedNotice(_revisionValue: string): KernelInvalidationNotice {
  return {
    documentChange: "content",
    paths: ["note.md" as never],
    scopes: ["documents", "resources"],
  };
}

function syncSucceededNotice(): KernelInvalidationNotice {
  return {
    documentChange: "snapshot",
    scopes: ["sync-status", "documents", "resources"],
  };
}

function entry(input: {
  kind?: "file" | "directory";
  locator: string;
  name: string;
  parent?: string;
  relativePath: string;
}): KernelDocumentEntrySnapshot {
  return {
    modifiedAt: "2026-07-30T00:00:00Z",
    revision,
    sizeBytes: 5,
    workspaceGeneration: generation,
    kind: input.kind ?? "file",
    locator: input.locator as never,
    name: input.name,
    parent: (input.parent ?? "") as never,
    relativePath: input.relativePath as never,
  };
}

function resource(input: {
  id: string;
  kind?: "attachment" | "image";
  mediaType?: string;
  name: string;
  previewable?: boolean;
  relativePath: string;
}): Awaited<ReturnType<ServerKernelDomainPort["resources"]["list"]>>["items"][number] {
  const separator = input.relativePath.lastIndexOf("/");
  return {
    entryType: "resource",
    resource: {
      id: input.id,
      kind: input.kind ?? "image",
      mediaType: input.mediaType ?? "image/png",
      modifiedAt: "2026-07-30T00:00:00Z",
      name: input.name,
      parent: (separator < 0 ? "" : input.relativePath.slice(0, separator)) as never,
      previewable: input.previewable ?? true,
      relativePath: input.relativePath as never,
      revision: "resource-revision-1" as KernelRevision,
      sizeBytes: 7,
      workspaceGeneration: generation,
    },
  };
}

function kernelPort(): ServerKernelDomainPort {
  const documentEntry = entry({
    locator: "document-1",
    name: "note.md",
    relativePath: "note.md",
  });
  const unavailable = () => vi.fn(async () => {
    throw new Error("not used");
  });
  return {
    appConfig: createUnavailableKernelDomainPort().appConfig,
    availability: "available",
    documents: {
      create: unavailable(),
      delete: unavailable(),
      history: { list: unavailable(), read: unavailable(), restore: unavailable() },
      list: vi.fn<KernelDomainPort["documents"]["list"]>(async () => ({
        items: [documentEntry],
        nextCursor: null,
        workspaceGeneration: generation,
      })),
      move: unavailable(),
      read: vi.fn<KernelDomainPort["documents"]["read"]>(async () => ({
        ...documentEntry,
        contents: "first",
        kind: "file",
      })),
      search: unavailable(),
      update: vi.fn<KernelDomainPort["documents"]["update"]>(async () => ({
        ...documentEntry,
        contents: "second",
        kind: "file",
        revision: "revision-2" as KernelRevision,
        sizeBytes: 6,
      })),
    },
    invalidations: {
      available: false,
      subscribe: () => () => undefined,
    },
    runtime: { read: unavailable() },
    resources: {
      create: unavailable(),
      createBatch: unavailable(),
      imageUrl: vi.fn(({ id, revision }) =>
        `/media/v1/images/${encodeURIComponent(id)}?revision=${encodeURIComponent(revision)}`
      ),
      list: vi.fn(async () => ({ items: [], workspaceGeneration: generation })),
      open: vi.fn(async () => ({
        body: new Blob([new Uint8Array([1])]),
        mediaType: "image/png",
      })),
    },
    serverEvents: {
      available: false,
      subscribe: () => () => undefined,
    },
    settings: { patch: unavailable(), read: unavailable() },
    sync: {
      bindRepository: unavailable(), exportKey: unavailable(), importKey: unavailable(),
      listNotebooks: unavailable(), readKeyState: unavailable(),
      patchConfig: unavailable(),
      readConfig: unavailable(),
      readRepositoryBinding: unavailable(),
      readRun: unavailable(),
      readStatus: unavailable(),
      testConnection: unavailable(),
      trigger: unavailable(),
    },
    workspace: {
      read: vi.fn(async () => ({
        displayName: "Notes",
        generation,
        id: "workspace-1",
        readiness: "ready" as const,
        revision: "workspace-revision-1" as KernelRevision,
      })),
    },
  } satisfies ServerKernelDomainPort;
}
