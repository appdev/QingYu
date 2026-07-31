import {
  createUnavailableKernelDomainPort,
  type KernelDomainPort,
  type KernelRevision,
  type KernelWorkspaceGeneration,
} from "../kernel-domain";

import {
  createKernelFileRuntime,
  kernelWorkspaceRoot,
} from "./files";

const generation = "generation-1" as KernelWorkspaceGeneration;
const revision = "revision-1" as KernelRevision;

describe("Kernel AppRuntime adapter", () => {
  it("routes workspace writes to Kernel and ignores a full legacy file fallback", async () => {
    const legacySave = vi.fn(() => Promise.reject(new Error("legacy writer called")));
    const unavailable = createUnavailableKernelDomainPort();
    const update = vi.fn(async () => ({
      contents: "next",
      kind: "file" as const,
      locator: "document-1" as never,
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "note.md",
      parent: "" as never,
      relativePath: "note.md" as never,
      revision: "revision-2" as KernelRevision,
      sizeBytes: 4,
      workspaceGeneration: generation,
    }));
    const kernel: KernelDomainPort = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [{
            kind: "file" as const,
            locator: "document-1" as never,
            modifiedAt: "2026-07-31T00:00:00Z",
            name: "note.md",
            parent: "" as never,
            relativePath: "note.md" as never,
            revision,
            sizeBytes: 5,
            workspaceGeneration: generation,
          }],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
        update,
      },
      resources: undefined,
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-1",
          readiness: "ready" as const,
          revision,
        })),
      },
    };
    const files = createKernelFileRuntime(kernel, {
      nativeShell: { saveMarkdownFile: legacySave } as never,
    });

    await files.listMarkdownFilesForPath(kernelWorkspaceRoot);
    await expect(files.saveMarkdownFile({
      contents: "next",
      path: `${kernelWorkspaceRoot}/note.md`,
      suggestedName: "note.md",
    })).resolves.toEqual({
      name: "note.md",
      path: `${kernelWorkspaceRoot}/note.md`,
    });

    expect(update).toHaveBeenCalledOnce();
    expect(legacySave).not.toHaveBeenCalled();
  });

  it("fails closed for resource and template mutations that Kernel does not expose", async () => {
    const files = createKernelFileRuntime(createUnavailableKernelDomainPort());

    await expect(files.importLocalFile({} as never)).rejects.toThrow("unavailable");
    await expect(files.writeMarkdownTemplateFile("template", "contents"))
      .rejects.toThrow("unavailable");
  });

  it("uses a synthetic workspace root with no host absolute path", () => {
    expect(kernelWorkspaceRoot).toBe("kernel-workspace://primary");
    expect(kernelWorkspaceRoot).not.toMatch(/^\/(?:Users|Volumes|home|data)\//u);
    expect(kernelWorkspaceRoot).not.toMatch(/^[A-Za-z]:\\/u);
  });
});
