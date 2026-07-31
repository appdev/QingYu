import { open } from "@tauri-apps/plugin-dialog";

type DesktopWorkspaceDirectorySelection = string | string[] | null;

export type DesktopWorkspaceDirectoryOpener = (
  options: { readonly directory: true; readonly multiple: false },
) => Promise<DesktopWorkspaceDirectorySelection>;

const openNativeDesktopWorkspaceDirectory: DesktopWorkspaceDirectoryOpener =
  async (options) => open(options);

export async function selectDesktopWorkspaceDirectory(
  openDirectory: DesktopWorkspaceDirectoryOpener = openNativeDesktopWorkspaceDirectory,
): Promise<string | null> {
  const selectedPath = await openDirectory({
    directory: true,
    multiple: false,
  });
  return typeof selectedPath === "string" ? selectedPath : null;
}
