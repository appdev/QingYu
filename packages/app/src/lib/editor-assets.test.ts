import {
  persistRemoteEditorImage,
  resolveEditorAssetAction,
  resolveEditorAssetContext
} from "./editor-assets";

describe("editor asset policy", () => {
  it.each([
    ["primary-workspace", "clipboard", "copy-workspace"],
    ["primary-workspace", "drop", "copy-workspace"],
    ["primary-workspace", "import", "copy-workspace"],
    ["primary-workspace", "remote", "copy-workspace"],
    ["standalone", "clipboard", "copy-document"],
    ["standalone", "drop", "reference"],
    ["standalone", "import", "reference"],
    ["standalone", "remote", "reference"]
  ] as const)("uses %s/%s resource policy", (mode, origin, expected) => {
    expect(resolveEditorAssetAction({ mode, origin })).toBe(expected);
  });

  it("uses root assets for a nested primary note without consulting sync configuration", () => {
    expect(resolveEditorAssetContext({
      documentPath: "/Notes/journal/2026/day.md",
      primaryWorkspaceRoot: "/Notes"
    })).toEqual({
      mode: "primary-workspace",
      primaryRootPath: "/Notes"
    });
  });

  it("uses primary assets for a canonical Windows-style nested note", () => {
    expect(resolveEditorAssetContext({
      documentPath: "C:\\Notes\\journal\\day.markdown",
      primaryWorkspaceRoot: "C:\\Notes"
    })).toEqual({ mode: "primary-workspace", primaryRootPath: "C:\\Notes" });
  });

  it.each([
    "kernel-workspace://primary/notes/day.md",
    "kernel-workspace://primary/notes/%E6%97%A5%E8%AE%B0.md"
  ])("uses an explicit managed URI root for %s", (documentPath) => {
    expect(resolveEditorAssetContext({
      documentPath,
      managedWorkspaceRoot: "kernel-workspace://primary",
      primaryWorkspaceRoot: "/private/tmp/QingYu/Workspace/Notes"
    })).toEqual({
      mode: "primary-workspace",
      primaryRootPath: "kernel-workspace://primary"
    });
  });

  it.each([
    "kernel-workspace://primary-archive/notes/day.md",
    "kernel-workspace://primary/%2e%2e/outside.md",
    "kernel-workspace://primary/notes%2Foutside.md",
    "kernel-workspace://primary/notes%5Coutside.md",
    "kernel-workspace://primary/notes/%ZZ.md",
    "kernel-workspace://primary/notes/image.png",
    "/private/tmp/QingYu/Workspace/Notes/day.md"
  ])("does not escape or fall back from an explicit managed URI root for %s", (documentPath) => {
    expect(resolveEditorAssetContext({
      documentPath,
      managedWorkspaceRoot: "kernel-workspace://primary",
      primaryWorkspaceRoot: "/private/tmp/QingYu/Workspace/Notes"
    })).toEqual({ mode: "standalone" });
  });

  it.each([
    [null, "/Notes"],
    ["/Notes/day.md", null],
    ["/External/day.md", "/Notes"],
    ["/Notes-archive/day.md", "/Notes"],
    ["/Notes/../External/day.md", "/Notes"],
    ["/Notes/image.png", "/Notes"]
  ] as const)("uses standalone mode for document %j and primary root %j", (documentPath, primaryWorkspaceRoot) => {
    expect(resolveEditorAssetContext({
      documentPath,
      primaryWorkspaceRoot
    })).toEqual({ mode: "standalone" });
  });

  it("keeps standalone remote images as URLs without downloading", async () => {
    const download = vi.fn();
    const save = vi.fn();

    await expect(persistRemoteEditorImage({
      alt: "Kitten",
      context: { mode: "standalone" },
      download,
      save,
      url: "https://images.example.test/kitten.png"
    })).resolves.toEqual({ alt: "Kitten", src: "https://images.example.test/kitten.png" });
    expect(download).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
  });

  it("downloads primary-workspace remote images before workspace persistence", async () => {
    const file = new File([new Uint8Array([1])], "kitten.png", { type: "image/png" });
    const download = vi.fn().mockResolvedValue(file);
    const save = vi.fn().mockResolvedValue({ alt: "kitten", src: "../assets/kitten.png" });

    await expect(persistRemoteEditorImage({
      alt: "Kitten",
      context: { mode: "primary-workspace", primaryRootPath: "/vault" },
      download,
      save,
      url: "https://images.example.test/kitten.png"
    })).resolves.toEqual({ alt: "kitten", src: "../assets/kitten.png" });
    expect(download).toHaveBeenCalledWith("https://images.example.test/kitten.png");
    expect(save).toHaveBeenCalledWith(file);
  });
});
