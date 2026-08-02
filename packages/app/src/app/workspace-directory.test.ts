import { describe, expect, it } from "vitest";
import { directoryPathIsWithinWorkspaceRoot } from "./workspace-directory";

describe("directoryPathIsWithinWorkspaceRoot", () => {
  it.each([
    ["POSIX root equality", "/vault", "/vault"],
    ["POSIX child", "/vault", "/vault/notes"],
    ["Windows drive child", "C:\\Vault", "c:/vault/notes"],
    ["Windows UNC child", "\\\\Server\\Share\\Vault", "//server/share/vault/notes"]
  ])("accepts %s", (_case, root, directory) => {
    expect(directoryPathIsWithinWorkspaceRoot(root, directory)).toBe(true);
  });

  it.each([
    ["POSIX traversal", "/vault", "/vault/notes/../../outside"],
    ["Windows drive traversal", "C:\\vault", "C:\\vault\\..\\outside"],
    [
      "Windows UNC traversal",
      "\\\\server\\share\\vault",
      "\\\\server\\share\\vault\\notes\\..\\..\\outside"
    ]
  ])("rejects %s", (_case, root, directory) => {
    expect(directoryPathIsWithinWorkspaceRoot(root, directory)).toBe(false);
  });

  it("accepts the Kernel workspace root and a valid encoded child", () => {
    const root = "kernel-workspace://primary";

    expect(directoryPathIsWithinWorkspaceRoot(root, root)).toBe(true);
    expect(directoryPathIsWithinWorkspaceRoot(root, `${root}/My%20Notes`)).toBe(true);
  });

  it.each([
    "kernel-workspace://primary/notes/../outside",
    "kernel-workspace://primary/notes/%2e%2e/outside",
    "kernel-workspace://primary/notes%2Foutside",
    "kernel-workspace://primary-old/notes"
  ])("rejects invalid Kernel directory %s", (directory) => {
    expect(directoryPathIsWithinWorkspaceRoot("kernel-workspace://primary", directory)).toBe(false);
  });
});
