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

  it("ignores the obsolete recent-directory expansion field", () => {
    const obsoleteExpansionKey = ["recent", "FoldersOpen"].join("");
    expect(normalizeWorkspaceState({
      ...defaultWorkspaceState,
      [obsoleteExpansionKey]: false
    })).toEqual(defaultWorkspaceState);
  });

  it.each([
    {
      absolute: "C:/Notes/nested/day.md",
      document: "c:\\notes\\nested\\day.md",
      relative: "nested/day.md",
      root: "\\\\?\\C:\\Notes"
    },
    {
      absolute: "C:/NOTES/Nested/day.markdown",
      document: "c:/notes/Nested/day.markdown",
      relative: "Nested/day.markdown",
      root: "C:\\NOTES"
    },
    {
      absolute: "//Server/Share/Notes/nested/day.md",
      document: "\\\\?\\UNC\\server\\share\\notes\\nested\\day.md",
      relative: "nested/day.md",
      root: "\\\\Server\\Share\\Notes"
    },
    {
      absolute: "//Server/Share/Notes/nested/day.md",
      document: "//server/share/notes/nested/day.md",
      relative: "nested/day.md",
      root: "//?/UNC/Server/Share/Notes"
    }
  ])("round-trips Windows drive and UNC variants below $root", ({ absolute, document, relative, root }) => {
    expect(managedDocumentRelativePath(root, document)).toBe(relative);
    expect(managedDocumentAbsolutePath(root, relative)).toBe(absolute);
    expect(managedDocumentRelativePath(root, absolute)).toBe(relative);
  });

  it("round-trips a normalized POSIX managed document path", () => {
    expect(managedDocumentRelativePath(
      "/mobile/workspace/",
      "/mobile/workspace/notes\\daily.markdown"
    )).toBe("notes/daily.markdown");
    expect(managedDocumentAbsolutePath("/mobile/workspace/", "notes\\daily.markdown"))
      .toBe("/mobile/workspace/notes/daily.markdown");
  });

  it.each([
    "",
    "notes.txt",
    "/mobile/workspace/notes.md",
    "../outside.md",
    "notes/../outside.md",
    "./notes.md",
    "C:/outside.md"
  ])("rejects unsafe managed document restoration path %j", (relativePath) => {
    expect(managedDocumentAbsolutePath("/mobile/workspace", relativePath)).toBeNull();
  });

  it.each([
    "/mobile/workspace",
    "/mobile/workspace/notes.txt",
    "/mobile/outside.md",
    "/mobile/workspace/../outside.md"
  ])("rejects non-document or outside-root managed file %j", (filePath) => {
    expect(managedDocumentRelativePath("/mobile/workspace", filePath)).toBeNull();
  });

  it.each([
    ["/Notes", "/notes/day.md"],
    ["/mobile/workspace", "/mobile/outside.md"],
    ["C:\\Notes", "C:\\Notes-archive\\day.md"],
    ["\\\\server\\share\\Notes", "\\\\SERVER\\SHARE\\Notes-archive\\day.md"]
  ])("rejects a document outside root %s", (root, document) => {
    expect(managedDocumentRelativePath(root, document)).toBeNull();
  });

  it("normalizes open files, drafts, windows, and split groups", () => {
    expect(normalizeWorkspaceState({
      ...defaultWorkspaceState,
      activeDraftId: "draft-1",
      draftTabs: [{
        content: "draft",
        creationDirectory: " kernel-workspace://primary/abc ",
        id: "draft-1",
        name: " Draft.md ",
        path: null
      }],
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
      draftTabs: [{
        content: "draft",
        creationDirectory: "kernel-workspace://primary/abc",
        id: "draft-1",
        name: "Draft.md",
        path: null
      }],
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
        openFilePaths: ["notes/a.md"],
        openWindows: []
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
