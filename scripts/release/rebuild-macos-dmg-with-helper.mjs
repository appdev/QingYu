import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

function requireEnv(name) {
  const value = process.env[name]?.trim();

  if (!value) {
    throw new Error(`${name} is required.`);
  }

  return value;
}

function findSingleFile(dir, predicate, description) {
  const matches = fs
    .readdirSync(dir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && predicate(entry.name))
    .map((entry) => path.join(dir, entry.name))
    .sort();

  if (matches.length !== 1) {
    throw new Error(`Expected exactly one ${description} in ${dir}, found ${matches.length}.`);
  }

  return matches[0];
}

function removeIfExists(filePath) {
  if (fs.existsSync(filePath)) {
    fs.rmSync(filePath, { force: true });
  }
}

const dmgDir = requireEnv("DMG_DIR");
const macosBundleDir = requireEnv("MACOS_BUNDLE_DIR");
const helperPath = requireEnv("HELPER_PATH");
const productName = process.env.APP_PRODUCT_NAME?.trim() || "QingYu";
const appName = `${productName}.app`;
const appPath = path.join(macosBundleDir, appName);
const helperName = path.basename(helperPath);
const dmgPath = findSingleFile(dmgDir, (name) => name.endsWith(".dmg") && !name.endsWith(".with-helper.dmg"), "DMG file");
const bundleDmgScript = path.join(dmgDir, "bundle_dmg.sh");
const outputDmgPath = path.join(dmgDir, `${path.basename(dmgPath, ".dmg")}.with-helper.dmg`);
const stageRoot = fs.mkdtempSync(path.join(os.tmpdir(), "markra-dmg-stage-"));
const stageSourceDir = path.join(stageRoot, "source");
const stagedAppPath = path.join(stageSourceDir, appName);

if (!fs.existsSync(bundleDmgScript)) {
  throw new Error(`bundle_dmg.sh not found: ${bundleDmgScript}`);
}

if (!fs.existsSync(appPath)) {
  throw new Error(`macOS app bundle not found: ${appPath}`);
}

if (!fs.existsSync(helperPath)) {
  throw new Error(`macOS open helper not found: ${helperPath}`);
}

fs.mkdirSync(stageSourceDir, { recursive: true });
fs.cpSync(appPath, stagedAppPath, { recursive: true, verbatimSymlinks: true });
removeIfExists(outputDmgPath);

const args = [
  bundleDmgScript,
  "--volname",
  productName,
  "--window-size",
  "660",
  "400",
  "--icon",
  appName,
  "180",
  "170",
  "--app-drop-link",
  "480",
  "170",
  "--add-file",
  helperName,
  helperPath,
  "330",
  "310",
  "--no-internet-enable",
  outputDmgPath,
  stageSourceDir,
];

try {
  if (process.env.DRY_RUN === "1") {
    const output = {
      command: ["bash", ...args],
      dmgPath,
      outputDmgPath,
      stagedAppExists: fs.existsSync(stagedAppPath),
      stagedAppPath,
    };
    const dryRunOutputPath = process.env.DRY_RUN_OUTPUT_PATH;

    if (dryRunOutputPath) {
      fs.writeFileSync(dryRunOutputPath, `${JSON.stringify(output, null, 2)}\n`);
    } else {
      process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
    }
  } else {
    const result = spawnSync("bash", args, { stdio: "inherit" });

    if (result.status !== 0) {
      throw new Error(`bundle_dmg.sh failed with exit code ${result.status ?? "unknown"}.`);
    }

    fs.renameSync(outputDmgPath, dmgPath);
  }
} finally {
  fs.rmSync(stageRoot, { force: true, recursive: true });
}
