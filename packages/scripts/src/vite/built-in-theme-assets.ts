import { readFile } from "node:fs/promises";
import type { Plugin } from "vite";

export const builtInThemeLicenseFileNames = [
  "FONT-LICENSE.txt",
  "FONT-SOURCE.txt",
  "THEME-LICENSE.txt"
] as const;

export type BuiltInThemeAssetsPluginOptions = {
  licenseDirectoryUrl: string | URL;
};

export function builtInThemeAssetsPlugin(
  options: BuiltInThemeAssetsPluginOptions
): Plugin {
  return {
    apply: "build",
    name: "markra-built-in-theme-assets",
    async buildStart() {
      for (const fileName of builtInThemeLicenseFileNames) {
        const source = await readFile(new URL(fileName, options.licenseDirectoryUrl), "utf8");
        this.emitFile({
          type: "asset",
          fileName: `assets/licenses/${fileName}`,
          source
        });
      }
    }
  };
}
