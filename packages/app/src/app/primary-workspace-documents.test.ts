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

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => unknown;
};

function deferred<T>(): Deferred<T> {
  let resolve!: Deferred<T>["resolve"];
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

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

  it("rejects a delayed read instead of rolling back a newer mutation revision", async () => {
    const pendingRead = deferred<KernelDocumentSnapshot>();
    const updateCalls: Array<{ expectedRevision: KernelRevision }> = [];
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listRootReadmeDocument,
        read: async () => pendingRead.promise,
        update: async (input) => {
          updateCalls.push({ expectedRevision: input.expectedRevision });
          return document("readme.md", input.contents, {
            revision: `revision:update-${updateCalls.length}` as KernelRevision
          });
        }
      }),
      workspace: WORKSPACE
    });

    const staleRead = controller.read(path("readme.md"));
    await controller.update({ contents: "newer", relativePath: path("readme.md") });
    pendingRead.resolve(document("readme.md", "older", {
      revision: "revision:stale-read" as KernelRevision
    }));

    await expect(staleRead).rejects.toMatchObject({ code: "operation-superseded" });
    expect(controller.entries()).toContainEqual(expect.objectContaining({
      relativePath: "readme.md",
      revision: "revision:update-1"
    }));
    await controller.update({ contents: "newest", relativePath: path("readme.md") });
    expect(updateCalls).toEqual([
      { expectedRevision: "revision:readme.md" },
      { expectedRevision: "revision:update-1" }
    ]);
  });

  it("requires rebuild when a delayed update response loses its captured sidecar revision", async () => {
    const firstResponse = deferred<KernelDocumentSnapshot>();
    const secondResponse = deferred<KernelDocumentSnapshot>();
    const updateCalls: Array<{ contents: string; expectedRevision: KernelRevision }> = [];
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listRootReadmeDocument,
        update: async (input) => {
          updateCalls.push({ contents: input.contents, expectedRevision: input.expectedRevision });
          return updateCalls.length === 1 ? firstResponse.promise : secondResponse.promise;
        }
      }),
      workspace: WORKSPACE
    });

    const staleUpdate = controller.update({ contents: "first", relativePath: path("readme.md") });
    const newerUpdate = controller.update({ contents: "second", relativePath: path("readme.md") });
    secondResponse.resolve(document("readme.md", "second", {
      revision: "revision:second-update" as KernelRevision
    }));
    await expect(newerUpdate).resolves.toMatchObject({ revision: "revision:second-update" });
    firstResponse.resolve(document("readme.md", "first", {
      revision: "revision:first-update" as KernelRevision
    }));

    await expect(staleUpdate).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
    expect(updateCalls).toEqual([
      { contents: "first", expectedRevision: "revision:readme.md" },
      { contents: "second", expectedRevision: "revision:readme.md" }
    ]);
  });

  it("creates documents through the Kernel and records the returned sidecar entry", async () => {
    const createCalls: unknown[] = [];
    const create: KernelDomainPort["documents"]["create"] = async (input) => {
      createCalls.push(input);
      return document("created.md", input.kind === "file" ? input.contents : "");
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
      parent: path("")
    })).resolves.toMatchObject({
      locator: "signed:created.md",
      relativePath: "created.md"
    });

    expect(createCalls).toEqual([{
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: "",
      workspaceGeneration: GENERATION
    }]);
    expect(controller.entries().map((item) => item.relativePath)).toEqual(["created.md"]);
  });

  it("rejects a non-root create whose parent directory is not indexed before calling the Kernel", async () => {
    const create = vi.fn<KernelDomainPort["documents"]["create"]>(async (input) =>
      document("notes/created.md", input.kind === "file" ? input.contents : ""));
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
    })).rejects.toMatchObject({ code: "document-not-indexed" });
    expect(create).not.toHaveBeenCalled();
  });

  it("rejects a non-root create whose indexed parent is not a directory", async () => {
    const create = vi.fn<KernelDomainPort["documents"]["create"]>(async (input) =>
      document("readme.md/created.md", input.kind === "file" ? input.contents : ""));
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({ create, list: listRootReadmeDocument }),
      workspace: WORKSPACE
    });

    await expect(controller.create({
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: path("readme.md")
    })).rejects.toMatchObject({ code: "directory-required" });
    expect(create).not.toHaveBeenCalled();
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
        create: async () => document("different.md", "created"),
        list: async () => ({ items: [], nextCursor: null, workspaceGeneration: GENERATION })
      }),
      workspace: WORKSPACE
    });

    await expect(controller.create({
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: path("")
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

  it("rejects a move whose non-root target parent is not indexed before calling the Kernel", async () => {
    const move = vi.fn<KernelDomainPort["documents"]["move"]>(async () =>
      entry("archive/readme.md", "file", {
        locator: "signed:archive/readme.md:v2" as KernelDocumentLocator
      }));
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({ list: listRootReadmeDocument, move }),
      workspace: WORKSPACE
    });

    await expect(controller.move({
      name: "readme.md",
      relativePath: path("readme.md"),
      targetParent: path("archive")
    })).rejects.toMatchObject({ code: "document-not-indexed" });
    expect(move).not.toHaveBeenCalled();
  });

  it("rejects a move whose indexed target parent is not a directory", async () => {
    const move = vi.fn<KernelDomainPort["documents"]["move"]>(async () =>
      entry("target.md/readme.md", "file", {
        locator: "signed:target.md/readme.md:v2" as KernelDocumentLocator
      }));
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: async (input) => ({
          items: input.parent === undefined
            ? [entry("readme.md", "file"), entry("target.md", "file")]
            : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        }),
        move
      }),
      workspace: WORKSPACE
    });

    await expect(controller.move({
      name: "readme.md",
      relativePath: path("readme.md"),
      targetParent: path("target.md")
    })).rejects.toMatchObject({ code: "directory-required" });
    expect(move).not.toHaveBeenCalled();
  });

  it("never resurrects a deleted path from a delayed move response", async () => {
    const pendingMove = deferred<KernelDocumentEntrySnapshot>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        delete: async () => undefined,
        list: listRootReadmeDocument,
        move: async () => pendingMove.promise
      }),
      workspace: WORKSPACE
    });

    const staleMove = controller.move({
      name: "renamed.md",
      relativePath: path("readme.md"),
      targetParent: path("")
    });
    await controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("readme.md")
    });
    pendingMove.resolve(entry("renamed.md", "file", {
      locator: "signed:renamed.md:v2" as KernelDocumentLocator,
      revision: "revision:delayed-move" as KernelRevision
    }));

    await expect(staleMove).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
  });

  it("rejects a delayed move when its target path changed while the request was in flight", async () => {
    const pendingMove = deferred<KernelDocumentEntrySnapshot>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        create: async (input) => document("target.md", input.kind === "file" ? input.contents : ""),
        delete: async () => undefined,
        list: listRootReadmeDocument,
        move: async () => pendingMove.promise
      }),
      workspace: WORKSPACE
    });

    const staleMove = controller.move({
      name: "target.md",
      relativePath: path("readme.md"),
      targetParent: path("")
    });
    await controller.create({
      contents: "temporary target",
      kind: "file",
      name: "target.md",
      parent: path("")
    });
    await controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("target.md")
    });
    pendingMove.resolve(entry("target.md", "file", {
      locator: "signed:target.md:v2" as KernelDocumentLocator,
      revision: "revision:delayed-target-move" as KernelRevision
    }));

    await expect(staleMove).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
  });

  it("rejects a delayed move after its captured target parent directory was deleted", async () => {
    const pendingMove = deferred<KernelDocumentEntrySnapshot>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        delete: async () => undefined,
        list: async (input) => ({
          items: input.parent === undefined
            ? [entry("archive", "directory"), entry("readme.md", "file")]
            : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        }),
        move: async () => pendingMove.promise
      }),
      workspace: WORKSPACE
    });

    const staleMove = controller.move({
      name: "readme.md",
      relativePath: path("readme.md"),
      targetParent: path("archive")
    });
    await controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("archive")
    });
    pendingMove.resolve(entry("archive/readme.md", "file", {
      locator: "signed:archive/readme.md:v2" as KernelDocumentLocator,
      revision: "revision:delayed-parent-move" as KernelRevision
    }));

    await expect(staleMove).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
  });

  it("rejects a delayed move after its target parent was recreated with the same identity", async () => {
    const pendingMove = deferred<KernelDocumentEntrySnapshot>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        create: async () => ({ ...entry("archive", "directory"), kind: "directory" as const }),
        delete: async () => undefined,
        list: async (input) => ({
          items: input.parent === undefined
            ? [entry("archive", "directory"), entry("readme.md", "file")]
            : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        }),
        move: async () => pendingMove.promise
      }),
      workspace: WORKSPACE
    });

    const staleMove = controller.move({
      name: "readme.md",
      relativePath: path("readme.md"),
      targetParent: path("archive")
    });
    await controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("archive")
    });
    await controller.create({ kind: "directory", name: "archive", parent: path("") });
    pendingMove.resolve(entry("archive/readme.md", "file", {
      locator: "signed:archive/readme.md:v2" as KernelDocumentLocator,
      revision: "revision:delayed-aba-move" as KernelRevision
    }));

    await expect(staleMove).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
  });

  it("fences a delayed move when deleting its source parent removes the captured source path", async () => {
    const pendingMove = deferred<KernelDocumentEntrySnapshot>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        delete: async () => undefined,
        list: listReadmeDocument,
        move: async () => pendingMove.promise
      }),
      workspace: WORKSPACE
    });

    const staleMove = controller.move({
      name: "renamed.md",
      relativePath: path("notes/readme.md"),
      targetParent: path("")
    });
    await controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("notes")
    });
    pendingMove.resolve(entry("renamed.md", "file", {
      locator: "signed:renamed.md:v2" as KernelDocumentLocator,
      revision: "revision:delayed-source-parent-move" as KernelRevision
    }));

    await expect(staleMove).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
  });

  it("fails closed when a delayed delete returns after the document moved", async () => {
    const pendingDelete = deferred<undefined>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        delete: async () => pendingDelete.promise,
        list: listRootReadmeDocument,
        move: async () => entry("renamed.md", "file", {
          locator: "signed:renamed.md:v2" as KernelDocumentLocator,
          revision: "revision:moved-before-delete" as KernelRevision
        })
      }),
      workspace: WORKSPACE
    });

    const staleDelete = controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("readme.md")
    });
    await controller.move({
      name: "renamed.md",
      relativePath: path("readme.md"),
      targetParent: path("")
    });
    pendingDelete.resolve(undefined);

    await expect(staleDelete).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
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

  it("never resurrects a deleted path from a delayed create response", async () => {
    const firstResponse = deferred<KernelDocumentSnapshot>();
    const secondResponse = deferred<KernelDocumentSnapshot>();
    let createCount = 0;
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        create: async () => {
          createCount += 1;
          return createCount === 1 ? firstResponse.promise : secondResponse.promise;
        },
        delete: async () => undefined,
        list: async () => ({ items: [], nextCursor: null, workspaceGeneration: GENERATION })
      }),
      workspace: WORKSPACE
    });

    const staleCreate = controller.create({
      contents: "first",
      kind: "file",
      name: "created.md",
      parent: path("")
    });
    const newerCreate = controller.create({
      contents: "second",
      kind: "file",
      name: "created.md",
      parent: path("")
    });
    secondResponse.resolve(document("created.md", "second", {
      revision: "revision:second-create" as KernelRevision
    }));
    await newerCreate;
    await controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("created.md")
    });
    firstResponse.resolve(document("created.md", "first", {
      revision: "revision:first-create" as KernelRevision
    }));

    await expect(staleCreate).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
  });

  it("rejects a delayed create after its captured parent directory was deleted", async () => {
    const pendingCreate = deferred<KernelDocumentSnapshot>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        create: async () => pendingCreate.promise,
        delete: async () => undefined,
        list: async (input) => ({
          items: input.parent === undefined ? [entry("notes", "directory")] : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        })
      }),
      workspace: WORKSPACE
    });

    const staleCreate = controller.create({
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: path("notes")
    });
    await controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("notes")
    });
    pendingCreate.resolve(document("notes/created.md", "created"));

    await expect(staleCreate).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
  });

  it("rejects a delayed create after its parent was recreated with the same identity", async () => {
    const pendingChildCreate = deferred<KernelDocumentSnapshot>();
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        create: async (input) => input.parent === path("notes")
          ? pendingChildCreate.promise
          : { ...entry("notes", "directory"), kind: "directory" as const },
        delete: async () => undefined,
        list: async (input) => ({
          items: input.parent === undefined ? [entry("notes", "directory")] : [],
          nextCursor: null,
          workspaceGeneration: GENERATION
        })
      }),
      workspace: WORKSPACE
    });

    const staleCreate = controller.create({
      contents: "created",
      kind: "file",
      name: "created.md",
      parent: path("notes")
    });
    await controller.delete({
      deletionPolicy: "recoverable",
      relativePath: path("notes")
    });
    await controller.create({ kind: "directory", name: "notes", parent: path("") });
    pendingChildCreate.resolve(document("notes/created.md", "created"));

    await expect(staleCreate).rejects.toMatchObject({ code: "rebuild-required" });
    expect(() => controller.entries()).toThrow(expect.objectContaining({
      code: "rebuild-required"
    }));
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

  it("rejects delayed search pagination when a mutation changes the indexed sidecar", async () => {
    const secondPageRequested = deferred<undefined>();
    const secondPage = deferred<KernelSearchPageSnapshot>();
    let searchCount = 0;
    const controller = await createPrimaryWorkspaceDocumentController({
      kernel: createKernelPort({
        list: listRootReadmeDocument,
        search: async () => {
          searchCount += 1;
          if (searchCount === 1) {
            return {
              items: [{
                column: 1,
                document: entry("readme.md", "file"),
                line: 1,
                preview: "first needle"
              }],
              nextCursor: cursor("search-page-2"),
              workspaceGeneration: GENERATION
            };
          }
          secondPageRequested.resolve(undefined);
          return secondPage.promise;
        },
        update: async (input) => document("readme.md", input.contents, {
          revision: "revision:updated-during-search" as KernelRevision
        })
      }),
      workspace: WORKSPACE
    });

    const staleSearch = controller.search({ query: "needle" });
    await secondPageRequested.promise;
    await controller.update({ contents: "newer", relativePath: path("readme.md") });
    secondPage.resolve({
      items: [{
        column: 1,
        document: entry("readme.md", "file"),
        line: 2,
        preview: "second needle"
      }],
      nextCursor: null,
      workspaceGeneration: GENERATION
    });

    await expect(staleSearch).rejects.toMatchObject({ code: "operation-superseded" });
    expect(controller.entries()).toContainEqual(expect.objectContaining({
      relativePath: "readme.md",
      revision: "revision:updated-during-search"
    }));
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
