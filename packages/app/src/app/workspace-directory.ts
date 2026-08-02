import { managedDirectoryPathIsWithinRoot } from "../lib/settings/workspace-state";
import { kernelWorkspaceRelativePathFromPath } from "../runtime/kernel-app/app-config";
import { kernelWorkspaceRoot } from "../runtime/kernel-app/files";

export function directoryPathIsWithinWorkspaceRoot(
  workspaceRoot: string | null | undefined,
  directoryPath: string | null | undefined
) {
  const directory = directoryPath?.trim();
  const root = workspaceRoot?.trim();
  if (!directory || !root) return false;

  if (root === kernelWorkspaceRoot) {
    try {
      kernelWorkspaceRelativePathFromPath(directory);
      return true;
    } catch {
      return false;
    }
  }

  return managedDirectoryPathIsWithinRoot(root, directory);
}
