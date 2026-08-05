import {
  createUnavailableKernelDomainPort,
  type KernelDomainPort,
  type KernelDocumentEntrySnapshot,
  type KernelResourceSnapshot,
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

function imageResource(
  overrides: Partial<KernelResourceSnapshot> = {},
): KernelResourceSnapshot {
  return {
    id: "image-1",
    kind: "image",
    mediaType: "image/png",
    modifiedAt: "2026-07-31T00:00:00Z",
    name: "cover.png",
    parent: "" as never,
    previewable: true,
    relativePath: "cover.png" as never,
    revision,
    sizeBytes: 16,
    workspaceGeneration: generation,
    ...overrides,
  };
}

function documentEntry(
  overrides: Partial<KernelDocumentEntrySnapshot> = {},
): KernelDocumentEntrySnapshot {
  return {
    kind: "file",
    locator: "document-1" as never,
    modifiedAt: "2026-07-31T00:00:00Z",
    name: "note.md",
    parent: "" as never,
    relativePath: "note.md" as never,
    revision,
    sizeBytes: 5,
    workspaceGeneration: generation,
    ...overrides,
  };
}

function batchResource(
  id: string,
  name: string,
  mediaType: string,
): KernelResourceSnapshot {
  return {
    id,
    kind: "image",
    mediaType,
    modifiedAt: "2026-07-31T00:00:00Z",
    name,
    parent: "notes/assets" as never,
    previewable: true,
    relativePath: `notes/assets/${name}` as never,
    revision,
    sizeBytes: 16,
    workspaceGeneration: generation,
  };
}

function batchKernel(
  createBatch: KernelDomainPort["resources"]["createBatch"],
  open: KernelDomainPort["resources"]["open"],
  invalidations?: KernelDomainPort["invalidations"],
): KernelDomainPort {
  const unavailable = createUnavailableKernelDomainPort();
  return {
    ...unavailable,
    availability: "available",
    documents: {
      ...unavailable.documents,
      list: vi.fn(async () => ({
        items: [{
          kind: "file" as const,
          locator: "batch-document" as never,
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
    invalidations: invalidations ?? unavailable.invalidations,
    resources: {
      create: unavailable.resources.create,
      createBatch,
      imageUrl: ({ id, revision: resourceRevision }) =>
        `/api/v1/resources/${id}?revision=${resourceRevision}`,
      list: vi.fn(async () => ({ items: [], workspaceGeneration: generation })),
      open,
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
}

describe("Kernel AppRuntime adapter", () => {
  it("delegates containing-folder actions to an explicitly injected native shell", async () => {
    const openContainingFolder = vi.fn(async () => undefined);
    const files = createKernelFileRuntime(createUnavailableKernelDomainPort(), {
      nativeShell: { openContainingFolder },
    });
    const path = `${kernelWorkspaceRoot}/notes/note.md`;

    await expect(files.openContainingFolder(path)).resolves.toBeUndefined();

    expect(openContainingFolder).toHaveBeenCalledWith(path);
  });

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
        createBatch: unavailable.resources.createBatch,
        imageUrl: unavailable.resources.imageUrl,
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

  it("returns exact Kernel rename identity and preserves both files on collision without numbering", async () => {
    const sourceContents = "# A\n\0original source";
    const targetContents = "# B\noriginal target 🐉";
    const source: KernelDocumentEntrySnapshot = {
      kind: "file" as const,
      locator: "document-a" as never,
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "A.md",
      parent: "" as never,
      relativePath: "A.md" as never,
      revision,
      sizeBytes: new TextEncoder().encode(sourceContents).byteLength,
      workspaceGeneration: generation,
    };
    const target: KernelDocumentEntrySnapshot = {
      ...source,
      locator: "document-b" as never,
      name: "B.md",
      relativePath: "B.md" as never,
      sizeBytes: new TextEncoder().encode(targetContents).byteLength,
    };
    const entries = new Map<string, { contents: string; entry: KernelDocumentEntrySnapshot }>([
      [source.locator, { contents: sourceContents, entry: source }],
      [target.locator, { contents: targetContents, entry: target }],
    ]);
    const unavailable = createUnavailableKernelDomainPort();
    const move = vi.fn<KernelDomainPort["documents"]["move"]>(async (input) => {
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
        modifiedAt: "2026-07-31T00:00:01Z",
        name: input.name,
        parent: input.targetParent,
        relativePath: relativePath as never,
        revision: "revision-2" as KernelRevision,
      } satisfies KernelDocumentEntrySnapshot;
      entries.delete(input.locator);
      entries.set(moved.locator, { contents: current.contents, entry: moved });
      return moved;
    });
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [...entries.values()].map(({ entry }) => entry),
          nextCursor: null,
          workspaceGeneration: generation,
        })),
        move,
        read: vi.fn(async ({ locator }) => {
          const document = entries.get(locator);
          if (document === undefined) throw new Error("document unavailable");
          return { ...document.entry, contents: document.contents, kind: "file" as const };
        }),
      },
      resources: {
        ...unavailable.resources,
        list: vi.fn(async () => ({ items: [], workspaceGeneration: generation })),
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
    const files = createKernelFileRuntime(kernel);

    await expect(files.renameMarkdownTreeFile(
      kernelWorkspaceRoot,
      `${kernelWorkspaceRoot}/A.md`,
      "B.md",
    )).rejects.toMatchObject({ code: "document_already_exists" });
    expect(move).toHaveBeenCalledOnce();
    expect(move).toHaveBeenLastCalledWith(expect.objectContaining({ name: "B.md" }));
    const sourceAfterCollision = await files.readMarkdownFile(`${kernelWorkspaceRoot}/A.md`);
    const targetAfterCollision = await files.readMarkdownFile(`${kernelWorkspaceRoot}/B.md`);
    expect(sourceAfterCollision.name).toBe("A.md");
    expect(targetAfterCollision.name).toBe("B.md");
    expect(new TextEncoder().encode(sourceAfterCollision.content))
      .toEqual(new TextEncoder().encode(sourceContents));
    expect(new TextEncoder().encode(targetAfterCollision.content))
      .toEqual(new TextEncoder().encode(targetContents));
    move.mockClear();
    const renamed = await files.renameMarkdownTreeFile(
      kernelWorkspaceRoot,
      `${kernelWorkspaceRoot}/A.md`,
      "C.md",
    );
    expect({
      name: renamed.name,
      path: renamed.path,
      relativePath: renamed.relativePath,
    }).toEqual({
      name: "C.md",
      path: `${kernelWorkspaceRoot}/C.md`,
      relativePath: "C.md",
    });
    expect(move).toHaveBeenCalledOnce();
  });

  it("allocates a numbered name when a new Markdown document already exists", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    const collision = Object.assign(new Error("document already exists"), {
      code: "document_already_exists",
    });
    const create: KernelDomainPort["documents"]["create"] = vi.fn(async (input) => {
      if (input.name !== "Untitled 2.md") throw collision;
      return {
        contents: input.kind === "file" ? input.contents : "",
        kind: "file" as const,
        locator: "document-3" as never,
        modifiedAt: "2026-07-31T00:00:00Z",
        name: "Untitled 2.md",
        parent: "notes" as never,
        relativePath: "notes/Untitled 2.md" as never,
        revision,
        sizeBytes: 7,
        workspaceGeneration: generation,
      };
    });
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: { ...unavailable.documents, create },
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
    const files = createKernelFileRuntime(kernel);

    await expect(files.saveMarkdownFile({
      contents: "# Draft",
      defaultDirectory: `${kernelWorkspaceRoot}/notes`,
      path: null,
      suggestedName: "Untitled.md",
    })).resolves.toEqual({
      name: "Untitled 2.md",
      path: `${kernelWorkspaceRoot}/notes/Untitled%202.md`,
    });

    expect(create).toHaveBeenCalledTimes(3);
    expect(create).toHaveBeenNthCalledWith(1, {
      contents: "# Draft",
      kind: "file",
      name: "Untitled.md",
      parent: "notes",
      workspaceGeneration: generation,
    });
    expect(create).toHaveBeenNthCalledWith(2, {
      contents: "# Draft",
      kind: "file",
      name: "Untitled 1.md",
      parent: "notes",
      workspaceGeneration: generation,
    });
    expect(create).toHaveBeenNthCalledWith(3, {
      contents: "# Draft",
      kind: "file",
      name: "Untitled 2.md",
      parent: "notes",
      workspaceGeneration: generation,
    });
  });

  it("uses the same numbered allocation for file-tree and template document creation", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    const collision = Object.assign(new Error("document already exists"), {
      code: "document_already_exists",
    });
    const create: KernelDomainPort["documents"]["create"] = vi.fn(async (input) => {
      if (input.name !== "Untitled 2.md") throw collision;
      return {
        contents: input.kind === "file" ? input.contents : "",
        kind: "file" as const,
        locator: "document-3" as never,
        modifiedAt: "2026-07-31T00:00:00Z",
        name: input.name,
        parent: "notes" as never,
        relativePath: `notes/${input.name}` as never,
        revision,
        sizeBytes: input.kind === "file" ? input.contents.length : 0,
        workspaceGeneration: generation,
      };
    });
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: { ...unavailable.documents, create },
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
    const files = createKernelFileRuntime(kernel);

    await expect(files.createMarkdownTreeFile(
      kernelWorkspaceRoot,
      "Untitled.md",
      {
        contents: "# Template body",
        parentPath: `${kernelWorkspaceRoot}/notes`,
      },
    )).resolves.toMatchObject({
      name: "Untitled 2.md",
      path: `${kernelWorkspaceRoot}/notes/Untitled%202.md`,
      relativePath: "notes/Untitled 2.md",
    });

    expect(create).toHaveBeenCalledTimes(3);
    expect(create).toHaveBeenNthCalledWith(1, {
      contents: "# Template body",
      kind: "file",
      name: "Untitled.md",
      parent: "notes",
      workspaceGeneration: generation,
    });
    expect(create).toHaveBeenNthCalledWith(2, {
      contents: "# Template body",
      kind: "file",
      name: "Untitled 1.md",
      parent: "notes",
      workspaceGeneration: generation,
    });
    expect(create).toHaveBeenNthCalledWith(3, {
      contents: "# Template body",
      kind: "file",
      name: "Untitled 2.md",
      parent: "notes",
      workspaceGeneration: generation,
    });
  });

  it.each([
    ["Draft.Md", "Draft 1.Md", `${kernelWorkspaceRoot}/Draft%201.Md`],
    ["Draft.MaRkDoWn", "Draft 1.MaRkDoWn", `${kernelWorkspaceRoot}/Draft%201.MaRkDoWn`],
    ["Draft 1.md", "Draft 2.md", `${kernelWorkspaceRoot}/Draft%202.md`],
  ])(
    "preserves the supported Markdown extension when allocating from %s",
    async (suggestedName, numberedName, expectedPath) => {
      const unavailable = createUnavailableKernelDomainPort();
      const collision = Object.assign(new Error("document already exists"), {
        code: "document_already_exists",
      });
      let attempt = 0;
      const create: KernelDomainPort["documents"]["create"] = vi.fn(async (input) => {
        attempt += 1;
        if (attempt === 1) throw collision;
        return {
          contents: input.kind === "file" ? input.contents : "",
          kind: "file" as const,
          locator: "document-2" as never,
          modifiedAt: "2026-07-31T00:00:00Z",
          name: input.name,
          parent: "" as never,
          relativePath: input.name as never,
          revision,
          sizeBytes: 7,
          workspaceGeneration: generation,
        };
      });
      const kernel: KernelDomainPort = {
        ...unavailable,
        availability: "available",
        documents: { ...unavailable.documents, create },
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
      const files = createKernelFileRuntime(kernel);

      await expect(files.saveMarkdownFile({
        contents: "# Draft",
        path: null,
        suggestedName,
      })).resolves.toEqual({ name: numberedName, path: expectedPath });
      expect(create).toHaveBeenNthCalledWith(1, expect.objectContaining({ name: suggestedName }));
      expect(create).toHaveBeenNthCalledWith(2, expect.objectContaining({ name: numberedName }));
    },
  );

  it("does not retry a new Markdown document after a non-collision error", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    const failure = Object.assign(new Error("workspace locked"), {
      code: "workspace_locked",
    });
    const create: KernelDomainPort["documents"]["create"] = vi.fn(async () => {
      throw failure;
    });
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: { ...unavailable.documents, create },
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
    const files = createKernelFileRuntime(kernel);

    await expect(files.saveMarkdownFile({
      contents: "# Draft",
      path: null,
      suggestedName: "Untitled.md",
    })).rejects.toBe(failure);
    expect(create).toHaveBeenCalledOnce();
  });

  it("stops allocating a new Markdown document after 10,000 collisions", async () => {
    const unavailable = createUnavailableKernelDomainPort();
    const collision = Object.assign(new Error("document already exists"), {
      code: "document_already_exists",
    });
    const create: KernelDomainPort["documents"]["create"] = vi.fn(async () => {
      throw collision;
    });
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: { ...unavailable.documents, create },
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
    const files = createKernelFileRuntime(kernel);

    await expect(files.saveMarkdownFile({
      contents: "# Draft",
      path: null,
      suggestedName: "Untitled.md",
    })).rejects.toThrow("Unable to allocate a unique Markdown filename after 10,000 attempts.");
    expect(create).toHaveBeenCalledTimes(10_000);
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
        createBatch: unavailable.resources.createBatch,
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

  it("returns an encoded document-relative image URL and resolves it through the Kernel", async () => {
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
        createBatch: unavailable.resources.createBatch,
        imageUrl: vi.fn(({ id, revision: resourceRevision }) =>
          `/api/v1/resources/${id}?revision=${resourceRevision}`,
        ),
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
    const owner = createKernelFileRuntimeOwner(kernel);
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
    )).toBe("/api/v1/resources/resource-encoded?revision=resource-revision-2");
    owner.release();
  });

  it("imports an ordered image batch with one Kernel call without reopening image bodies", async () => {
    const created = [
      batchResource("batch-1", "one-2.png", "image/png"),
      batchResource("batch-2", "two.webp", "image/webp"),
    ];
    const createBatch = vi.fn(async () => created);
    const open = vi.fn();
    const owner = createKernelFileRuntimeOwner(batchKernel(createBatch, open));
    const first = new File(["upload-one"], "one.png", { type: "image/png" });
    const second = new File(["upload-two"], "two.webp", { type: "image/webp" });

    const saved = await owner.files.saveClipboardImages([
      {
        copyToStorage: true,
        documentPath: `${kernelWorkspaceRoot}/notes/note.md`,
        fileName: "one.png",
        folder: "assets",
        image: first,
      },
      {
        copyToStorage: true,
        documentPath: `${kernelWorkspaceRoot}/notes/note.md`,
        fileName: "two.webp",
        folder: "assets",
        image: second,
      },
    ]);

    expect(saved).toEqual([
      { alt: "one", src: "assets/one-2.png" },
      { alt: "two", src: "assets/two.webp" },
    ]);
    expect(createBatch).toHaveBeenCalledOnce();
    expect(createBatch).toHaveBeenCalledWith(expect.objectContaining({
      documentLocator: "batch-document",
      folder: "assets",
      items: [
        { body: first, kind: "image", mediaType: "image/png", name: "one.png" },
        { body: second, kind: "image", mediaType: "image/webp", name: "two.webp" },
      ],
      workspaceGeneration: generation,
    }));
    expect(open).not.toHaveBeenCalled();
    expect(owner.files.resolveMarkdownImageSrc?.(
      `${kernelWorkspaceRoot}/notes/note.md`,
      "assets/two.webp",
    )).toBe("/api/v1/resources/batch-2?revision=revision-1");
    owner.release();
  });

  it("rejects an incomplete or reordered Kernel batch before caching previews", async () => {
    const png = batchResource("batch-1", "one.png", "image/png");
    const webp = batchResource("batch-2", "two.webp", "image/webp");
    const inputs = [
      {
        copyToStorage: true,
        documentPath: `${kernelWorkspaceRoot}/notes/note.md`,
        fileName: "one.png",
        folder: "assets",
        image: new File(["one"], "one.png", { type: "image/png" }),
      },
      {
        copyToStorage: true,
        documentPath: `${kernelWorkspaceRoot}/notes/note.md`,
        fileName: "two.webp",
        folder: "assets",
        image: new File(["two"], "two.webp", { type: "image/webp" }),
      },
    ];

    for (const response of [[png], [webp, png]]) {
      const open = vi.fn();
      const files = createKernelFileRuntime(batchKernel(vi.fn(async () => response), open));
      await expect(files.saveClipboardImages(inputs)).rejects.toThrow(
        response.length === 1 ? "batch was incomplete" : "batch metadata changed",
      );
      expect(open).not.toHaveBeenCalled();
      expect(files.resolveMarkdownImageSrc?.(
        `${kernelWorkspaceRoot}/notes/note.md`,
        "assets/one.png",
      )).toBeNull();
    }
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
        createBatch: unavailable.resources.createBatch,
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

  it("uses a synthetic workspace root with no host absolute path", () => {
    expect(kernelWorkspaceRoot).toBe("kernel-workspace://primary");
    expect(kernelWorkspaceRoot).not.toMatch(/^\/(?:Users|Volumes|home|data)\//u);
    expect(kernelWorkspaceRoot).not.toMatch(/^[A-Za-z]:\\/u);
  });

  it("marks a missing local Kernel image as handled without claiming remote URLs", () => {
    const files = createKernelFileRuntime(createUnavailableKernelDomainPort());

    expect(files.resolveMarkdownImageSrc?.(
      `${kernelWorkspaceRoot}/note.md`,
      "assets/missing.png",
    )).toBeNull();
    expect(files.resolveMarkdownImageSrc?.(
      `${kernelWorkspaceRoot}/note.md`,
      "https://example.com/missing.png",
    )).toBeUndefined();
  });

  it("requests a watched tree refresh when Kernel resources change", async () => {
    let notify: ((notice: { scopes: readonly ["resources"] }) => unknown) | undefined;
    const files = createKernelFileRuntime(createUnavailableKernelDomainPort(), {
      invalidations: {
        available: true,
        subscribe: (listener) => {
          notify = listener as never;
          return () => undefined;
        },
      },
    });
    const onTreeChange = vi.fn(async () => undefined);

    const stopWatching = await files.watchMarkdownTree(kernelWorkspaceRoot, onTreeChange);
    notify?.({ scopes: ["resources"] });

    await vi.waitFor(() => {
      expect(onTreeChange).toHaveBeenCalledWith(kernelWorkspaceRoot);
    });
    stopWatching();
  });

  it("refreshes workspace identity before rebuilding image inventory after a workspace notice", async () => {
    const secondGeneration = "generation-2" as KernelWorkspaceGeneration;
    let currentGeneration = generation;
    let notify: ((notice: { scopes: readonly ["workspace"] }) => unknown) | undefined;
    const unavailable = createUnavailableKernelDomainPort();
    const readWorkspace = vi.fn(async () => ({
      displayName: "Notes",
      generation: currentGeneration,
      id: "workspace-1",
      readiness: "ready" as const,
      revision,
    }));
    const listDocuments = vi.fn(async () => ({
      items: [],
      nextCursor: null,
      workspaceGeneration: currentGeneration,
    }));
    const listResources = vi.fn(async () => ({
      items: [],
      workspaceGeneration: currentGeneration,
    }));
    const kernel = {
      ...unavailable,
      availability: "available",
      documents: { ...unavailable.documents, list: listDocuments },
      invalidations: {
        available: true,
        subscribe: (listener: (notice: { scopes: readonly ["workspace"] }) => unknown) => {
          notify = listener;
          return () => undefined;
        },
      },
      resources: {
        ...unavailable.resources,
        imageUrl: ({ id, revision: resourceRevision }: {
          id: string;
          revision: KernelRevision;
        }) => `/media/${id}?revision=${resourceRevision}`,
        list: listResources,
      },
      workspace: { read: readWorkspace },
    } as unknown as KernelDomainPort;
    const owner = createKernelFileRuntimeOwner(kernel);
    const onTreeChange = vi.fn(async () => undefined);

    await owner.files.loadMarkdownFilesForPath?.(kernelWorkspaceRoot);
    await owner.files.watchMarkdownTree(kernelWorkspaceRoot, onTreeChange);
    currentGeneration = secondGeneration;
    notify?.({ scopes: ["workspace"] });
    await vi.waitFor(() => expect(onTreeChange).toHaveBeenCalledOnce());
    await owner.files.loadMarkdownFilesForPath?.(kernelWorkspaceRoot);

    expect(readWorkspace).toHaveBeenCalledTimes(2);
    expect(listDocuments).toHaveBeenLastCalledWith(expect.objectContaining({
      workspaceGeneration: secondGeneration,
    }));
    expect(listResources).toHaveBeenLastCalledWith(expect.objectContaining({
      workspaceGeneration: secondGeneration,
    }));
    owner.release();
  });

  it("restarts initial image prewarming when a resource notice invalidates an in-flight inventory", async () => {
    let notify: ((notice: { scopes: readonly ["resources"] }) => unknown) | undefined;
    let resolveFirstInventory: ((value: {
      items: Array<{ entryType: "resource"; resource: KernelResourceSnapshot }>;
      workspaceGeneration: KernelWorkspaceGeneration;
    }) => unknown) | undefined;
    const firstInventory = new Promise<{
      items: Array<{ entryType: "resource"; resource: KernelResourceSnapshot }>;
      workspaceGeneration: KernelWorkspaceGeneration;
    }>((resolve) => {
      resolveFirstInventory = resolve;
    });
    const unavailable = createUnavailableKernelDomainPort();
    const document = {
      kind: "file" as const,
      locator: "document-1" as never,
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "note.md",
      parent: "" as never,
      relativePath: "note.md" as never,
      revision,
      sizeBytes: 5,
      workspaceGeneration: generation,
    };
    const oldImage = imageResource({ id: "image-old" });
    const newImage = imageResource({ id: "image-new" });
    const listResources = vi.fn()
      .mockImplementationOnce(async () => firstInventory)
      .mockResolvedValue({
        items: [{ entryType: "resource", resource: newImage }],
        workspaceGeneration: generation,
      });
    const kernel = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [document],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
        read: vi.fn(async () => ({ ...document, contents: "![Cover](cover.png)" })),
      },
      invalidations: {
        available: true,
        subscribe: (listener: (notice: { scopes: readonly ["resources"] }) => unknown) => {
          notify = listener;
          return () => undefined;
        },
      },
      resources: {
        ...unavailable.resources,
        imageUrl: ({ id }: { id: string }) => `/media/${id}`,
        list: listResources,
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
    const owner = createKernelFileRuntimeOwner(kernel);

    const reading = owner.files.readMarkdownFile(`${kernelWorkspaceRoot}/note.md`);
    await vi.waitFor(() => expect(listResources).toHaveBeenCalledOnce());
    notify?.({ scopes: ["resources"] });
    const concurrentReading = owner.files.readMarkdownFile(`${kernelWorkspaceRoot}/note.md`);
    resolveFirstInventory?.({
      items: [{ entryType: "resource", resource: oldImage }],
      workspaceGeneration: generation,
    });
    await Promise.all([reading, concurrentReading]);

    expect(listResources).toHaveBeenCalledTimes(2);
    expect(owner.files.resolveMarkdownImageSrc?.(
      `${kernelWorkspaceRoot}/note.md`,
      "cover.png",
    )).toBe("/media/image-new");
    owner.release();
  });

  it("does not repopulate the document cache from a superseded workspace listing", async () => {
    const secondGeneration = "generation-2" as KernelWorkspaceGeneration;
    let currentGeneration = generation;
    let notify: ((notice: { scopes: readonly ["workspace"] }) => unknown) | undefined;
    let resolveFirstList: ((value: {
      items: KernelDocumentEntrySnapshot[];
      nextCursor: null;
      workspaceGeneration: KernelWorkspaceGeneration;
    }) => unknown) | undefined;
    const firstList = new Promise<{
      items: KernelDocumentEntrySnapshot[];
      nextCursor: null;
      workspaceGeneration: KernelWorkspaceGeneration;
    }>((resolve) => {
      resolveFirstList = resolve;
    });
    const unavailable = createUnavailableKernelDomainPort();
    const oldDocument = documentEntry({
      locator: "old-document" as never,
      name: "old.md",
      relativePath: "old.md" as never,
      workspaceGeneration: generation,
    });
    const newDocument = documentEntry({
      locator: "new-document" as never,
      name: "new.md",
      relativePath: "new.md" as never,
      workspaceGeneration: secondGeneration,
    });
    const listDocuments = vi.fn()
      .mockImplementationOnce(async () => firstList)
      .mockImplementation(async () => ({
        items: [newDocument],
        nextCursor: null,
        workspaceGeneration: secondGeneration,
      }));
    const kernel = {
      ...unavailable,
      availability: "available",
      documents: { ...unavailable.documents, list: listDocuments },
      invalidations: {
        available: true,
        subscribe: (listener: (notice: { scopes: readonly ["workspace"] }) => unknown) => {
          notify = listener;
          return () => undefined;
        },
      },
      resources: {
        ...unavailable.resources,
        list: vi.fn(async () => ({ items: [], workspaceGeneration: currentGeneration })),
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation: currentGeneration,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    } as unknown as KernelDomainPort;
    const owner = createKernelFileRuntimeOwner(kernel);

    const loading = owner.files.loadMarkdownFilesForPath?.(kernelWorkspaceRoot);
    await vi.waitFor(() => expect(listDocuments).toHaveBeenCalledOnce());
    currentGeneration = secondGeneration;
    notify?.({ scopes: ["workspace"] });
    resolveFirstList?.({
      items: [oldDocument],
      nextCursor: null,
      workspaceGeneration: generation,
    });

    await expect(loading).resolves.toEqual([
      expect.objectContaining({ name: "new.md", relativePath: "new.md" }),
    ]);
    expect(listDocuments).toHaveBeenCalledTimes(2);
    owner.release();
  });
});
