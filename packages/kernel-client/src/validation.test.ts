import { describe, expect, it } from "vitest";

import { isAppConfigSnapshot } from "./validation.ts";

const draft = {
  content: "draft",
  id: "draft-1",
  name: "Draft.md",
  path: null
};

describe("StoredWorkspaceDraftDto validation", () => {
  it("accepts an absent or valid workspace-relative creation directory", () => {
    expect(isAppConfigSnapshot(snapshotWithDraft(draft))).toBe(true);
    expect(isAppConfigSnapshot(snapshotWithDraft({
      ...draft,
      creationDirectory: "abc/nested"
    }))).toBe(true);
  });

  it.each([
    ["null", null],
    ["unsafe traversal", "../outside"],
    ["absolute", "/absolute"]
  ])("rejects a %s creation directory", (_label, creationDirectory) => {
    expect(isAppConfigSnapshot(snapshotWithDraft({
      ...draft,
      creationDirectory
    }))).toBe(false);
  });

  it("rejects an unknown draft property", () => {
    expect(isAppConfigSnapshot(snapshotWithDraft({
      ...draft,
      unknown: true
    }))).toBe(false);
  });
});

function snapshotWithDraft(storedDraft: unknown) {
  return {
    appConfigVersion: 1,
    localState: {
      fileTreeSort: { direction: "ascending", key: "name" },
      pandocPath: null,
      recentMarkdownFiles: [],
      revision: "state-1",
      uiLayout: {
        openWindows: [],
        schemaVersion: 1,
        windowStates: {
          main: {
            activeDraftId: "draft-1",
            draftTabs: [storedDraft],
            filePath: null,
            fileTreeAssetsVisible: true,
            fileTreeOpen: false,
            folderName: null,
            folderPath: null,
            openFilePaths: [],
            sideBySideGroup: null
          }
        }
      }
    },
    settings: { revision: "settings-1", values: [] },
    workspace: {
      generation: "generation-1",
      id: "123e4567-e89b-42d3-a456-426614174000"
    }
  };
}
