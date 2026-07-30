import type {
  KernelDocumentEntrySnapshot,
  KernelPageCursor,
  KernelDomainPort,
  KernelRevision,
  KernelWorkspaceGeneration,
} from "@markra/app/runtime";

import {
  createServerFileRuntime,
  serverWorkspaceRoot,
} from "./files";
import type { ServerKernelDomainPort } from "./kernel";
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
});

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

function kernelPort(): KernelDomainPort {
  const documentEntry = entry({
    locator: "document-1",
    name: "note.md",
    relativePath: "note.md",
  });
  const unavailable = () => vi.fn(async () => {
    throw new Error("not used");
  });
  return {
    availability: "available",
    documents: {
      create: unavailable(),
      delete: unavailable(),
      history: { list: unavailable(), restore: unavailable() },
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
    runtime: { read: unavailable() },
    settings: { patch: unavailable(), read: unavailable() },
    sync: {
      patchConfig: unavailable(),
      readConfig: unavailable(),
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
  } satisfies KernelDomainPort;
}
