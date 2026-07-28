import { createSettingsStoreHarness, resetSettingsStoreRuntime, setupSettingsStoreHarness } from "../../test/settings-store";
import {
  clearStoredRecentMarkdownFiles,
  getStoredRecentMarkdownFiles,
  removeStoredRecentMarkdownFile,
  saveStoredRecentMarkdownFile
} from "./app-settings";
import { normalizeRecentMarkdownFiles } from "./recent-markdown";

const settingsStore = createSettingsStoreHarness();
const { loadStore: mockedLoadStore, store } = settingsStore;

describe("recent markdown settings", () => {
  beforeEach(() => {
    setupSettingsStoreHarness(settingsStore);
  });

  afterEach(() => {
    resetSettingsStoreRuntime();
  });

  it("normalizes recently used markdown files", () => {
    expect(normalizeRecentMarkdownFiles([
      { name: "draft.md", path: "/mock-files/draft.md" },
      { name: "duplicate.md", path: "/mock-files/draft.md" },
      { name: "", path: "/mock-files/research.md" },
      { name: "blank path.md", path: " " },
      null
    ])).toEqual([
      { name: "draft.md", path: "/mock-files/draft.md" },
      { name: "research.md", path: "/mock-files/research.md" }
    ]);
  });

  it("loads recently used markdown files from settings", async () => {
    store.get.mockResolvedValue([
      { name: "draft.md", path: "/mock-files/draft.md" },
      { name: "duplicate.md", path: "/mock-files/draft.md" }
    ]);

    await expect(getStoredRecentMarkdownFiles()).resolves.toEqual([
      { name: "draft.md", path: "/mock-files/draft.md" }
    ]);
    expect(store.get).toHaveBeenCalledWith("recentMarkdownFiles");
  });

  it("prepends and persists a recently used markdown file", async () => {
    store.get.mockResolvedValue([
      { name: "draft.md", path: "/mock-files/draft.md" },
      { name: "notes old.md", path: "/mock-files/notes.md" }
    ]);

    await saveStoredRecentMarkdownFile({
      name: "notes.md",
      path: "/mock-files/notes.md"
    });

    expect(store.set).toHaveBeenCalledWith("recentMarkdownFiles", [
      { name: "notes.md", path: "/mock-files/notes.md" },
      { name: "draft.md", path: "/mock-files/draft.md" }
    ]);
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it("removes a recently used markdown file", async () => {
    store.get.mockResolvedValue([
      { name: "draft.md", path: "/mock-files/draft.md" },
      { name: "notes.md", path: "/mock-files/notes.md" }
    ]);

    await expect(removeStoredRecentMarkdownFile("/mock-files/draft.md")).resolves.toEqual([
      { name: "notes.md", path: "/mock-files/notes.md" }
    ]);

    expect(store.set).toHaveBeenCalledWith("recentMarkdownFiles", [
      { name: "notes.md", path: "/mock-files/notes.md" }
    ]);
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it("clears recently used markdown files", async () => {
    await clearStoredRecentMarkdownFiles();

    expect(store.delete).toHaveBeenCalledWith("recentMarkdownFiles");
    expect(store.save).toHaveBeenCalledTimes(1);
  });
});
