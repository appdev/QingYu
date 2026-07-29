import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  builtInThemeAssetInventory,
  verifyBuiltInThemeAssets
} from "./verify-built-in-theme-assets.mjs";

const licenseFileNames = ["FONT-LICENSE.txt", "FONT-SOURCE.txt", "THEME-LICENSE.txt"];

async function createValidOutput() {
  const outputDirectory = await mkdtemp(join(tmpdir(), "markra-built-in-theme-output-"));
  const fontDirectory = join(outputDirectory, "assets/fonts");
  const licenseDirectory = join(outputDirectory, "assets/licenses");
  await mkdir(fontDirectory, { recursive: true });
  await mkdir(licenseDirectory, { recursive: true });

  const fontContents = [];
  for (let index = 1; index <= 9; index += 1) {
    const contents = Buffer.from(`wOF2font-subset-${index}`);
    fontContents.push(contents);
    await writeFile(
      join(fontDirectory, `zhenkai-gb-regular-subset-${index}-hash${index}.woff2`),
      contents
    );
  }
  for (const fileName of licenseFileNames) {
    await writeFile(join(licenseDirectory, fileName), `license:${fileName}`);
  }
  return {
    expectedCombinedSha256: createHash("sha256").update(Buffer.concat(fontContents)).digest("hex"),
    outputDirectory
  };
}

describe("verifyBuiltInThemeAssets", () => {
  it("accepts exactly nine ZhenKai subsets and the three stable notices", async () => {
    const { expectedCombinedSha256, outputDirectory } = await createValidOutput();

    await expect(verifyBuiltInThemeAssets(outputDirectory, { expectedCombinedSha256 })).resolves.toEqual({
      fontCount: 9,
      licenseCount: 3
    });
  });

  it("rejects eight ZhenKai subsets", async () => {
    const { expectedCombinedSha256, outputDirectory } = await createValidOutput();
    await unlink(
      join(outputDirectory, "assets/fonts/zhenkai-gb-regular-subset-9-hash9.woff2")
    );

    await expect(verifyBuiltInThemeAssets(outputDirectory, { expectedCombinedSha256 })).rejects.toThrow(
      "expected 9 ZhenKai font subsets (1-9), found 8"
    );
  });

  it("rejects ten ZhenKai subsets", async () => {
    const { expectedCombinedSha256, outputDirectory } = await createValidOutput();
    await writeFile(
      join(outputDirectory, "assets/fonts/zhenkai-gb-regular-subset-10-extra.woff2"),
      "wOF2font-subset-10"
    );

    await expect(verifyBuiltInThemeAssets(outputDirectory, { expectedCombinedSha256 })).rejects.toThrow(
      "expected 9 ZhenKai font subsets (1-9), found 10"
    );
    await expect(builtInThemeAssetInventory(outputDirectory)).resolves.toMatchObject({
      licenseFileNames
    });
  });

  it("rejects a missing notice", async () => {
    const { expectedCombinedSha256, outputDirectory } = await createValidOutput();
    await unlink(join(outputDirectory, "assets/licenses/THEME-LICENSE.txt"));

    await expect(verifyBuiltInThemeAssets(outputDirectory, { expectedCombinedSha256 })).rejects.toThrow(
      "expected licenses FONT-LICENSE.txt, FONT-SOURCE.txt, THEME-LICENSE.txt"
    );
  });

  it("rejects the same built-in font bytes published under a second name", async () => {
    const { expectedCombinedSha256, outputDirectory } = await createValidOutput();
    await writeFile(
      join(outputDirectory, "assets/fonts/paper-font-copy.woff2"),
      "wOF2font-subset-4"
    );

    await expect(verifyBuiltInThemeAssets(outputDirectory, { expectedCombinedSha256 })).rejects.toThrow(
      "duplicates ZhenKai subset content"
    );
  });

  it("rejects a subset without the WOFF2 signature", async () => {
    const { expectedCombinedSha256, outputDirectory } = await createValidOutput();
    await writeFile(
      join(outputDirectory, "assets/fonts/zhenkai-gb-regular-subset-5-hash5.woff2"),
      "not-a-woff2-subset"
    );

    await expect(verifyBuiltInThemeAssets(outputDirectory, { expectedCombinedSha256 })).rejects.toThrow(
      "does not start with the WOFF2 signature"
    );
  });

  it("rejects duplicate content between ZhenKai subsets", async () => {
    const { expectedCombinedSha256, outputDirectory } = await createValidOutput();
    const firstSubset = await readFile(
      join(outputDirectory, "assets/fonts/zhenkai-gb-regular-subset-1-hash1.woff2")
    );
    await writeFile(
      join(outputDirectory, "assets/fonts/zhenkai-gb-regular-subset-2-hash2.woff2"),
      firstSubset
    );

    await expect(verifyBuiltInThemeAssets(outputDirectory, { expectedCombinedSha256 })).rejects.toThrow(
      "duplicates ZhenKai subset content"
    );
  });

  it("rejects a changed ordered ZhenKai bundle", async () => {
    const { expectedCombinedSha256, outputDirectory } = await createValidOutput();
    await writeFile(
      join(outputDirectory, "assets/fonts/zhenkai-gb-regular-subset-9-hash9.woff2"),
      "wOF2changed-font-subset-9"
    );

    await expect(verifyBuiltInThemeAssets(outputDirectory, { expectedCombinedSha256 })).rejects.toThrow(
      "ordered ZhenKai bundle SHA-256"
    );
  });
});
