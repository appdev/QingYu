import {
  createAppConfigSnapshot,
  createSettingsStoreHarness,
  resetSettingsStoreRuntime,
  setupSettingsStoreHarness
} from "../../test/settings-store";
import {
  getStoredFileTreeSortByWorkspace,
  getStoredWorkspaceState,
  saveStoredFileTreeSortForWorkspace,
  saveStoredWorkspaceState
} from "./app-settings";
import {
  defaultWorkspaceState,
  managedDocumentAbsolutePath,
  managedDocumentRelativePath,
  normalizeWorkspaceState
} from "./workspace-state";

const settingsStore = createSettingsStoreHarness();
const appConfig = settingsStore.appConfig;

describe("workspace state settings", () => {
  beforeEach(() => setupSettingsStoreHarness(settingsStore));
  afterEach(() => resetSettingsStoreRuntime());

  it("keeps managed document path containment portable across POSIX and Windows roots", () => {
    expect(managedDocumentRelativePath("/mobile/workspace", "/mobile/workspace/notes/day.md"))
      .toBe("notes/day.md");
    expect(managedDocumentAbsolutePath("/mobile/workspace", "notes/day.md"))
      .toBe("/mobile/workspace/notes/day.md");
    expect(managedDocumentRelativePath("C:\\Notes", "c:\\notes\\nested\\day.md"))
      .toBe("nested/day.md");
    expect(managedDocumentAbsolutePath("C:\\Notes", "nested/day.md"))
      .toBe("C:/Notes/nested/day.md");
  });

  it.each([
    ["/Notes", "/notes/day.md"],
    ["/mobile/workspace", "/mobile/outside.md"],
    ["C:\\Notes", "C:\\Notes-archive\\day.md"]
  ])("rejects a document outside root %s", (root, document) => {
    expect(managedDocumentRelativePath(root, document)).toBeNull();
  });

  it("normalizes open files, drafts, windows, and split groups", () => {
    expect(normalizeWorkspaceState({
      ...defaultWorkspaceState,
      activeDraftId: "draft-1",
      draftTabs: [{ content: "draft", id: "draft-1", name: " Draft.md ", path: null }],
      openFilePaths: ["kernel-workspace://primary/a.md", " ", "kernel-workspace://primary/a.md"],
      openWindows: [{
        filePath: "kernel-workspace://primary/a.md",
        label: "secondary",
        openFilePaths: []
      }],
      sideBySideGroup: {
        primaryFilePath: "kernel-workspace://primary/a.md",
        sideFilePath: "kernel-workspace://primary/b.md"
      }
    })).toEqual({
      activeDraftId: "draft-1",
      draftTabs: [{ content: "draft", id: "draft-1", name: "Draft.md", path: null }],
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: ["kernel-workspace://primary/a.md"],
      openWindows: [{
        filePath: "kernel-workspace://primary/a.md",
        label: "secondary",
        openFilePaths: ["kernel-workspace://primary/a.md"]
      }]
    });
  });

  it("reads the requested window through AppConfig only", async () => {
    vi.mocked(appConfig.readWorkspaceState).mockResolvedValue({
      ...defaultWorkspaceState,
      filePath: "kernel-workspace://primary/secondary.md",
      fileTreeOpen: true,
      openFilePaths: ["kernel-workspace://primary/secondary.md"]
    });

    await expect(getStoredWorkspaceState({ windowLabel: "secondary" })).resolves.toMatchObject({
      filePath: "kernel-workspace://primary/secondary.md",
      fileTreeOpen: true
    });
    expect(appConfig.readWorkspaceState).toHaveBeenCalledWith("secondary");
    expect(settingsStore.loadStore).not.toHaveBeenCalledWith("local-state.json", expect.anything());
  });

  it("persists one canonical layout patch for the targeted window", async () => {
    await saveStoredWorkspaceState({
      filePath: "kernel-workspace://primary/notes/a.md",
      fileTreeOpen: true,
      folderPath: "kernel-workspace://primary/notes",
      openFilePaths: ["kernel-workspace://primary/notes/a.md"]
    }, { windowLabel: "secondary" });

    expect(appConfig.patchState).toHaveBeenCalledWith([{
      patch: {
        filePath: "notes/a.md",
        fileTreeOpen: true,
        folderPath: "notes",
        openFilePaths: ["notes/a.md"]
      },
      type: "patch-ui-layout",
      windowLabel: "secondary"
    }]);
  });

  it("uses the active-workspace sort without a host filesystem key", async () => {
    vi.mocked(appConfig.getSnapshot).mockReturnValue({
      ...createAppConfigSnapshot(),
      localState: {
        ...createAppConfigSnapshot().localState,
        fileTreeSort: { direction: "descending", key: "modifiedAt" }
      }
    });

    await expect(getStoredFileTreeSortByWorkspace()).resolves.toEqual({
      "kernel-workspace://primary": { direction: "descending", key: "modifiedAt" }
    });
    await saveStoredFileTreeSortForWorkspace("kernel-workspace://primary", {
      direction: "ascending",
      key: "createdAt"
    });
    expect(appConfig.patchState).toHaveBeenCalledWith([{
      sort: { direction: "ascending", key: "createdAt" },
      type: "set-file-tree-sort"
    }]);
  });
});
