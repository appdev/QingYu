import { describe, expect, it } from "vitest";
import { resolveNewDocumentCreationDirectory } from "./workspace-model";

describe("resolveNewDocumentCreationDirectory", () => {
  it("prefers the focused saved note parent", () => {
    expect(resolveNewDocumentCreationDirectory({
      activeDocument: { path: "/vault/active.md" },
      focusedDocument: { path: "/vault/abc/focused.md" },
      workspaceSourcePath: "/vault"
    })).toBe("/vault/abc");
  });

  it("inherits a focused draft planned directory", () => {
    expect(resolveNewDocumentCreationDirectory({
      activeDocument: { path: "/vault/active.md" },
      focusedDocument: { creationDirectory: "/vault/abc", path: null },
      workspaceSourcePath: "/vault"
    })).toBe("/vault/abc");
  });

  it("uses the active note when no note has logical focus", () => {
    expect(resolveNewDocumentCreationDirectory({
      activeDocument: { path: "/vault/abc/active.md" },
      focusedDocument: null,
      workspaceSourcePath: "/vault"
    })).toBe("/vault/abc");
  });

  it("rejects an outside note and falls back to the workspace root", () => {
    expect(resolveNewDocumentCreationDirectory({
      activeDocument: { path: "/outside/note.md" },
      focusedDocument: null,
      workspaceSourcePath: "/vault"
    })).toBe("/vault");
  });

  it("does not treat a POSIX sibling prefix as within the workspace", () => {
    expect(resolveNewDocumentCreationDirectory({
      activeDocument: { path: "/vault-old/note.md" },
      focusedDocument: null,
      workspaceSourcePath: "/vault"
    })).toBe("/vault");
  });

  it("accepts a Windows path within the workspace regardless of separators and case", () => {
    expect(resolveNewDocumentCreationDirectory({
      activeDocument: { path: "c:/VAULT/abc/active.md" },
      focusedDocument: null,
      workspaceSourcePath: "C:\\vault"
    })).toBe("c:/VAULT/abc");
  });

  it("does not treat a kernel workspace sibling prefix as within the workspace", () => {
    expect(resolveNewDocumentCreationDirectory({
      activeDocument: { path: "kernel-workspace://primary-old/note.md" },
      focusedDocument: null,
      workspaceSourcePath: "kernel-workspace://primary"
    })).toBe("kernel-workspace://primary");
  });

  it.each([
    ["POSIX", "/vault/dir/../../outside", "/vault"],
    ["Windows", "C:\\vault\\..\\outside", "C:\\vault"],
    [
      "Kernel",
      "kernel-workspace://primary/notes/%2e%2e/outside",
      "kernel-workspace://primary"
    ]
  ])("rejects %s traversal in a planned directory", (_kind, creationDirectory, workspaceRoot) => {
    expect(resolveNewDocumentCreationDirectory({
      activeDocument: null,
      focusedDocument: { creationDirectory, path: null },
      workspaceSourcePath: workspaceRoot
    })).toBe(workspaceRoot);
  });
});
