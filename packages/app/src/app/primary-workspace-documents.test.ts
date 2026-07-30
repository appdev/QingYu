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
    const list = listReadmeDocument;
    const read: KernelDomainPort["documents"]["read"] = async (input) => {
      readCalls.push(input);
      return document("notes/readme.md", "saved", {
        revision: "revision:read" as KernelRevision
      });
    };
    const update: KernelDomainPort["documents"]["update"] = async (input) => {
      updateCalls.push(input);
      return document("notes/readme.md", input.contents, {
        revision: "revision:updated" as KernelRevision
      });
    };
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({ list, read, update }),
      workspace: WORKSPACE
    });

    await expect(controller.read(path("notes/readme.md"))).resolves.toMatchObject({
      contents: "saved",
      relativePath: "notes/readme.md",
      revision: "revision:read"
    });
    await expect(controller.update({
      contents: "updated",
      relativePath: path("notes/readme.md")
    })).resolves.toMatchObject({ contents: "updated", revision: "revision:updated" });

    expect(readCalls).toEqual([{
      locator: "signed:notes/readme.md",
      workspaceGeneration: GENERATION
    }]);
    expect(updateCalls).toEqual([{
      contents: "updated",
      expectedRevision: "revision:read",
      locator: "signed:notes/readme.md",
      workspaceGeneration: GENERATION
    }]);
    expect(controller.entries().find((item) => item.relativePath === path("notes/readme.md")))
      .toMatchObject({
      relativePath: "notes/readme.md",
      revision: "revision:updated"
    });
    expect(controller.entries().find((item) => item.relativePath === path("notes/readme.md")))
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

  it("moves a directory with its signed revision and invalidates every descendant sidecar entry", async () => {
    const moveCalls: unknown[] = [];
    const read = vi.fn<KernelDomainPort["documents"]["read"]>();
    const move: KernelDomainPort["documents"]["move"] = async (input) => {
      moveCalls.push(input);
      return entry("archive/notebook", "directory", {
        locator: "signed:notes" as KernelDocumentLocator,
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
      locator: "signed:notes",
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

  it("keeps the controller usable after moving a file and updates its path sidecar", async () => {
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listReadmeDocument,
        move: async () => entry("notes/renamed.md", "file", {
          locator: "signed:notes/readme.md" as KernelDocumentLocator,
          revision: "revision:renamed" as KernelRevision
        })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.move({
      name: "renamed.md",
      relativePath: path("notes/readme.md"),
      targetParent: path("notes")
    })).resolves.toMatchObject({ relativePath: "notes/renamed.md" });
    expect(controller.entries().map((item) => item.relativePath)).toEqual([
      "notes",
      "notes/renamed.md"
    ]);
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
