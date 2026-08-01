import {
  createUnavailableKernelDomainPort,
  type KernelAppConfigSnapshot,
  type KernelAppConfigStateOperation,
  type KernelDomainPort,
  type KernelInvalidationNotice,
  type KernelRevision,
  type KernelWorkspaceGeneration,
  type KernelWorkspaceRelativePath,
} from "../index";
import {
  createKernelAppConfigRuntime,
  kernelWorkspacePathFromRelativePath,
  kernelWorkspaceRelativePathFromPath,
} from "./app-config";
import { kernelWorkspaceRoot } from "./files";

const generation = "generation-1" as KernelWorkspaceGeneration;

describe("Kernel AppConfig runtime", () => {
  it("starts from one immutable bootstrap without issuing another read", () => {
    const harness = appConfigHarness();
    const runtime = createKernelAppConfigRuntime(harness.kernel, async () => "main");

    expect(runtime.bootstrap).toBe(runtime.getSnapshot());
    expect(harness.read).not.toHaveBeenCalled();
    expect(Object.isFrozen(runtime.bootstrap)).toBe(true);
    expect(Object.isFrozen(runtime.bootstrap.localState.uiLayout.windowStates.main)).toBe(true);
  });

  it("joins the selected window state with workspace-level open windows", async () => {
    const harness = appConfigHarness();
    const runtime = createKernelAppConfigRuntime(harness.kernel, async () => "editor-2");

    await expect(runtime.readWorkspaceState()).resolves.toEqual({
      activeDraftId: null,
      draftTabs: [],
      filePath: `${kernelWorkspaceRoot}/notes/editor%20two.md`,
      fileTreeAssetsVisible: true,
      fileTreeOpen: true,
      folderName: "notes",
      folderPath: `${kernelWorkspaceRoot}/notes`,
      openFilePaths: [`${kernelWorkspaceRoot}/notes/editor%20two.md`],
      openWindows: [{
        filePath: `${kernelWorkspaceRoot}/notes/main.md`,
        label: "main",
        openFilePaths: [`${kernelWorkspaceRoot}/notes/main.md`],
      }],
      sideBySideGroup: null,
    });
    await expect(runtime.readWorkspaceState("markra-settings")).resolves.toMatchObject({
      filePath: `${kernelWorkspaceRoot}/notes/main.md`,
    });
    await expect(runtime.readWorkspaceState("missing-window")).resolves.toEqual({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: [],
      openWindows: [{
        filePath: `${kernelWorkspaceRoot}/notes/main.md`,
        label: "main",
        openFilePaths: [`${kernelWorkspaceRoot}/notes/main.md`],
      }],
    });
  });

  it("round-trips canonical pseudo-root paths and rejects paths outside it", () => {
    const relative = "notes/My draft #1.md" as KernelWorkspaceRelativePath;
    const canonical = `${kernelWorkspaceRoot}/notes/My%20draft%20%231.md`;

    expect(kernelWorkspacePathFromRelativePath(relative)).toBe(canonical);
    expect(kernelWorkspaceRelativePathFromPath(canonical)).toBe(relative);
    expect(() => kernelWorkspaceRelativePathFromPath("/workspace/notes/a.md"))
      .toThrow("outside the Kernel workspace");
    expect(() => kernelWorkspaceRelativePathFromPath(`${kernelWorkspaceRoot}/notes/../a.md`))
      .toThrow("invalid");
    expect(() => kernelWorkspaceRelativePathFromPath(`${kernelWorkspaceRoot}/notes%2Fa.md`))
      .toThrow("invalid");
  });

  it.each([
    "/absolute.md",
    "../outside.md",
    "notes//draft.md",
    "notes\\draft.md",
    "notes/./draft.md",
  ])("fails closed when a reload contains malformed relative path %s", async (path) => {
    const harness = appConfigHarness();
    const runtime = createKernelAppConfigRuntime(harness.kernel, async () => "main");
    const malformed = snapshot({
      mainFilePath: path as KernelWorkspaceRelativePath,
      revision: "local-2",
    });
    harness.read.mockResolvedValueOnce(malformed);

    await expect(runtime.reload()).rejects.toThrow("Kernel AppConfig snapshot");
    expect(runtime.getSnapshot().localState.revision).toBe("local-1");
  });

  it("reloads atomically only when workspace identity still matches", async () => {
    const harness = appConfigHarness();
    const runtime = createKernelAppConfigRuntime(harness.kernel, async () => "main");
    harness.read
      .mockResolvedValueOnce(snapshot({ revision: "local-2" }))
      .mockResolvedValueOnce(snapshot({ generation: "generation-2", revision: "local-3" }));

    await expect(runtime.reload()).resolves.toMatchObject({
      localState: { revision: "local-2" },
    });
    await expect(runtime.reload()).rejects.toThrow("workspace identity");
    expect(runtime.getSnapshot().localState.revision).toBe("local-2");
  });

  it("patches with the cached generation and replaces the cache only after validation", async () => {
    const harness = appConfigHarness();
    const runtime = createKernelAppConfigRuntime(harness.kernel, async () => "main");
    const operations = [{ type: "clear-recent-files" }] satisfies readonly KernelAppConfigStateOperation[];
    harness.patchState
      .mockResolvedValueOnce(snapshot({ revision: "local-2" }))
      .mockResolvedValueOnce(snapshot({ generation: "generation-2", revision: "local-3" }));

    await expect(runtime.patchState(operations)).resolves.toMatchObject({
      localState: { revision: "local-2" },
    });
    expect(harness.patchState).toHaveBeenNthCalledWith(1, {
      operations,
      workspaceGeneration: generation,
    });
    await expect(runtime.patchState(operations)).rejects.toThrow("workspace identity");
    expect(runtime.getSnapshot().localState.revision).toBe("local-2");
  });

  it("reloads the cache after an app-config invalidation", async () => {
    const harness = appConfigHarness();
    const runtime = createKernelAppConfigRuntime(harness.kernel, async () => "main");
    harness.read.mockResolvedValueOnce(snapshot({ revision: "local-2" }));

    harness.publish({ scopes: ["app-config"] });

    await vi.waitFor(() => expect(runtime.getSnapshot().localState.revision).toBe("local-2"));
    expect(harness.read).toHaveBeenCalledOnce();
  });
});

function appConfigHarness() {
  const unavailable = createUnavailableKernelDomainPort();
  const listeners = new Set<(notice: KernelInvalidationNotice) => unknown>();
  const read = vi.fn(async () => snapshot());
  const patchState = vi.fn(async () => snapshot());
  const bootstrap = snapshot();
  const kernel = {
    ...unavailable,
    appConfig: { bootstrap, patchState, read },
    availability: "available" as const,
    invalidations: {
      available: true,
      subscribe: (listener: (notice: KernelInvalidationNotice) => unknown) => {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
    },
  } satisfies KernelDomainPort;
  return {
    bootstrap,
    kernel,
    patchState,
    publish: (notice: KernelInvalidationNotice) => {
      for (const listener of listeners) listener(notice);
    },
    read,
  };
}

function snapshot({
  generation: nextGeneration = generation,
  mainFilePath = "notes/main.md" as KernelWorkspaceRelativePath,
  revision = "local-1",
}: {
  generation?: KernelWorkspaceGeneration | string;
  mainFilePath?: KernelWorkspaceRelativePath;
  revision?: string;
} = {}): KernelAppConfigSnapshot {
  const relative = (path: string) => path as KernelWorkspaceRelativePath;
  return {
    appConfigVersion: 1,
    localState: {
      fileTreeSort: { direction: "ascending", key: "name" },
      pandocPath: null,
      recentMarkdownFiles: [{ name: "Main", path: relative("notes/main.md") }],
      revision: revision as KernelRevision,
      uiLayout: {
        openWindows: [{
          filePath: relative("notes/main.md"),
          label: "main",
          openFilePaths: [relative("notes/main.md")],
        }],
        schemaVersion: 1,
        windowStates: {
          main: {
            activeDraftId: null,
            draftTabs: [],
            filePath: mainFilePath,
            fileTreeAssetsVisible: true,
            fileTreeOpen: false,
            folderName: "notes",
            folderPath: relative("notes"),
            openFilePaths: [mainFilePath],
            sideBySideGroup: null,
          },
          "editor-2": {
            activeDraftId: null,
            draftTabs: [],
            filePath: relative("notes/editor two.md"),
            fileTreeAssetsVisible: true,
            fileTreeOpen: true,
            folderName: "notes",
            folderPath: relative("notes"),
            openFilePaths: [relative("notes/editor two.md")],
            sideBySideGroup: null,
          },
        },
      },
    },
    settings: { revision: "settings-1" as KernelRevision, values: [] },
    workspace: {
      generation: nextGeneration as KernelWorkspaceGeneration,
      id: "workspace-1",
    },
  };
}
