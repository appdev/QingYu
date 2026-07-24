import type {
  ThemeDescriptor,
  ThemeImportResult
} from "@markra/app/runtime";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { invokeNative } from "./invoke";
export {
  cancelNativeThemeActivation,
  commitNativeThemeActivation,
  confirmNativeThemeActivation,
  deleteNativeTheme,
  listNativeThemes,
  prepareNativeThemeActivation,
  releaseNativeThemeActivation
} from "./themes/shared";

export async function importNativeTheme() {
  const sourcePath = await open({
    directory: false,
    filters: [{ extensions: ["css", "theme"], name: "Theme" }],
    multiple: false
  });
  if (typeof sourcePath !== "string") return null;

  return invokeNative<ThemeImportResult>("import_theme_file", { sourcePath });
}

export function replaceNativeTheme(sourcePath: string, expectedFingerprint: string) {
  return invokeNative<ThemeDescriptor>("replace_theme_file", { sourcePath, expectedFingerprint });
}

export async function openNativeThemeDirectory() {
  const path = await invokeNative<string>("theme_directory_path");
  await openPath(path);
}
