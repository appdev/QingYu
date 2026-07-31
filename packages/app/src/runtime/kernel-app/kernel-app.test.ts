import {
  createUnavailableKernelDomainPort,
  type KernelDomainPort,
  type KernelRevision,
  type KernelWorkspaceGeneration,
} from "../kernel-domain";

import {
  createKernelFileRuntime,
  createKernelFileRuntimeOwner,
  kernelWorkspaceRoot,
} from "./files";

const generation = "generation-1" as KernelWorkspaceGeneration;
const revision = "revision-1" as KernelRevision;

describe("Kernel AppRuntime adapter", () => {
  it("routes workspace writes to Kernel and ignores a full legacy file fallback", async () => {
    const legacySave = vi.fn(() => Promise.reject(new Error("legacy writer called")));
    const unavailable = createUnavailableKernelDomainPort();
    const update = vi.fn(async () => ({
      contents: "next",
      kind: "file" as const,
      locator: "document-1" as never,
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "note.md",
      parent: "" as never,
      relativePath: "note.md" as never,
      revision: "revision-2" as KernelRevision,
      sizeBytes: 4,
      workspaceGeneration: generation,
    }));
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [{
            kind: "file" as const,
            locator: "document-1" as never,
            modifiedAt: "2026-07-31T00:00:00Z",
            name: "note.md",
            parent: "" as never,
            relativePath: "note.md" as never,
            revision,
            sizeBytes: 5,
            workspaceGeneration: generation,
          }],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
        update,
      },
      resources: {
        create: unavailable.resources.create,
        list: vi.fn(async () => ({ items: [], workspaceGeneration: generation })),
        open: unavailable.resources.open,
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    };
    const files = createKernelFileRuntime(kernel, {
      nativeShell: { saveMarkdownFile: legacySave } as never,
    });

    await files.listMarkdownFilesForPath(kernelWorkspaceRoot);
    await expect(files.saveMarkdownFile({
      contents: "next",
      path: `${kernelWorkspaceRoot}/note.md`,
      suggestedName: "note.md",
    })).resolves.toEqual({
      name: "note.md",
      path: `${kernelWorkspaceRoot}/note.md`,
    });

    expect(update).toHaveBeenCalledOnce();
    expect(legacySave).not.toHaveBeenCalled();
  });

  it("saves pasted images through the Kernel resource writer without a legacy fallback", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    const create = vi.fn(async () => ({
      id: "resource-1",
      kind: "image" as const,
      mediaType: "image/png",
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "pasted.png",
      parent: "assets" as never,
      previewable: true,
      relativePath: "assets/pasted.png" as never,
      revision: "resource-revision-1" as KernelRevision,
      sizeBytes: 8,
      workspaceGeneration: generation,
    }));
    const legacySave = vi.fn(() => Promise.reject(new Error("legacy image writer called")));
    const kernel = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [{
            kind: "file" as const,
            locator: "document-1" as never,
            modifiedAt: "2026-07-31T00:00:00Z",
            name: "note.md",
            parent: "" as never,
            relativePath: "note.md" as never,
            revision,
            sizeBytes: 5,
            workspaceGeneration: generation,
          }],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
      },
      resources: {
        create,
        list: vi.fn(async () => ({ items: [], workspaceGeneration: generation })),
        open: unavailable.resources.open,
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    } as unknown as KernelDomainPort;
    const files = createKernelFileRuntime(kernel, {
      nativeShell: { saveClipboardImage: legacySave } as never,
    });
    const image = new File([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], "pasted.png", {
      type: "image/png",
    });

    await expect(files.saveClipboardImage({
      documentPath: `${kernelWorkspaceRoot}/note.md`,
      fileName: "pasted.png",
      folder: "assets",
      image,
    })).resolves.toEqual({ alt: "pasted", src: "assets/pasted.png" });

    expect(create).toHaveBeenCalledWith({
      body: image,
      documentLocator: "document-1",
      folder: "assets",
      kind: "image",
      mediaType: "image/png",
      name: "pasted.png",
      workspaceGeneration: generation,
    });
    expect(legacySave).not.toHaveBeenCalled();
  });

  it("returns an encoded document-relative image URL and makes its preview available immediately", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    const created = {
      id: "resource-encoded",
      kind: "image" as const,
      mediaType: "image/png",
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "image one-2.png",
      parent: "notes/assets" as never,
      previewable: true,
      relativePath: "notes/assets/image one-2.png" as never,
      revision: "resource-revision-2" as KernelRevision,
      sizeBytes: 8,
      workspaceGeneration: generation,
    };
    let image: File;
    const materialize = vi.fn(async (_resource, open) => {
      const opened = await open();
      expect(opened.body).toBe(image);
      expect(opened.mediaType).toBe("image/png");
      return "blob:new-image";
    });
    const kernel = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [{
            kind: "file" as const,
            locator: "nested-document" as never,
            modifiedAt: "2026-07-31T00:00:00Z",
            name: "note.md",
            parent: "notes" as never,
            relativePath: "notes/note.md" as never,
            revision,
            sizeBytes: 5,
            workspaceGeneration: generation,
          }],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
      },
      resources: {
        create: vi.fn(async () => created),
        list: vi.fn(async () => ({ items: [], workspaceGeneration: generation })),
        open: vi.fn(async () => Promise.reject(new Error("new upload must not be downloaded"))),
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    } as unknown as KernelDomainPort;
    const owner = createKernelFileRuntimeOwner(kernel, {
      imageSource: { materialize, release: vi.fn() },
    });
    image = new File([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], "image one.png", {
      type: "image/png",
    });

    const saved = await owner.files.saveClipboardImage({
      documentPath: `${kernelWorkspaceRoot}/notes/note.md`,
      fileName: "image one.png",
      folder: "assets",
      image,
    });

    expect(saved).toEqual({ alt: "image one", src: "assets/image%20one-2.png" });
    expect(owner.files.resolveMarkdownImageSrc?.(
      `${kernelWorkspaceRoot}/notes/note.md`,
      saved.src,
    )).toBe("blob:new-image");
    expect(materialize).toHaveBeenCalledWith(created, expect.any(Function));
    owner.release();
  });

  it("saves attachments as raw Kernel resources and returns a document-relative URL", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    const create = vi.fn(async () => ({
      id: "attachment-1",
      kind: "attachment" as const,
      mediaType: "application/octet-stream",
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "road map.pdf",
      parent: "notes/files" as never,
      previewable: false,
      relativePath: "notes/files/road map.pdf" as never,
      revision: "attachment-revision-1" as KernelRevision,
      sizeBytes: 8,
      workspaceGeneration: generation,
    }));
    const kernel = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [{
            kind: "file" as const,
            locator: "nested-document" as never,
            modifiedAt: "2026-07-31T00:00:00Z",
            name: "note.md",
            parent: "notes" as never,
            relativePath: "notes/note.md" as never,
            revision,
            sizeBytes: 5,
            workspaceGeneration: generation,
          }],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
      },
      resources: {
        create,
        list: vi.fn(async () => ({ items: [], workspaceGeneration: generation })),
        open: unavailable.resources.open,
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    } as unknown as KernelDomainPort;
    const files = createKernelFileRuntime(kernel);
    const attachment = new File(["%PDF-1.7"], "road map.pdf", { type: "application/pdf" });

    await expect(files.saveClipboardAttachment({
      attachment,
      documentPath: `${kernelWorkspaceRoot}/notes/note.md`,
      folder: "files",
    })).resolves.toEqual({ label: "road map.pdf", src: "files/road%20map.pdf" });

    expect(create).toHaveBeenCalledWith({
      body: attachment,
      documentLocator: "nested-document",
      folder: "files",
      kind: "attachment",
      mediaType: "application/octet-stream",
      name: "road map.pdf",
      workspaceGeneration: generation,
    });
  });

  it("fails closed for resource and template mutations that Kernel does not expose", async () => {
    const files = createKernelFileRuntime(createUnavailableKernelDomainPort());

    await expect(files.importLocalFile({} as never)).rejects.toThrow("unavailable");
    await expect(files.writeMarkdownTemplateFile("template", "contents"))
      .rejects.toThrow("unavailable");
  });

  it("never executes forbidden legacy workspace writers", async () => {
    const forbidden = {
      deleteMarkdownTemplateFile: vi.fn(() => Promise.reject(new Error("legacy template delete"))),
      importLocalFile: vi.fn(() => Promise.reject(new Error("legacy import"))),
      saveClipboardAttachment: vi.fn(() => Promise.reject(new Error("legacy attachment"))),
      saveClipboardImage: vi.fn(() => Promise.reject(new Error("legacy image"))),
      trashMarkdownAssets: vi.fn(() => Promise.reject(new Error("legacy asset trash"))),
      trashWorkspaceResources: vi.fn(() => Promise.reject(new Error("legacy resource trash"))),
      writeMarkdownTemplateFile: vi.fn(() => Promise.reject(new Error("legacy template write"))),
    };
    const files = createKernelFileRuntime(createUnavailableKernelDomainPort(), {
      nativeShell: forbidden as never,
    });

    await expect(files.deleteMarkdownTemplateFile("template.md")).rejects.toThrow("unavailable");
    await expect(files.importLocalFile({} as never)).rejects.toThrow("unavailable");
    await expect(files.saveClipboardAttachment({} as never)).rejects.toThrow("unavailable");
    await expect(files.saveClipboardImage({} as never)).rejects.toThrow("unavailable");
    await expect(files.trashMarkdownAssets?.({} as never)).rejects.toThrow("unavailable");
    await expect(files.trashWorkspaceResources(kernelWorkspaceRoot, []))
      .rejects.toThrow("unavailable");
    await expect(files.writeMarkdownTemplateFile("template.md", "contents"))
      .rejects.toThrow("unavailable");

    Object.values(forbidden).forEach((operation) => expect(operation).not.toHaveBeenCalled());
  });

  it("materializes authenticated image bodies and releases object sources", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    const listeners = new Set<(notice: {
      scopes: readonly ("resources" | "documents")[];
    }) => unknown>();
    const releaseSource = vi.fn();
    const materialize = vi.fn(async (_resource, open) => {
      const body = await open();
      expect(body.mediaType).toBe("image/png");
      expect(await body.body.text()).toBe("image bytes");
      return "blob:kernel-image-1";
    });
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [{
            kind: "file" as const,
            locator: "document-1" as never,
            modifiedAt: "2026-07-31T00:00:00Z",
            name: "note.md",
            parent: "" as never,
            relativePath: "note.md" as never,
            revision,
            sizeBytes: 5,
            workspaceGeneration: generation,
          }],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
      },
      invalidations: {
        available: true,
        subscribe: (listener) => {
          listeners.add(listener as never);
          return () => listeners.delete(listener as never);
        },
      },
      resources: {
        create: unavailable.resources.create,
        list: vi.fn(async () => ({
          items: [{
            entryType: "resource" as const,
            resource: {
              id: "resource-1",
              kind: "image" as const,
              mediaType: "image/png",
              modifiedAt: "2026-07-31T00:00:00Z",
              name: "cover.png",
              parent: "" as never,
              previewable: true,
              relativePath: "cover.png" as never,
              revision,
              sizeBytes: 11,
              workspaceGeneration: generation,
            },
          }],
          workspaceGeneration: generation,
        })),
        open: vi.fn(async () => ({
          body: new Blob(["image bytes"], { type: "image/png" }),
          mediaType: "image/png",
        })),
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    };
    const owner = createKernelFileRuntimeOwner(kernel, {
      imageSource: { materialize, release: releaseSource },
    });

    await owner.files.loadMarkdownFilesForPath?.(kernelWorkspaceRoot);
    expect(owner.files.resolveMarkdownImageSrc?.(
      `${kernelWorkspaceRoot}/note.md`,
      "./cover.png",
    )).toBe("blob:kernel-image-1");
    expect(materialize).toHaveBeenCalledOnce();

    await owner.files.loadMarkdownFilesForPath?.(kernelWorkspaceRoot);
    expect(materialize).toHaveBeenCalledOnce();

    listeners.forEach((listener) => listener({ scopes: ["resources"] }));
    expect(releaseSource).toHaveBeenCalledWith("blob:kernel-image-1");
    expect(owner.files.resolveMarkdownImageSrc?.(
      `${kernelWorkspaceRoot}/note.md`,
      "./cover.png",
    )).toBeUndefined();

    await owner.files.loadMarkdownFilesForPath?.(kernelWorkspaceRoot);
    owner.release();
    owner.release();
    expect(releaseSource).toHaveBeenCalledTimes(2);
  });

  it("revokes a stale image source that finishes after invalidation", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    let notify: ((notice: { scopes: readonly ["resources"] }) => unknown) | undefined;
    let resolveMaterialized: ((source: string) => unknown) | undefined;
    const releaseSource = vi.fn();
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
      },
      invalidations: {
        available: true,
        subscribe: (listener) => {
          notify = listener as never;
          return () => undefined;
        },
      },
      resources: {
        create: unavailable.resources.create,
        list: vi.fn(async () => ({
          items: [{
            entryType: "resource" as const,
            resource: {
              id: "resource-stale",
              kind: "image" as const,
              mediaType: "image/png",
              modifiedAt: "2026-07-31T00:00:00Z",
              name: "stale.png",
              parent: "" as never,
              previewable: true,
              relativePath: "stale.png" as never,
              revision,
              sizeBytes: 5,
              workspaceGeneration: generation,
            },
          }],
          workspaceGeneration: generation,
        })),
        open: vi.fn(async () => ({
          body: new Blob(["stale"], { type: "image/png" }),
          mediaType: "image/png",
        })),
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    };
    const owner = createKernelFileRuntimeOwner(kernel, {
      imageSource: {
        materialize: async (_resource, open) => {
          await open();
          return new Promise<string>((resolve) => {
            resolveMaterialized = resolve;
          });
        },
        release: releaseSource,
      },
    });

    const loading = owner.files.loadMarkdownFilesForPath?.(kernelWorkspaceRoot);
    await vi.waitFor(() => expect(resolveMaterialized).toBeTypeOf("function"));
    notify?.({ scopes: ["resources"] });
    resolveMaterialized?.("blob:stale-image");
    await loading;

    expect(releaseSource).toHaveBeenCalledWith("blob:stale-image");
    expect(owner.files.resolveMarkdownImageSrc?.(
      `${kernelWorkspaceRoot}/note.md`,
      "./stale.png",
    )).toBeUndefined();
  });

  it("revokes the last image source when owner release aborts after materialization", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    let resolveMaterialized: ((source: string) => unknown) | undefined;
    const releaseSource = vi.fn();
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [], nextCursor: null, workspaceGeneration: generation,
        })),
      },
      resources: {
        create: unavailable.resources.create,
        list: vi.fn(async () => ({
          items: [{
            entryType: "resource" as const,
            resource: {
              id: "resource-aborted",
              kind: "image" as const,
              mediaType: "image/png",
              modifiedAt: "2026-07-31T00:00:00Z",
              name: "aborted.png",
              parent: "" as never,
              previewable: true,
              relativePath: "aborted.png" as never,
              revision,
              sizeBytes: 5,
              workspaceGeneration: generation,
            },
          }],
          workspaceGeneration: generation,
        })),
        open: vi.fn(async () => ({
          body: new Blob(["aborted"], { type: "image/png" }),
          mediaType: "image/png",
        })),
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    };
    const owner = createKernelFileRuntimeOwner(kernel, {
      imageSource: {
        materialize: async () => new Promise<string>((resolve) => {
          resolveMaterialized = resolve;
        }),
        release: releaseSource,
      },
    });

    const abort = new AbortController();
    const loading = owner.files.loadMarkdownFilesForPath?.(kernelWorkspaceRoot, {
      signal: abort.signal,
    });
    await vi.waitFor(() => expect(resolveMaterialized).toBeTypeOf("function"));
    abort.abort();
    resolveMaterialized?.("blob:aborted-image");

    await expect(loading).rejects.toThrow();
    expect(releaseSource).toHaveBeenCalledWith("blob:aborted-image");
    owner.release();
  });

  it("releases materialized image sources when a later prewarm fails", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    const resources = ["first.png", "second.png"].map((name, index) => ({
      entryType: "resource" as const,
      resource: {
        id: `resource-${index + 1}`,
        kind: "image" as const,
        mediaType: "image/png",
        modifiedAt: "2026-07-31T00:00:00Z",
        name,
        parent: "" as never,
        previewable: true,
        relativePath: name as never,
        revision,
        sizeBytes: 5,
        workspaceGeneration: generation,
      },
    }));
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [], nextCursor: null, workspaceGeneration: generation,
        })),
      },
      resources: {
        create: unavailable.resources.create,
        list: vi.fn(async () => ({ items: resources, workspaceGeneration: generation })),
        open: vi.fn(async () => ({
          body: new Blob(["image"], { type: "image/png" }),
          mediaType: "image/png",
        })),
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    };
    const releaseSource = vi.fn();
    let materialized = 0;
    const owner = createKernelFileRuntimeOwner(kernel, {
      imageSource: {
        materialize: async () => {
          materialized += 1;
          if (materialized === 2) throw new Error("second image failed");
          return "blob:first-image";
        },
        release: releaseSource,
      },
    });

    await expect(owner.files.loadMarkdownFilesForPath?.(kernelWorkspaceRoot))
      .rejects.toThrow("second image failed");
    expect(releaseSource).toHaveBeenCalledWith("blob:first-image");
  });

  it("uses a synthetic workspace root with no host absolute path", () => {
    expect(kernelWorkspaceRoot).toBe("kernel-workspace://primary");
    expect(kernelWorkspaceRoot).not.toMatch(/^\/(?:Users|Volumes|home|data)\//u);
    expect(kernelWorkspaceRoot).not.toMatch(/^[A-Za-z]:\\/u);
  });
});
