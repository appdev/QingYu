import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { describe, expect, it, vi } from "vitest";
import { builtInThemeAssetsPlugin } from "./built-in-theme-assets";

const licenseFileNames = ["FONT-LICENSE.txt", "FONT-SOURCE.txt", "THEME-LICENSE.txt"] as const;

describe("builtInThemeAssetsPlugin", () => {
  it("emits the three built-in theme notices under stable output names", async () => {
    const root = await mkdtemp(join(tmpdir(), "markra-theme-licenses-"));
    const licenseDirectory = join(root, "licenses");
    await mkdir(licenseDirectory);
    for (const fileName of licenseFileNames) {
      await writeFile(join(licenseDirectory, fileName), `contents:${fileName}\n`);
    }

    const plugin = builtInThemeAssetsPlugin({
      licenseDirectoryUrl: pathToFileURL(`${licenseDirectory}/`)
    });
    const emitFile = vi.fn();
    const buildStart = plugin.buildStart;
    expect(typeof buildStart).toBe("function");
    if (typeof buildStart !== "function") return;

    await buildStart.call({ emitFile } as never, {} as never);

    expect(emitFile.mock.calls.map(([asset]) => asset)).toEqual(
      licenseFileNames.map((fileName) => ({
        type: "asset",
        fileName: `assets/licenses/${fileName}`,
        source: `contents:${fileName}\n`
      }))
    );
  });

  it("fails the build when a required notice is missing", async () => {
    const root = await mkdtemp(join(tmpdir(), "markra-theme-licenses-missing-"));
    const plugin = builtInThemeAssetsPlugin({
      licenseDirectoryUrl: pathToFileURL(`${root}/`)
    });
    const buildStart = plugin.buildStart;
    expect(typeof buildStart).toBe("function");
    if (typeof buildStart !== "function") return;

    await expect(
      buildStart.call({ emitFile: vi.fn() } as never, {} as never)
    ).rejects.toThrow("FONT-LICENSE.txt");
  });
});
