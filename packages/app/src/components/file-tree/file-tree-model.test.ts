import { buildMarkdownFileTree, collectMarkdownMoveTargets } from "./file-tree-model";

describe("file tree model", () => {
  it("reuses a collator instead of calling localeCompare for every name sort comparison", () => {
    const localeCompareSpy = vi.spyOn(String.prototype, "localeCompare");

    try {
      buildMarkdownFileTree([
        { path: "/vault/note-10.md", name: "note-10.md", relativePath: "note-10.md" },
        { path: "/vault/note-2.md", name: "note-2.md", relativePath: "note-2.md" },
        { path: "/vault/docs/readme.md", name: "readme.md", relativePath: "docs/readme.md" }
      ]);

      expect(localeCompareSpy).not.toHaveBeenCalled();
    } finally {
      localeCompareSpy.mockRestore();
    }
  });

  it("keeps protected project metadata out of the shared tree", () => {
    const tree = buildMarkdownFileTree([
      { path: "/vault/note.md", name: "note.md", relativePath: "note.md" },
      { path: "/vault/.qingyu/config.json", name: "config.json", relativePath: ".qingyu/config.json" },
      { path: "/vault/.markra-sync/state.json", name: "state.json", relativePath: ".markra-sync/state.json" }
    ], "/vault");

    expect(tree.map((node) => node.name)).toEqual(["note.md"]);
  });

  it("includes the project root but excludes a moved folder and all descendants from move targets", () => {
    const files = [
      { kind: "folder" as const, path: "/vault/docs", name: "docs", relativePath: "docs" },
      { kind: "folder" as const, path: "/vault/docs/drafts", name: "drafts", relativePath: "docs/drafts" },
      { kind: "folder" as const, path: "/vault/archive", name: "archive", relativePath: "archive" }
    ];
    const tree = buildMarkdownFileTree(files, "/vault");

    expect(collectMarkdownMoveTargets(tree, files[0], "/vault").map((target) => target.path)).toEqual([
      "/vault/archive"
    ]);
  });

  it("excludes the moved item's current parent while retaining other valid destinations", () => {
    const files = [
      { kind: "folder" as const, path: "/vault/docs", name: "docs", relativePath: "docs" },
      { path: "/vault/docs/note.md", name: "note.md", relativePath: "docs/note.md" },
      { kind: "folder" as const, path: "/vault/archive", name: "archive", relativePath: "archive" }
    ];
    const tree = buildMarkdownFileTree(files, "/vault");

    expect(collectMarkdownMoveTargets(tree, files[1], "/vault").map((target) => target.path)).toEqual([
      null,
      "/vault/archive"
    ]);
  });
});
