import {
  createAppConfigSnapshot,
  createSettingsStoreHarness,
  resetSettingsStoreRuntime,
  setupSettingsStoreHarness
} from "../../test/settings-store";
import {
  clearStoredRecentMarkdownFiles,
  getStoredRecentMarkdownFiles,
  removeStoredRecentMarkdownFile,
  saveStoredRecentMarkdownFile
} from "./app-settings";
import { normalizeRecentMarkdownFiles } from "./recent-markdown";

const settingsStore = createSettingsStoreHarness();
const appConfig = settingsStore.appConfig;

describe("recent markdown settings", () => {
  beforeEach(() => setupSettingsStoreHarness(settingsStore));
  afterEach(() => resetSettingsStoreRuntime());

  it("normalizes recently used markdown files", () => {
    expect(normalizeRecentMarkdownFiles([
      { name: "draft.md", path: "kernel-workspace://primary/draft.md" },
      { name: "duplicate.md", path: "kernel-workspace://primary/draft.md" },
      { name: "", path: "kernel-workspace://primary/research.md" },
      { name: "blank path.md", path: " " },
      null
    ])).toEqual([
      { name: "draft.md", path: "kernel-workspace://primary/draft.md" },
      { name: "research.md", path: "kernel-workspace://primary/research.md" }
    ]);
  });

  it("loads relative Kernel paths as canonical recent files", async () => {
    vi.mocked(appConfig.getSnapshot).mockReturnValue({
      ...createAppConfigSnapshot(),
      localState: {
        ...createAppConfigSnapshot().localState,
        recentMarkdownFiles: [{ name: "draft.md", path: "draft.md" as never }]
      }
    });

    await expect(getStoredRecentMarkdownFiles()).resolves.toEqual([{
      name: "draft.md",
      path: "kernel-workspace://primary/draft.md"
    }]);
  });

  it("persists remember, remove, and clear as semantic operations", async () => {
    await saveStoredRecentMarkdownFile({
      name: "notes.md",
      path: "kernel-workspace://primary/notes.md"
    });
    await removeStoredRecentMarkdownFile("kernel-workspace://primary/notes.md");
    await clearStoredRecentMarkdownFiles();

    expect(vi.mocked(appConfig.patchState).mock.calls.map(([operations]) => operations)).toEqual([
      [{ type: "remember-recent-file", file: { name: "notes.md", path: "notes.md" } }],
      [{ type: "remove-recent-file", path: "notes.md" }],
      [{ type: "clear-recent-files" }]
    ]);
    expect(settingsStore.loadStore).not.toHaveBeenCalledWith("local-state.json", expect.anything());
  });
});
