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
import type { DesktopNativeShellDependencies } from "./native-shell";

function absolutePath(value: string) {
  return value as NativeAbsolutePath;
}

function nativeRevision(hexDigit: string) {
  return `native-v2-${hexDigit.repeat(64)}` as NativeStandaloneRevision;
}

function dependencies(
  overrides: Partial<DesktopNativeShellDependencies> = {}
): DesktopNativeShellDependencies {
  return {
    newHandle: () => "standalone-default",
    openContainingFolder: vi.fn(async () => undefined),
    openExternalUrl: vi.fn(async () => undefined),
    openMarkdownFile: vi.fn(async () => null),
    openMarkdownFolder: vi.fn(async () => null),
    readStandaloneDocument: vi.fn(async () => ({
      contents: "initial",
      displayName: "note.md",
      revision: nativeRevision("a")
    })),
    resolveMarkdownFolder: vi.fn(),
    resolveMarkdownPath: vi.fn(),
    writeStandaloneDocumentCas: vi.fn(async (input) => ({
      contents: input.contents,
      displayName: "note.md",
      revision: nativeRevision("b")
    })),
    ...overrides
  };
}

describe("desktop native shell port", () => {
  it("keeps standalone paths behind opaque handles while reading and writing", async () => {
    const writeStandaloneDocumentCas = vi.fn(async (input: {
      contents: string;
      expectedRevision: NativeStandaloneRevision;
      path: string;
    }) => ({
      contents: input.contents,
      displayName: "note.md",
      revision: nativeRevision("b")
    }));
    const port = createDesktopNativeShellPort(dependencies({
      newHandle: () => "standalone-1",
      openMarkdownFile: vi.fn(async () => ({
        content: "picker payload",
        name: "note.md",
        path: "/private/note.md",
        sizeBytes: 14
      })),
      writeStandaloneDocumentCas
    }));

    const selected = await port.pickers.pickStandaloneDocument();
    expect(selected).toEqual({ displayName: "note.md", handle: "standalone-1" });
    expect(selected).not.toHaveProperty("path");

    const initial = await port.standalone.read(selected!.handle);
    expect(initial).toMatchObject({
      contents: "initial",
      displayName: "note.md",
      revision: nativeRevision("a")
    });
    expect(initial).not.toHaveProperty("path");

    const saved = await port.standalone.write({
      contents: "second",
      expectedRevision: initial.revision,
      handle: selected!.handle
    });
    expect(saved).toMatchObject({
      contents: "second",
      displayName: "note.md",
      revision: nativeRevision("b")
    });
    expect(saved).not.toHaveProperty("path");
    expect(writeStandaloneDocumentCas).toHaveBeenCalledWith({
      contents: "second",
      expectedRevision: nativeRevision("a"),
      path: "/private/note.md"
    });
    expectTypeOf<
      Extract<keyof DesktopNativeShellDependencies, "readMarkdownFile" | "saveMarkdownFile">
    >().toEqualTypeOf<never>();
  });

  it("maps a native stale revision to the stable standalone conflict", async () => {
    const port = createDesktopNativeShellPort(dependencies({
      newHandle: () => "standalone-stale",
      openMarkdownFile: vi.fn(async () => ({
        content: "picker payload",
        name: "note.md",
        path: "/private/note.md",
        sizeBytes: 14
      })),
      writeStandaloneDocumentCas: vi.fn(async () =>
        Promise.reject("standalone-document-conflict"))
    }));
    const selected = await port.pickers.pickStandaloneDocument();

    await expect(port.standalone.write({
      contents: "local edit",
      expectedRevision: nativeRevision("a"),
      handle: selected!.handle
    })).rejects.toBeInstanceOf(NativeStandaloneConflictError);
  });

  it("serializes competing adapter writes so one stale revision cannot overwrite the winner", async () => {
    let currentRevision = nativeRevision("a");
    let contents = "initial";
    const writeStandaloneDocumentCas = vi.fn(async (input: {
      contents: string;
      expectedRevision: NativeStandaloneRevision;
    }) => {
      if (input.expectedRevision !== currentRevision) {
        return Promise.reject("standalone-document-conflict");
      }
      contents = input.contents;
      currentRevision = nativeRevision("b");
      return {
        contents,
        displayName: "note.md",
        revision: currentRevision
      };
    });
    const port = createDesktopNativeShellPort(dependencies({
      newHandle: () => "standalone-queue",
      openMarkdownFile: vi.fn(async () => ({
        content: "picker payload",
        name: "note.md",
        path: "/private/note.md",
        sizeBytes: 14
      })),
      writeStandaloneDocumentCas
    }));
    const selected = await port.pickers.pickStandaloneDocument();

    const outcomes = await Promise.allSettled([
      port.standalone.write({
        contents: "first winner",
        expectedRevision: nativeRevision("a"),
        handle: selected!.handle
      }),
      port.standalone.write({
        contents: "stale loser",
        expectedRevision: nativeRevision("a"),
        handle: selected!.handle
      })
    ]);

    expect(outcomes[0].status).toBe("fulfilled");
    expect(outcomes[1]).toMatchObject({
      reason: expect.any(NativeStandaloneConflictError),
      status: "rejected"
    });
    expect(writeStandaloneDocumentCas).toHaveBeenCalledTimes(2);
    expect(contents).toBe("first winner");
  });

  it("hides paths and contents from unexpected native write failures", async () => {
    const port = createDesktopNativeShellPort(dependencies({
      newHandle: () => "native-errors",
      openMarkdownFile: vi.fn(async () => ({
        content: "picker payload",
        name: "secret.md",
        path: "/private/secret.md",
        sizeBytes: 14
      })),
      writeStandaloneDocumentCas: vi.fn(async () =>
        Promise.reject("/private/secret.md: target-secret"))
    }));
    const selection = await port.pickers.pickStandaloneDocument();
    const unavailable = await port.standalone.write({
      contents: "local-secret",
      expectedRevision: nativeRevision("a"),
      handle: selection!.handle
    }).catch((error: unknown) => error);

    expect(unavailable).toBeInstanceOf(NativeStandaloneDocumentUnavailableError);
    expect(String(unavailable)).not.toContain("/private/secret.md");
    expect(String(unavailable)).not.toContain("target-secret");
    expect(String(unavailable)).not.toContain("local-secret");
  });

  it("classifies native paths without returning file paths to standalone callers", async () => {
    const port = createDesktopNativeShellPort(dependencies({
      newHandle: () => "classified-file",
      openMarkdownFolder: vi.fn(async () => ({ name: "Notes", path: "/data/Notes" })),
      resolveMarkdownFolder: vi.fn(async (path) => ({ name: "Notes", path })),
      resolveMarkdownPath: vi.fn(async (path) => ({
        kind: "file" as const,
        name: "note.md",
        path
      }))
    }));

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

  it("revokes a standalone handle so subsequent reads and writes fail closed", async () => {
    const port = createDesktopNativeShellPort(dependencies({
      newHandle: () => "revoked-file",
      openMarkdownFile: vi.fn(async () => ({
        content: "picker payload",
        name: "private.md",
        path: "/private/private.md",
        sizeBytes: 14
      }))
    }));
    const selected = await port.pickers.pickStandaloneDocument();
    const snapshot = await port.standalone.read(selected!.handle);

    await port.standalone.release(selected!.handle);

    await expect(port.standalone.read(selected!.handle))
      .rejects.toBeInstanceOf(NativeStandaloneDocumentUnavailableError);
    await expect(port.standalone.write({
      contents: "must not be written",
      expectedRevision: snapshot.revision,
      handle: selected!.handle
    })).rejects.toBeInstanceOf(NativeStandaloneDocumentUnavailableError);
  });

  it("deduplicates repeated selections of the same native file and forgets it on release", async () => {
    let nextHandle = 0;
    const port = createDesktopNativeShellPort(dependencies({
      newHandle: () => `deduplicated-${nextHandle += 1}`,
      openMarkdownFile: vi.fn(async () => ({
        content: "same file",
        name: "same.md",
        path: "/private/same.md",
        sizeBytes: 9
      }))
    }));

    const first = await port.pickers.pickStandaloneDocument();
    const repeated = await port.pickers.pickStandaloneDocument();
    expect(repeated!.handle).toBe(first!.handle);

    await port.standalone.release(first!.handle);
    const reopened = await port.pickers.pickStandaloneDocument();
    expect(reopened!.handle).not.toBe(first!.handle);
  });

  it("bounds live standalone handles instead of retaining paths without limit", async () => {
    let selected = 0;
    const port = createDesktopNativeShellPort(dependencies({
      newHandle: () => `bounded-${selected}`,
      openMarkdownFile: vi.fn(async () => {
        selected += 1;
        return {
          content: "bounded",
          name: `note-${selected}.md`,
          path: `/private/note-${selected}.md`,
          sizeBytes: 7
        };
      })
    }));

    for (let index = 0; index < 256; index += 1) {
      await expect(port.pickers.pickStandaloneDocument()).resolves.not.toBeNull();
    }
    await expect(port.pickers.pickStandaloneDocument())
      .rejects.toBeInstanceOf(NativeStandaloneDocumentUnavailableError);
  });

  it("reports exact native capability availability", () => {
    const port = createDesktopNativeShellPort(dependencies());

    expect(port.capabilities).toEqual({
      absolutePathClassification: "available",
      operatingSystemShell: "available",
      pickers: "available",
      standaloneDocuments: "available"
    });
    expect(typeof ("revision" as NativeStandaloneRevision)).toBe("string");
  });
});
