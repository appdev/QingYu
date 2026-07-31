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
