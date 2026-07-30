import { createUnavailableKernelDomainPort } from "../runtime/kernel-domain";
import type {
  KernelDocumentEntrySnapshot,
  KernelDocumentLocator,
  KernelDocumentSnapshot,
  KernelDomainPort,
  KernelPageCursor,
  KernelRevision,
  KernelSearchPageSnapshot,
  KernelWorkspaceGeneration,
  KernelWorkspaceRelativePath,
  KernelWorkspaceSnapshot
} from "../runtime/kernel-domain";
import { createPrimaryWorkspaceDocumentController } from "./primary-workspace-documents";

const GENERATION = "workspace-generation-1" as KernelWorkspaceGeneration;
const WORKSPACE: KernelWorkspaceSnapshot = {
  displayName: "Notes",
  generation: GENERATION,
  id: "workspace-1",
  readiness: "ready",
  revision: "workspace-revision-1" as KernelRevision
};

function path(value: string) {
  return value as KernelWorkspaceRelativePath;
}

function cursor(value: string) {
  return value as KernelPageCursor;
}

function entry(
  relativePath: string,
  kind: "file" | "directory",
  options: Partial<KernelDocumentEntrySnapshot> = {}
): KernelDocumentEntrySnapshot {
  const segments = relativePath.split("/");
  const name = segments.at(-1) ?? relativePath;
  const parent = segments.slice(0, -1).join("/");

  return {
    kind,
    locator: `signed:${relativePath}` as KernelDocumentLocator,
    modifiedAt: "2026-07-30T00:00:00Z",
    name,
    parent: path(parent),
    relativePath: path(relativePath),
    revision: `revision:${relativePath}` as KernelRevision,
    sizeBytes: kind === "file" ? 12 : 0,
    workspaceGeneration: GENERATION,
    ...options
  };
}

function document(
  relativePath: string,
  contents: string,
  options: Partial<KernelDocumentSnapshot> = {}
): KernelDocumentSnapshot {
  return {
    ...entry(relativePath, "file"),
    contents,
    kind: "file",
    ...options
  };
}

function createKernelPort(
  documents: Partial<KernelDomainPort["documents"]>
): KernelDomainPort {
  const unavailable = createUnavailableKernelDomainPort();
  return {
    ...unavailable,
    availability: "available",
    documents: {
      ...unavailable.documents,
      ...documents
    }
  };
}

const listReadmeDocument: KernelDomainPort["documents"]["list"] = async (input) => ({
  items: input.parent === undefined
    ? [entry("notes", "directory")]
    : input.parent === path("notes")
      ? [entry("notes/readme.md", "file")]
      : [],
  nextCursor: null,
  workspaceGeneration: GENERATION
});

const listRootReadmeDocument: KernelDomainPort["documents"]["list"] = async (input) => ({
  items: input.parent === undefined ? [entry("readme.md", "file")] : [],
  nextCursor: null,
  workspaceGeneration: GENERATION
});

describe("primary workspace document controller", () => {
  it("recursively drains every immediate-child page before exposing the workspace tree", async () => {
    const calls: Array<{ cursor?: KernelPageCursor; parent?: KernelWorkspaceRelativePath }> = [];
    const list: KernelDomainPort["documents"]["list"] = async (input) => {
      calls.push({
        ...(input.cursor === undefined ? {} : { cursor: input.cursor }),
        ...(input.parent === undefined ? {} : { parent: input.parent })
      });

      if (input.parent === undefined && input.cursor === undefined) {
        return {
          items: [entry("notes", "directory"), entry("root.md", "file")],
          nextCursor: cursor("root-page-2"),
          workspaceGeneration: GENERATION
        };
      }
      if (input.parent === undefined && input.cursor === cursor("root-page-2")) {
        return {
          items: [entry("archive", "directory")],
          nextCursor: null,
          workspaceGeneration: GENERATION
        };
      }
      if (input.parent === path("notes")) {
        return {
          items: [entry("notes/readme.md", "file"), entry("notes/deep", "directory")],
          nextCursor: null,
          workspaceGeneration: GENERATION
        };
      }
      if (input.parent === path("notes/deep")) {
        return {
          items: [entry("notes/deep/todo.md", "file")],
          nextCursor: null,
          workspaceGeneration: GENERATION
        };
      }
      return { items: [], nextCursor: null, workspaceGeneration: GENERATION };
    };

    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({ list }),
      workspace: WORKSPACE
    });

    expect(controller.entries().map((document) => document.relativePath)).toEqual([
      "archive",
      "notes",
      "notes/deep",
      "notes/deep/todo.md",
      "notes/readme.md",
      "root.md"
    ]);
    expect(calls).toEqual([
      {},
      { cursor: "root-page-2" },
      { parent: "notes" },
      { parent: "archive" },
      { parent: "notes/deep" }
    ]);
  });

  it("uses the signed sidecar for reads and advances the revision used by the next update", async () => {
    const readCalls: unknown[] = [];
    const updateCalls: unknown[] = [];
    const list = listRootReadmeDocument;
    const read: KernelDomainPort["documents"]["read"] = async (input) => {
      readCalls.push(input);
      return document("readme.md", "saved", {
        revision: "revision:read" as KernelRevision
      });
    };
    const update: KernelDomainPort["documents"]["update"] = async (input) => {
      updateCalls.push(input);
      return document("readme.md", input.contents, {
        revision: "revision:updated" as KernelRevision
      });
    };
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({ list, read, update }),
      workspace: WORKSPACE
    });

    await expect(controller.read(path("readme.md"))).resolves.toMatchObject({
      contents: "saved",
      relativePath: "readme.md",
      revision: "revision:read"
    });
    await expect(controller.update({
      contents: "updated",
      relativePath: path("readme.md")
    })).resolves.toMatchObject({ contents: "updated", revision: "revision:updated" });

    expect(readCalls).toEqual([{
      locator: "signed:readme.md",
      workspaceGeneration: GENERATION
    }]);
    expect(updateCalls).toEqual([{
      contents: "updated",
      expectedRevision: "revision:read",
      locator: "signed:readme.md",
      workspaceGeneration: GENERATION
    }]);
    expect(controller.entries().find((item) => item.relativePath === path("readme.md")))
      .toMatchObject({
      relativePath: "readme.md",
      revision: "revision:updated"
    });
    expect(controller.entries().find((item) => item.relativePath === path("readme.md")))
      .not.toHaveProperty("contents");
  });

  it("creates documents through the Kernel and records the returned sidecar entry", async () => {
    const createCalls: unknown[] = [];
    const create: KernelDomainPort["documents"]["create"] = async (input) => {
      createCalls.push(input);
      return document("notes/created.md", input.kind === "file" ? input.contents : "");
    };
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        create,
        list: async () => ({ items: [], nextCursor: null, workspaceGeneration: GENERATION })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.create({
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: path("notes")
    })).resolves.toMatchObject({
      locator: "signed:notes/created.md",
      relativePath: "notes/created.md"
    });

    expect(createCalls).toEqual([{
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: "notes",
      workspaceGeneration: GENERATION
    }]);
    expect(controller.entries().map((item) => item.relativePath)).toEqual(["notes/created.md"]);
  });

  it("rejects an initial page whose generation does not match the workspace snapshot", async () => {
    const driftedGeneration = "workspace-generation-2" as KernelWorkspaceGeneration;

    await expect(createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: async () => ({
          items: [],
          nextCursor: null,
          workspaceGeneration: driftedGeneration
        })
      }),
      workspace: WORKSPACE
    })).rejects.toMatchObject({
      code: "workspace-generation-drift",
      name: "PrimaryWorkspaceDocumentControllerError"
    });
  });

  it("rejects a repeated list cursor before requesting the same page again", async () => {
    let listCount = 0;
    await expect(createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: async () => {
          listCount += 1;
          if (listCount > 2) throw new Error("cursor loop escaped validation");
          return {
            items: [],
            nextCursor: cursor("repeated-cursor"),
            workspaceGeneration: GENERATION
          };
        }
      }),
      workspace: WORKSPACE
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    expect(listCount).toBe(2);
  });

  it("permanently invalidates after response generation drift and requires a rebuild", async () => {
    const driftedGeneration = "workspace-generation-2" as KernelWorkspaceGeneration;
    let readCount = 0;
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listReadmeDocument,
        read: async () => {
          readCount += 1;
          return document("notes/readme.md", "drifted", {
            workspaceGeneration: driftedGeneration
          });
        }
      }),
      workspace: WORKSPACE
    });

    await expect(controller.read(path("notes/readme.md"))).rejects.toMatchObject({
      code: "workspace-generation-drift"
    });
    await expect(controller.read(path("notes/readme.md"))).rejects.toMatchObject({
      code: "workspace-generation-drift"
    });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "workspace-generation-drift"
    }));
    expect(readCount).toBe(1);
  });

  it("rejects a read response that changes the signed document identity", async () => {
    let readCount = 0;
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listReadmeDocument,
        read: async () => {
          readCount += 1;
          return document("notes/readme.md", "wrong", {
            locator: "signed:different.md" as KernelDocumentLocator
          });
        }
      }),
      workspace: WORKSPACE
    });

    await expect(controller.read(path("notes/readme.md"))).rejects.toMatchObject({
      code: "protocol-mismatch",
      name: "PrimaryWorkspaceDocumentControllerError"
    });
    await expect(controller.read(path("notes/readme.md"))).rejects.toMatchObject({
      code: "protocol-mismatch"
    });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "protocol-mismatch"
    }));
    expect(readCount).toBe(1);
  });

  it("rejects a create response whose returned path differs from the requested path", async () => {
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        create: async () => document("notes/different.md", "created"),
        list: async () => ({ items: [], nextCursor: null, workspaceGeneration: GENERATION })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.create({
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: path("notes")
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "protocol-mismatch"
    }));
  });

  it("permanently rejects a file create response whose contents differ from the request", async () => {
    let createCount = 0;
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        create: async () => {
          createCount += 1;
          return document("created.md", "line\n世界");
        },
        list: async () => ({ items: [], nextCursor: null, workspaceGeneration: GENERATION })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.create({
      contents: "line\r\n世界",
      kind: "file",
      name: "created.md",
      parent: path("")
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    await expect(controller.create({
      contents: "second",
      kind: "file",
      name: "second.md",
      parent: path("")
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    expect(createCount).toBe(1);
  });

  it("permanently rejects an update response whose contents differ from the request", async () => {
    let updateCount = 0;
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listRootReadmeDocument,
        update: async () => {
          updateCount += 1;
          return document("readme.md", "line\n世界", {
            revision: "revision:mismatched-update" as KernelRevision
          });
        }
      }),
      workspace: WORKSPACE
    });

    await expect(controller.update({
      contents: "line\r\n世界",
      relativePath: path("readme.md")
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    await expect(controller.update({
      contents: "second",
      relativePath: path("readme.md")
    })).rejects.toMatchObject({ code: "protocol-mismatch" });
    expect(updateCount).toBe(1);
  });

  it("moves a directory with its signed revision and invalidates every descendant sidecar entry", async () => {
    const moveCalls: unknown[] = [];
    const read = vi.fn<KernelDomainPort["documents"]["read"]>();
    const move: KernelDomainPort["documents"]["move"] = async (input) => {
      moveCalls.push(input);
      return entry("archive/notebook", "directory", {
        locator: "signed:archive/notebook:v2" as KernelDocumentLocator,
        revision: "revision:moved" as KernelRevision
      });
    };
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: async (input) => {
          if (input.parent === undefined) {
            return {
              items: [entry("archive", "directory"), entry("notes", "directory")],
              nextCursor: null,
              workspaceGeneration: GENERATION
            };
          }
          if (input.parent === path("notes")) {
            return {
              items: [entry("notes/deep", "directory"), entry("notes/readme.md", "file")],
              nextCursor: null,
              workspaceGeneration: GENERATION
            };
          }
          if (input.parent === path("notes/deep")) {
            return {
              items: [entry("notes/deep/todo.md", "file")],
              nextCursor: null,
              workspaceGeneration: GENERATION
            };
          }
          return { items: [], nextCursor: null, workspaceGeneration: GENERATION };
        },
        move,
        read
      }),
      workspace: WORKSPACE
    });

    await expect(controller.move({
      name: "notebook",
      relativePath: path("notes"),
      targetParent: path("archive")
    })).resolves.toMatchObject({
      locator: "signed:archive/notebook:v2",
      relativePath: "archive/notebook",
      revision: "revision:moved"
    });

    expect(moveCalls).toEqual([{
      expectedRevision: "revision:notes",
      locator: "signed:notes",
      name: "notebook",
      targetParent: "archive",
      workspaceGeneration: GENERATION
    }]);
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
    await expect(controller.read(path("archive/notebook/readme.md"))).rejects.toMatchObject({
      code: "rebuild-required"
    });
    expect(read).not.toHaveBeenCalled();
  });

  it("replaces a moved file's path-bound locator and never reuses the old locator", async () => {
    const updateCalls: unknown[] = [];
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listRootReadmeDocument,
        move: async () => entry("renamed.md", "file", {
          locator: "signed:renamed.md:v2" as KernelDocumentLocator,
          revision: "revision:renamed" as KernelRevision
        }),
        update: async (input) => {
          updateCalls.push(input);
          return document("renamed.md", input.contents, {
            locator: "signed:renamed.md:v2" as KernelDocumentLocator,
            revision: "revision:updated-after-move" as KernelRevision
          });
        }
      }),
      workspace: WORKSPACE
    });

    await expect(controller.move({
      name: "renamed.md",
      relativePath: path("readme.md"),
      targetParent: path("")
    })).resolves.toMatchObject({
      locator: "signed:renamed.md:v2",
      relativePath: "renamed.md"
    });
    await expect(controller.update({
      contents: "after move",
      relativePath: path("renamed.md")
    })).resolves.toMatchObject({ revision: "revision:updated-after-move" });

    expect(controller.entries().map((item) => item.relativePath)).toEqual([
      "renamed.md"
    ]);
    expect(updateCalls).toEqual([{
      contents: "after move",
      expectedRevision: "revision:renamed",
      locator: "signed:renamed.md:v2",
      workspaceGeneration: GENERATION
    }]);
  });

  it("deletes a directory with its signed revision and invalidates every descendant sidecar entry", async () => {
    const deleteCalls: unknown[] = [];
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        delete: async (input) => {
          deleteCalls.push(input);
          return undefined;
        },
        list: async (input) => ({
          items: input.parent === undefined
            ? [entry("notes", "directory")]
            : input.parent === path("notes")
              ? [entry("notes/readme.md", "file")]
              : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("notes")
    })).resolves.toBeUndefined();

    expect(deleteCalls).toEqual([{
      deletionPolicy: "recoverable",
      expectedRevision: "revision:notes",
      locator: "signed:notes",
      workspaceGeneration: GENERATION
    }]);
    expect(controller.entries()).toEqual([]);
  });

  it("returns a committed nested update and then blocks stale ancestor revisions until rebuild", async () => {
    let updateCount = 0;
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listReadmeDocument,
        update: async (input) => {
          updateCount += 1;
          return document("notes/readme.md", input.contents, {
            revision: "revision:nested-update" as KernelRevision
          });
        }
      }),
      workspace: WORKSPACE
    });

    await expect(controller.update({
      contents: "committed",
      relativePath: path("notes/readme.md")
    })).resolves.toMatchObject({
      contents: "committed",
      revision: "revision:nested-update"
    });
    await expect(controller.update({
      contents: "must not reuse stale parent",
      relativePath: path("notes/readme.md")
    })).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
    expect(updateCount).toBe(1);
  });

  it("returns a committed create under an indexed directory and then requires rebuild", async () => {
    let createCount = 0;
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        create: async (input) => {
          createCount += 1;
          return document(`notes/${input.name}`, input.kind === "file" ? input.contents : "");
        },
        list: async (input) => ({
          items: input.parent === undefined ? [entry("notes", "directory")] : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.create({
      contents: "committed",
      kind: "file",
      name: "created.md",
      parent: path("notes")
    })).resolves.toMatchObject({ relativePath: "notes/created.md" });
    await expect(controller.create({
      contents: "must not reuse stale parent",
      kind: "file",
      name: "second.md",
      parent: path("notes")
    })).rejects.toMatchObject({ code: "rebuild-required" });
    expect(createCount).toBe(1);
  });

  it("returns a committed nested delete and then blocks stale ancestor revisions until rebuild", async () => {
    let deleteCount = 0;
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        delete: async () => {
          deleteCount += 1;
          return undefined;
        },
        list: listReadmeDocument
      }),
      workspace: WORKSPACE
    });

    await expect(controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("notes/readme.md")
    })).resolves.toBeUndefined();
    await expect(controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("notes")
    })).rejects.toMatchObject({ code: "rebuild-required" });
    expect(deleteCount).toBe(1);
  });

  it("returns a committed file move across indexed directories and then requires rebuild", async () => {
    const update = vi.fn<KernelDomainPort["documents"]["update"]>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: async (input) => ({
          items: input.parent === undefined
            ? [entry("archive", "directory"), entry("notes", "directory")]
            : input.parent === path("notes")
              ? [entry("notes/readme.md", "file")]
              : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        }),
        move: async () => entry("archive/readme.md", "file", {
          locator: "signed:archive/readme.md:v2" as KernelDocumentLocator,
          revision: "revision:moved-across-directories" as KernelRevision
        }),
        update
      }),
      workspace: WORKSPACE
    });

    await expect(controller.move({
      name: "readme.md",
      relativePath: path("notes/readme.md"),
      targetParent: path("archive")
    })).resolves.toMatchObject({ relativePath: "archive/readme.md" });
    await expect(controller.update({
      contents: "must not reuse stale parents",
      relativePath: path("archive/readme.md")
    })).rejects.toMatchObject({ code: "rebuild-required" });
    expect(update).not.toHaveBeenCalled();
  });

  it("propagates initialization unavailability and mutation conflicts without wrapping them", async () => {
    const unavailable = new Error("Kernel unavailable");
    const conflict = new Error("revision conflict");
    await expect(createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: async () => {
          throw unavailable;
        }
      }),
      workspace: WORKSPACE
    })).rejects.toBe(unavailable);

    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listReadmeDocument,
        update: async () => {
          throw conflict;
        }
      }),
      workspace: WORKSPACE
    });

    await expect(controller.update({
      contents: "dirty",
      relativePath: path("notes/readme.md")
    })).rejects.toBe(conflict);
    expect(controller.entries().find((item) => item.relativePath === path("notes/readme.md")))
      .toMatchObject({ revision: "revision:notes/readme.md" });
  });

  it("drains Kernel search pages and replaces saved matches with dirty overlay matches without writing", async () => {
    const searchCalls: unknown[] = [];
    const update = vi.fn<KernelDomainPort["documents"]["update"]>();
    const search: KernelDomainPort["documents"]["search"] = async (input) => {
      searchCalls.push(input);
      const page: KernelSearchPageSnapshot = input.cursor === undefined
        ? {
            items: [{
              column: 1,
              document: entry("notes/draft.md", "file"),
              line: 1,
              preview: "needle from saved content"
            }],
            nextCursor: cursor("search-page-2"),
            workspaceGeneration: GENERATION
          }
        : {
            items: [{
              column: 3,
              document: entry("notes/saved.md", "file"),
              line: 4,
              preview: "a needle that remains saved"
            }],
            nextCursor: null,
            workspaceGeneration: GENERATION
          };
      return page;
    };
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: async (input) => ({
          items: input.parent === undefined
            ? [entry("notes", "directory")]
            : input.parent === path("notes")
              ? [entry("notes/draft.md", "file"), entry("notes/saved.md", "file")]
              : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        }),
        search,
        update
      }),
      workspace: WORKSPACE
    });

    const matches = await controller.search({
      dirtyOverlay: [{
        contents: "clean\nfresh needle here\nneedle again",
        relativePath: path("notes/draft.md")
      }],
      query: "needle"
    });

    expect(matches).toEqual([
      {
        column: 7,
        document: expect.objectContaining({ relativePath: "notes/draft.md" }),
        line: 2,
        preview: "fresh needle here"
      },
      {
        column: 1,
        document: expect.objectContaining({ relativePath: "notes/draft.md" }),
        line: 3,
        preview: "needle again"
      },
      {
        column: 3,
        document: expect.objectContaining({ relativePath: "notes/saved.md" }),
        line: 4,
        preview: "a needle that remains saved"
      }
    ]);
    expect(searchCalls).toEqual([
      { query: "needle", workspaceGeneration: GENERATION },
      {
        cursor: "search-page-2",
        query: "needle",
        workspaceGeneration: GENERATION
      }
    ]);
    expect(update).not.toHaveBeenCalled();
  });

  it("validates search identity without overwriting the revision used by mutations", async () => {
    const updateCalls: Array<{ expectedRevision: KernelRevision }> = [];
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listRootReadmeDocument,
        search: async () => ({
          items: [{
            column: 1,
            document: entry("readme.md", "file", {
              revision: "revision:stale-search-snapshot" as KernelRevision
            }),
            line: 1,
            preview: "needle"
          }],
          nextCursor: null,
          workspaceGeneration: GENERATION
        }),
        update: async (input) => {
          updateCalls.push({ expectedRevision: input.expectedRevision });
          return document("readme.md", input.contents, {
            revision: `revision:mutation-${updateCalls.length}` as KernelRevision
          });
        }
      }),
      workspace: WORKSPACE
    });

    await controller.update({ contents: "first", relativePath: path("readme.md") });
    await expect(controller.search({ query: "needle" })).resolves.toHaveLength(1);
    await controller.update({ contents: "second", relativePath: path("readme.md") });

    expect(updateCalls).toEqual([
      { expectedRevision: "revision:readme.md" },
      { expectedRevision: "revision:mutation-1" }
    ]);
  });

  it("rejects duplicate dirty overlay paths before issuing a Kernel search", async () => {
    const search = vi.fn<KernelDomainPort["documents"]["search"]>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({ list: listRootReadmeDocument, search }),
      workspace: WORKSPACE
    });

    await expect(controller.search({
      dirtyOverlay: [
        { contents: "first", relativePath: path("readme.md") },
        { contents: "second", relativePath: path("readme.md") }
      ],
      query: "needle"
    })).rejects.toMatchObject({ code: "invalid-dirty-overlay" });
    expect(search).not.toHaveBeenCalled();
  });

  it("caps merged search results at the Kernel's ten-thousand-match boundary", async () => {
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listRootReadmeDocument,
        search: async () => ({
          items: [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        })
      }),
      workspace: WORKSPACE
    });

    const matches = await controller.search({
      dirtyOverlay: [{
        contents: Array.from({ length: 10_001 }, () => "needle").join("\n"),
        relativePath: path("readme.md")
      }],
      query: "needle"
    });

    expect(matches).toHaveLength(10_000);
    expect(matches.at(-1)).toMatchObject({ column: 1, line: 10_000 });
  });

  it("rejects a search match whose locator disagrees with the indexed path", async () => {
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listReadmeDocument,
        search: async () => ({
          items: [{
            column: 1,
            document: entry("notes/readme.md", "file", {
              locator: "signed:different.md" as KernelDocumentLocator
            }),
            line: 1,
            preview: "needle"
          }],
          nextCursor: null,
          workspaceGeneration: GENERATION
        })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.search({ query: "needle" })).rejects.toMatchObject({
      code: "protocol-mismatch"
    });
  });

  it("rejects a search response that presents a directory as a content match", async () => {
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: async (input) => ({
          items: input.parent === undefined ? [entry("notes", "directory")] : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        }),
        search: async () => ({
          items: [{
            column: 1,
            document: entry("notes", "directory"),
            line: 1,
            preview: "needle"
          }],
          nextCursor: null,
          workspaceGeneration: GENERATION
        })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.search({ query: "needle" })).rejects.toMatchObject({
      code: "protocol-mismatch"
    });
  });

  it("treats an unindexed search result as a protocol mismatch instead of a local lookup miss", async () => {
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: async () => ({ items: [], nextCursor: null, workspaceGeneration: GENERATION }),
        search: async () => ({
          items: [{
            column: 1,
            document: entry("unknown.md", "file"),
            line: 1,
            preview: "needle"
          }],
          nextCursor: null,
          workspaceGeneration: GENERATION
        })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.search({ query: "needle" })).rejects.toMatchObject({
      code: "protocol-mismatch"
    });
  });
});
