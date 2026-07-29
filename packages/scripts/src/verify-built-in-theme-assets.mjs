import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import { join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const expectedLicenseFileNames = [
  "FONT-LICENSE.txt",
  "FONT-SOURCE.txt",
  "THEME-LICENSE.txt"
];
const zhenKaiFontPattern = /^zhenkai-gb-regular-subset-([0-9]+)(?:-[A-Za-z0-9_-]+)?\.woff2$/u;
const zhenKaiOrderedBundleSha256 = "1d2c35a72a03564ee61c771ec5eb5756beef3a6f00464a13e1755060e280e3e5";
const woff2Signature = Buffer.from("wOF2", "ascii");

async function directoryFileNames(directory) {
  try {
    return (await readdir(directory, { withFileTypes: true }))
      .filter((entry) => entry.isFile())
      .map((entry) => entry.name)
      .sort();
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }
}

async function recursiveFilePaths(directory) {
  let entries;
  try {
    entries = await readdir(directory, { withFileTypes: true });
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }

  const files = [];
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await recursiveFilePaths(path));
    if (entry.isFile()) files.push(path);
  }
  return files.sort();
}

function portableRelativePath(root, path) {
  return relative(root, path).split(sep).join("/");
}

export async function builtInThemeAssetInventory(outputDirectory) {
  const outputRoot = resolve(outputDirectory);
  const fontDirectory = join(outputRoot, "assets/fonts");
  const licenseDirectory = join(outputRoot, "assets/licenses");
  const fontFileNames = await directoryFileNames(fontDirectory);
  const zhenKaiFontFileNames = fontFileNames.filter((fileName) => zhenKaiFontPattern.test(fileName));
  const allWoff2Paths = (await recursiveFilePaths(outputRoot))
    .filter((path) => path.toLowerCase().endsWith(".woff2"));
  const zhenKaiPaths = new Set(
    zhenKaiFontFileNames.map((fileName) => join(fontDirectory, fileName))
  );

  return {
    outputRoot,
    licenseFileNames: await directoryFileNames(licenseDirectory),
    zhenKaiFontFileNames,
    otherWoff2Paths: allWoff2Paths.filter((path) => !zhenKaiPaths.has(path))
  };
}

async function contentDigest(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

export async function verifyBuiltInThemeAssets(
  outputDirectory,
  { expectedCombinedSha256 = zhenKaiOrderedBundleSha256 } = {}
) {
  const inventory = await builtInThemeAssetInventory(outputDirectory);
  const subsetIndexes = inventory.zhenKaiFontFileNames
    .map((fileName) => Number(zhenKaiFontPattern.exec(fileName)?.[1]))
    .sort((left, right) => left - right);

  if (
    inventory.zhenKaiFontFileNames.length !== 9
    || subsetIndexes.join(",") !== "1,2,3,4,5,6,7,8,9"
  ) {
    throw new Error(
      `[verify-built-in-theme-assets] expected 9 ZhenKai font subsets (1-9), found ${inventory.zhenKaiFontFileNames.length}`
    );
  }

  if (inventory.licenseFileNames.join(",") !== expectedLicenseFileNames.join(",")) {
    throw new Error(
      `[verify-built-in-theme-assets] expected licenses ${expectedLicenseFileNames.join(", ")}; found ${inventory.licenseFileNames.join(", ") || "none"}`
    );
  }

  const orderedZhenKaiFileNames = [...inventory.zhenKaiFontFileNames].sort((left, right) => (
    Number(zhenKaiFontPattern.exec(left)?.[1]) - Number(zhenKaiFontPattern.exec(right)?.[1])
  ));
  const zhenKaiDigests = new Map();
  const orderedZhenKaiContents = [];
  for (const fileName of orderedZhenKaiFileNames) {
    const path = join(inventory.outputRoot, "assets/fonts", fileName);
    const contents = await readFile(path);
    if (!contents.subarray(0, woff2Signature.length).equals(woff2Signature)) {
      throw new Error(
        `[verify-built-in-theme-assets] ${fileName} does not start with the WOFF2 signature`
      );
    }
    const digest = createHash("sha256").update(contents).digest("hex");
    const duplicateOf = zhenKaiDigests.get(digest);
    if (duplicateOf) {
      throw new Error(
        `[verify-built-in-theme-assets] ${fileName} duplicates ZhenKai subset content from ${duplicateOf}`
      );
    }
    zhenKaiDigests.set(digest, fileName);
    orderedZhenKaiContents.push(contents);
  }

  const combinedDigest = createHash("sha256")
    .update(Buffer.concat(orderedZhenKaiContents))
    .digest("hex");
  if (combinedDigest !== expectedCombinedSha256) {
    throw new Error(
      `[verify-built-in-theme-assets] ordered ZhenKai bundle SHA-256 ${combinedDigest} does not match ${expectedCombinedSha256}`
    );
  }
  for (const path of inventory.otherWoff2Paths) {
    const duplicateOf = zhenKaiDigests.get(await contentDigest(path));
    if (!duplicateOf) continue;
    throw new Error(
      `[verify-built-in-theme-assets] ${portableRelativePath(inventory.outputRoot, path)} duplicates ZhenKai subset content from ${duplicateOf}`
    );
  }

  return { fontCount: 9, licenseCount: 3 };
}

async function run() {
  const outputDirectory = resolve(process.cwd(), process.argv[2] ?? "dist");
  const result = await verifyBuiltInThemeAssets(outputDirectory);
  console.log(
    `[verify-built-in-theme-assets] verified ${result.fontCount} font subsets and ${result.licenseCount} notices`
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  run().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
