import type {
  NativeAbsolutePath,
  NativeStandaloneDocumentHandle,
  NativeStandaloneRevision
} from "@markra/app/runtime";
import {
  createDesktopNativeShellPort,
  NativeStandaloneConflictError,
  NativeStandaloneDocumentUnavailableError
} from "./native-shell";

function absolutePath(value: string) {
  return value as NativeAbsolutePath;
}

describe("desktop native shell port", () => {
  it("keeps standalone paths behind opaque handles while reading and writing", async () => {
    let contents = "first";
    const saveMarkdownFile = vi.fn(async (input: { contents: string; path: string | null }) => {
      contents = input.contents;
      return { name: "note.md", path: input.path! };
    });
    const port = createDesktopNativeShellPort({
      newHandle: () => "standalone-1",
      openContainingFolder: vi.fn(async () => undefined),
      openExternalUrl: vi.fn(async () => undefined),
      openMarkdownFile: vi.fn(async () => ({
        content: contents,
        name: "note.md",
        path: "/private/note.md",
        sizeBytes: contents.length
      })),
      openMarkdownFolder: vi.fn(async () => null),
      readMarkdownFile: vi.fn(async (path) => ({
        content: contents,
        name: "note.md",
        path,
        sizeBytes: contents.length
      })),
      resolveMarkdownFolder: vi.fn(),
      resolveMarkdownPath: vi.fn(),
      saveMarkdownFile
    });

    const selected = await port.pickers.pickStandaloneDocument();
    expect(selected).toEqual({ displayName: "note.md", handle: "standalone-1" });
    expect(selected).not.toHaveProperty("path");

    const first = await port.standalone.read(selected!.handle);
    expect(first.contents).toBe("first");
    expect(first).not.toHaveProperty("path");

    const saved = await port.standalone.write({
      contents: "second",
      expectedRevision: first.revision,
      handle: selected!.handle
    });
    expect(saved.contents).toBe("second");
    expect(saved.revision).not.toBe(first.revision);
    expect(saveMarkdownFile).toHaveBeenCalledWith(expect.objectContaining({
      contents: "second",
      path: "/private/note.md",
      suggestedName: "note.md"
    }));
  });

  it("rejects stale standalone writes before touching disk", async () => {
    let contents = "initial";
    const saveMarkdownFile = vi.fn();
    const port = createDesktopNativeShellPort({
      newHandle: () => "standalone-2",
      openContainingFolder: vi.fn(),
      openExternalUrl: vi.fn(),
      openMarkdownFile: vi.fn(async () => ({
        content: contents,
        name: "note.md",
        path: "/private/note.md",
        sizeBytes: contents.length
      })),
      openMarkdownFolder: vi.fn(),
      readMarkdownFile: vi.fn(async (path) => ({
        content: contents,
        name: "note.md",
        path,
        sizeBytes: contents.length
      })),
      resolveMarkdownFolder: vi.fn(),
      resolveMarkdownPath: vi.fn(),
      saveMarkdownFile
    });
    const selected = await port.pickers.pickStandaloneDocument();
    const first = await port.standalone.read(selected!.handle);
    contents = "changed outside";

    await expect(port.standalone.write({
      contents: "local edit",
      expectedRevision: first.revision,
      handle: selected!.handle
    })).rejects.toBeInstanceOf(NativeStandaloneConflictError);
    expect(saveMarkdownFile).not.toHaveBeenCalled();
  });

  it("serializes competing writes so one stale revision cannot overwrite the winner", async () => {
    let contents = "initial";
    const saveMarkdownFile = vi.fn(async (input: { contents: string; path: string | null }) => {
      contents = input.contents;
      return { name: "note.md", path: input.path! };
    });
    const port = createDesktopNativeShellPort({
      newHandle: () => "standalone-queue",
      openContainingFolder: vi.fn(),
      openExternalUrl: vi.fn(),
      openMarkdownFile: vi.fn(async () => ({
        content: contents,
        name: "note.md",
        path: "/private/note.md",
        sizeBytes: contents.length
      })),
      openMarkdownFolder: vi.fn(),
      readMarkdownFile: vi.fn(async (path) => ({
        content: contents,
        name: "note.md",
        path,
        sizeBytes: contents.length
      })),
      resolveMarkdownFolder: vi.fn(),
      resolveMarkdownPath: vi.fn(),
      saveMarkdownFile
    });
    const selected = await port.pickers.pickStandaloneDocument();
    const initial = await port.standalone.read(selected!.handle);

    const outcomes = await Promise.allSettled([
      port.standalone.write({
        contents: "first winner",
        expectedRevision: initial.revision,
        handle: selected!.handle
      }),
      port.standalone.write({
        contents: "stale loser",
        expectedRevision: initial.revision,
        handle: selected!.handle
      })
    ]);

    expect(outcomes[0].status).toBe("fulfilled");
    expect(outcomes[1]).toMatchObject({
      reason: expect.any(NativeStandaloneConflictError),
      status: "rejected"
    });
    expect(saveMarkdownFile).toHaveBeenCalledTimes(1);
    expect(contents).toBe("first winner");
  });

  it("classifies native paths without returning file paths to standalone callers", async () => {
    const port = createDesktopNativeShellPort({
      newHandle: () => "classified-file",
      openContainingFolder: vi.fn(),
      openExternalUrl: vi.fn(),
      openMarkdownFile: vi.fn(),
      openMarkdownFolder: vi.fn(async () => ({ name: "Notes", path: "/data/Notes" })),
      readMarkdownFile: vi.fn(async (path) => ({
        content: "classified",
        name: "note.md",
        path,
        sizeBytes: 10
      })),
      resolveMarkdownFolder: vi.fn(async (path) => ({ name: "Notes", path })),
      resolveMarkdownPath: vi.fn(async (path) => ({ kind: "file" as const, name: "note.md", path })),
      saveMarkdownFile: vi.fn()
    });

    await expect(port.pickers.pickWorkspaceDirectory()).resolves.toEqual({
      absolutePath: "/data/Notes",
      displayName: "Notes"
    });
    const classified = await port.paths.classify(absolutePath("/private/note.md"));
    expect(classified).toEqual({ kind: "standalone-document", handle: "classified-file" });
    expect(classified).not.toHaveProperty("absolutePath");
    await expect(port.standalone.read(
      "missing" as NativeStandaloneDocumentHandle
    )).rejects.toBeInstanceOf(NativeStandaloneDocumentUnavailableError);
  });

  it("reports exact native capability availability", () => {
    const port = createDesktopNativeShellPort({
      newHandle: vi.fn(),
      openContainingFolder: vi.fn(),
      openExternalUrl: vi.fn(),
      openMarkdownFile: vi.fn(),
      openMarkdownFolder: vi.fn(),
      readMarkdownFile: vi.fn(),
      resolveMarkdownFolder: vi.fn(),
      resolveMarkdownPath: vi.fn(),
      saveMarkdownFile: vi.fn()
    });

    expect(port.capabilities).toEqual({
      absolutePathClassification: "available",
      operatingSystemShell: "available",
      pickers: "available",
      standaloneDocuments: "available"
    });
    expect(typeof ("revision" as NativeStandaloneRevision)).toBe("string");
  });
});
