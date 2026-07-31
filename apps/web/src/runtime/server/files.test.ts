import type {
  KernelDocumentEntrySnapshot,
  KernelPageCursor,
  KernelDomainPort,
  KernelInvalidationNotice,
  KernelRevision,
  KernelWorkspaceGeneration,
} from "@markra/app/runtime";
import { KernelApiError } from "@markra/kernel-client";

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

  it("reuses a newly uploaded image Blob and rematerializes it after invalidation", async () => {
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
    const createdEntry = resource({
      id: "image-new.signature",
      name: "pasted-2.png",
      relativePath: "pasted-2.png",
    });
    if (createdEntry.entryType !== "resource") throw new Error("resource fixture expected");
    vi.mocked(kernel.resources.create).mockResolvedValue(createdEntry.resource);
    const canonicalBody = new Blob(["canonical image"], { type: "image/png" });
    vi.mocked(kernel.resources.open).mockResolvedValue({
      body: canonicalBody,
      mediaType: "image/png",
    });
    vi.mocked(kernel.resources.list).mockResolvedValue({
      items: [createdEntry],
      workspaceGeneration: generation,
    });
    const createObjectURL = vi.fn(() => "blob:server-new-image");
    const revokeObjectURL = vi.fn();
    const files = createServerFileRuntime(kernel, {
      objectUrls: { createObjectURL, revokeObjectURL },
    });
    const image = new File(
      [new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])],
      "pasted.png",
      { type: "image/png" },
    );
    const documentPath = `${serverWorkspaceRoot}/note.md`;

    const saved = await files.saveClipboardImage({
      documentPath,
      fileName: "pasted.png",
      folder: "",
      image,
    });

    expect(saved).toEqual({ alt: "pasted", src: "pasted-2.png" });
    expect(createObjectURL).toHaveBeenCalledWith(canonicalBody);
    expect(kernel.resources.open).toHaveBeenCalledWith({
      id: "image-new.signature",
      kind: "image",
      workspaceGeneration: generation,
    });
    expect(files.resolveMarkdownImageSrc?.(documentPath, saved.src))
      .toBe("blob:server-new-image");

    publish(listeners, {
      documentChange: "snapshot",
      scopes: ["resources"],
    });
    await vi.waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith("blob:server-new-image"));
    expect(files.resolveMarkdownImageSrc?.(documentPath, saved.src)).toBeUndefined();

    await files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
    expect(files.resolveMarkdownImageSrc?.(documentPath, saved.src)).toBe(
      "blob:server-new-image",
    );
    expect(kernel.resources.open).toHaveBeenCalledTimes(2);
  });

  it.each(["first open", "full reload"])(
    "materializes seven authenticated image resources after a transient auth bootstrap failure on %s",
    async () => {
      vi.useFakeTimers();
      try {
        const kernel = kernelPort();
        const imageEntries = [
          ["avif", "image/avif"],
          ["bmp", "image/bmp"],
          ["gif", "image/gif"],
          ["jpg", "image/jpeg"],
          ["png", "image/png"],
          ["svg", "image/svg+xml"],
          ["webp", "image/webp"],
        ].map(([extension, mediaType], index) => resource({
          id: `image-${index + 1}.signature`,
          mediaType,
          name: `asset.${extension}`,
          relativePath: `assets/asset.${extension}`,
        }));
        vi.mocked(kernel.documents.list).mockImplementation(async (input) => ({
          items: input.parent === ""
            ? [
                entry({
                  kind: "directory",
                  locator: "assets-folder",
                  name: "assets",
                  relativePath: "assets",
                }),
                entry({ locator: "document-1", name: "note.md", relativePath: "note.md" }),
              ]
            : [],
          nextCursor: null,
          workspaceGeneration: generation,
        }));
        vi.mocked(kernel.resources.list).mockImplementation(async (input) => ({
          items: input.parent === "assets" ? imageEntries : [],
          workspaceGeneration: generation,
        }));
        let activeOpens = 0;
        let maximumConcurrentOpens = 0;
        let openAttempt = 0;
        vi.mocked(kernel.resources.open).mockImplementation(async ({ id }) => {
          activeOpens += 1;
          maximumConcurrentOpens = Math.max(maximumConcurrentOpens, activeOpens);
          openAttempt += 1;
          try {
            if (openAttempt === 1) {
              throw new KernelApiError({
                code: "authentication_unavailable",
                requestId: "123e4567-e89b-42d3-a456-426614174001",
                status: 503,
              });
            }
            const image = imageEntries.find((entry) =>
              entry.entryType === "resource" && entry.resource.id === id
            );
            if (image?.entryType !== "resource") throw new Error("missing image fixture");
            return {
              body: new Blob([id], { type: image.resource.mediaType }),
              mediaType: image.resource.mediaType,
            };
          } finally {
            activeOpens -= 1;
          }
        });
        const createObjectURL = vi.fn((blob: Blob) => `blob:server-image-${blob.type}`);
        const revokeObjectURL = vi.fn();
        const owner = createServerFileRuntimeOwner(kernel, {
          objectUrls: { createObjectURL, revokeObjectURL },
        });

        const loading = owner.files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
        await vi.runAllTimersAsync();
        await expect(loading).resolves.toHaveLength(2);

        expect(kernel.resources.open).toHaveBeenCalledTimes(8);
        expect(maximumConcurrentOpens).toBe(1);
        expect(createObjectURL).toHaveBeenCalledTimes(7);
        for (const entry of imageEntries) {
          if (entry.entryType !== "resource") continue;
          expect(owner.files.resolveMarkdownImageSrc?.(
            `${serverWorkspaceRoot}/note.md`,
            entry.resource.relativePath,
          )).toBe(`blob:server-image-${entry.resource.mediaType}`);
        }

        owner.release();
        expect(revokeObjectURL).toHaveBeenCalledTimes(7);
      } finally {
        vi.useRealTimers();
      }
    },
  );

  it("bounds authentication retry with backoff and returns a redacted terminal error", async () => {
    vi.useFakeTimers();
    const schedule = vi.spyOn(globalThis, "setTimeout");
    try {
      const kernel = kernelWithRootImage();
      vi.mocked(kernel.resources.open).mockRejectedValue(authUnavailable());
      const createObjectURL = vi.fn(() => "blob:must-not-exist");
      const files = createServerFileRuntime(kernel, {
        objectUrls: { createObjectURL, revokeObjectURL: vi.fn() },
      });

      const loading = files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
      const outcome = loading?.then(
        () => ({ error: null }),
        (error: unknown) => ({ error }),
      );
      await vi.runAllTimersAsync();

      const result = await outcome;
      expect(result?.error).toEqual(expect.objectContaining({
        message: "The Server image preview is temporarily unavailable.",
      }));
      expect(kernel.resources.open).toHaveBeenCalledTimes(4);
      expect(schedule.mock.calls.map(([, delay]) => delay)).toEqual([25, 75, 225]);
      expect(createObjectURL).not.toHaveBeenCalled();
      expect(String(result?.error)).not.toContain("123e4567");
      expect(String(result?.error)).not.toContain("root.png");
    } finally {
      schedule.mockRestore();
      vi.useRealTimers();
    }
  });

  it("does not retry a non-transient resource failure", async () => {
    const kernel = kernelWithRootImage();
    vi.mocked(kernel.resources.open).mockRejectedValue(new KernelApiError({
      code: "resource_not_found",
      requestId: "123e4567-e89b-42d3-a456-426614174002",
      status: 404,
    }));
    const createObjectURL = vi.fn(() => "blob:must-not-exist");
    const files = createServerFileRuntime(kernel, {
      objectUrls: { createObjectURL, revokeObjectURL: vi.fn() },
    });

    await expect(files.loadMarkdownFilesForPath?.(serverWorkspaceRoot))
      .rejects.toMatchObject({ code: "resource_not_found", status: 404 });
    expect(kernel.resources.open).toHaveBeenCalledOnce();
    expect(createObjectURL).not.toHaveBeenCalled();
  });

  it.each(["request abort", "owner release"] as const)(
    "cancels pending image authentication retry on %s",
    async (interruption) => {
      vi.useFakeTimers();
      try {
        const kernel = kernelWithRootImage();
        let markOpenStarted: (() => unknown) | undefined;
        const openStarted = new Promise<undefined>((resolve) => {
          markOpenStarted = () => resolve(undefined);
        });
        vi.mocked(kernel.resources.open).mockImplementation(async () => {
          markOpenStarted?.();
          throw authUnavailable();
        });
        const createObjectURL = vi.fn(() => "blob:must-not-exist");
        const owner = createServerFileRuntimeOwner(kernel, {
          objectUrls: { createObjectURL, revokeObjectURL: vi.fn() },
        });
        const abort = new AbortController();
        const loading = owner.files.loadMarkdownFilesForPath?.(serverWorkspaceRoot, {
          signal: abort.signal,
        });
        const outcome = loading?.then(
          () => ({ error: null }),
          (error: unknown) => ({ error }),
        );
        await openStarted;
        expect(kernel.resources.open).toHaveBeenCalledOnce();

        if (interruption === "request abort") abort.abort();
        else owner.release();

        await expect(outcome).resolves.toEqual({
          error: expect.objectContaining({ name: "AbortError" }),
        });
        await vi.runAllTimersAsync();
        expect(kernel.resources.open).toHaveBeenCalledOnce();
        expect(createObjectURL).not.toHaveBeenCalled();
        if (interruption === "request abort") owner.release();
      } finally {
        vi.useRealTimers();
      }
    },
  );

  it("releases a materialized URL when invalidation makes its retry lease stale", async () => {
    vi.useFakeTimers();
    try {
      const listeners = new Set<(notice: KernelInvalidationNotice) => unknown>();
      const kernel = Object.assign(kernelWithRootImage(), {
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
      vi.mocked(kernel.resources.open)
        .mockRejectedValueOnce(authUnavailable())
        .mockResolvedValueOnce({
          body: new Blob(["root"], { type: "image/png" }),
          mediaType: "image/png",
        });
      const revokeObjectURL = vi.fn();
      const owner = createServerFileRuntimeOwner(kernel, {
        objectUrls: {
          createObjectURL: vi.fn(() => "blob:stale-auth-retry"),
          revokeObjectURL,
        },
      });
      const loading = owner.files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
      await vi.waitFor(() => expect(kernel.resources.open).toHaveBeenCalledOnce());

      publish(listeners, { documentChange: "snapshot", scopes: ["resources"] });
      await vi.runAllTimersAsync();
      await expect(loading).resolves.toBeDefined();

      expect(kernel.resources.open).toHaveBeenCalledTimes(2);
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:stale-auth-retry");
      expect(owner.files.resolveMarkdownImageSrc?.(
        `${serverWorkspaceRoot}/note.md`,
        "root.png",
      )).toBeUndefined();
      owner.release();
    } finally {
      vi.useRealTimers();
    }
  });

  it("revokes newly uploaded image URLs when the Server runtime owner is released", async () => {
    const kernel = kernelPort();
    const createdEntry = resource({
      id: "image-owner.signature",
      name: "owner.png",
      relativePath: "owner.png",
    });
    if (createdEntry.entryType !== "resource") throw new Error("resource fixture expected");
    vi.mocked(kernel.resources.create).mockResolvedValue(createdEntry.resource);
    const canonicalBody = new Blob(["canonical owner image"], { type: "image/png" });
    vi.mocked(kernel.resources.open).mockResolvedValue({
      body: canonicalBody,
      mediaType: "image/png",
    });
    const createObjectURL = vi.fn(() => "blob:server-owner-image");
    const revokeObjectURL = vi.fn();
    const owner = createServerFileRuntimeOwner(kernel, {
      objectUrls: { createObjectURL, revokeObjectURL },
    });
    const image = new File(
      [new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])],
      "owner.png",
      { type: "image/png" },
    );
    const documentPath = `${serverWorkspaceRoot}/note.md`;

    const saved = await owner.files.saveClipboardImage({
      documentPath,
      fileName: "owner.png",
      folder: "",
      image,
    });
    expect(owner.files.resolveMarkdownImageSrc?.(documentPath, saved.src))
      .toBe("blob:server-owner-image");
    expect(createObjectURL).toHaveBeenCalledWith(canonicalBody);

    owner.release();
    owner.release();

    expect(revokeObjectURL).toHaveBeenCalledOnce();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:server-owner-image");
    expect(owner.files.resolveMarkdownImageSrc?.(documentPath, saved.src)).toBeUndefined();
  });

  it("prewarms nested image capabilities before returning Markdown and rejects unsafe or non-image sources", async () => {
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
    const createObjectURL = vi.fn((blob: Blob) => `blob:server-${blob.type}`);
    const files = createServerFileRuntime(kernel, {
      objectUrls: { createObjectURL, revokeObjectURL: vi.fn() },
    });
    const documentPath = `${serverWorkspaceRoot}/notes/today.md`;

    expect(files.resolveMarkdownImageSrc?.(documentPath, "../assets/cover%20image.png"))
      .toBeUndefined();
    await files.readMarkdownFile(documentPath);

    const imageUrl = "blob:server-image/png";
    const svgUrl = "blob:server-image/svg+xml";
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
    expect(kernel.resources.open).toHaveBeenCalledTimes(2);
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
    for (const source of [
      "../../outside.png",
      "%2e%2e/%2e%2e/outside.png",
      "../assets/manual.pdf",
      "../assets/cover%2Fimage.png",
      "../assets/%5Cevil.png",
      "../assets/%00evil.png",
      "../assets/bad%encoding.png",
      "../assets//cover.png",
      "https://example.test/cover.png",
      "data:image/png;base64,abc",
      "blob:https://example.test/id",
      "file:///etc/passwd",
      "//example.test/cover.png",
    ]) {
      expect(files.resolveMarkdownImageSrc?.(documentPath, source)).toBeUndefined();
    }
  });

  it("replaces materialized image capabilities after refresh and invalidates them on sync completion", async () => {
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
    let sourceSequence = 0;
    const revokeObjectURL = vi.fn();
    const files = createServerFileRuntime(kernel, {
      objectUrls: {
        createObjectURL: vi.fn(() => `blob:server-version-${++sourceSequence}`),
        revokeObjectURL,
      },
    });
    const documentPath = `${serverWorkspaceRoot}/note.md`;

    await files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png"))
      .toBe("blob:server-version-1");
    imageId = "image-new.signature";
    await files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png"))
      .toBe("blob:server-version-2");
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:server-version-1");

    const onTreeChange = vi.fn();
    await files.watchMarkdownTree(serverWorkspaceRoot, onTreeChange);
    publish(listeners, syncSucceededNotice());

    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledOnce());
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png")).toBeUndefined();

    await files.loadMarkdownFilesForPath?.(serverWorkspaceRoot);
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png"))
      .toBe("blob:server-version-3");
    publish(listeners, {
      documentChange: "snapshot",
      scopes: ["documents", "workspace", "resources"],
    });

    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledTimes(2));
    expect(files.resolveMarkdownImageSrc?.(documentPath, "./cover.png")).toBeUndefined();
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
    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledOnce());
    expect(onFileTreeChange).not.toHaveBeenCalled();

    publish(listeners, {
      documentChange: "snapshot",
      scopes: ["documents", "workspace", "resources"],
    });
    await vi.waitFor(() => expect(onFileChange).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(onFileTreeChange).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledTimes(2));
    expect(schedule).not.toHaveBeenCalled();

    publish(listeners, {
      documentChange: "snapshot",
      scopes: ["documents", "resources"],
    });
    await vi.waitFor(() => expect(onFileChange).toHaveBeenCalledTimes(3));
    await vi.waitFor(() => expect(onFileTreeChange).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledTimes(3));

    stopFile();
    stopTree();
    // The runtime-lifetime image source listener remains so temporary object
    // URLs are revoked when resource or workspace capabilities change.
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
    documentChange: "tree",
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
  } satisfies ServerKernelDomainPort;
}

function kernelWithRootImage() {
  const kernel = kernelPort();
  vi.mocked(kernel.resources.list).mockResolvedValue({
    items: [resource({
      id: "image-root.signature",
      name: "root.png",
      relativePath: "root.png",
    })],
    workspaceGeneration: generation,
  });
  return kernel;
}

function authUnavailable() {
  return new KernelApiError({
    code: "authentication_unavailable",
    requestId: "123e4567-e89b-42d3-a456-426614174001",
    status: 503,
  });
}
