import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const legacyDisplayName = ["Mar", "kra"].join("");
const defaultDisplayName = "QingYu";
const chineseLocalePatterns = [
  /packages\/shared\/src\/i18n\/locales\/zh-(?:CN|TW)\.ts$/,
  /apps\/desktop\/src-tauri\/src\/menu_labels\/zh_(?:cn|tw)\.rs$/,
  /apps\/desktop\/src-tauri\/macos-locales\/zh-(?:Hans|Hant)\.lproj\/InfoPlist\.strings$/
];

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function scanStandaloneName(text, path, displayName, kind) {
  const violations = [];
  const lines = text.split(/\r?\n/);

  lines.forEach((lineText, lineIndex) => {
    const pattern = new RegExp(`\\b${escapeRegExp(displayName)}\\b`, "g");
    for (const match of lineText.matchAll(pattern)) {
      violations.push({
        column: match.index + 1,
        context: lineText.trim(),
        kind,
        line: lineIndex + 1,
        path
      });
    }
  });

  return violations;
}

function isChineseLocalePath(path) {
  const normalizedPath = path.replaceAll("\\", "/");
  return chineseLocalePatterns.some((pattern) => pattern.test(normalizedPath));
}

export function scanTextForLegacyBrandReferences(text, path) {
  return scanStandaloneName(text, path, legacyDisplayName, "legacy-display-name");
}

export function scanTextForBrandCopyViolations(text, path) {
  const violations = scanTextForLegacyBrandReferences(text, path);
  if (isChineseLocalePath(path)) {
    violations.push(...scanStandaloneName(
      text,
      path,
      defaultDisplayName,
      "chinese-locale-display-name"
    ));
  }
  return violations;
}

function trackedFiles(repoRoot) {
  const result = spawnSync("git", ["ls-files", "-z"], {
    cwd: repoRoot,
    encoding: null
  });
  if (result.status !== 0) {
    const message = result.stderr?.toString("utf8").trim() || "git ls-files failed";
    throw new Error(message);
  }
  return result.stdout
    .toString("utf8")
    .split("\0")
    .filter(Boolean);
}

export async function findBrandCopyViolations(repoRoot) {
  const violations = [];
  for (const path of trackedFiles(repoRoot)) {
    const buffer = await readFile(resolve(repoRoot, path));
    if (buffer.includes(0)) continue;
    violations.push(...scanTextForBrandCopyViolations(buffer.toString("utf8"), path));
  }
  return violations;
}

async function run() {
  const repoRoot = resolve(process.argv[2] || process.cwd());
  const violations = await findBrandCopyViolations(repoRoot);
  if (violations.length === 0) {
    console.log("Brand copy verification passed.");
    return;
  }

  for (const violation of violations) {
    console.error(
      `${violation.path}:${violation.line}:${violation.column} [${violation.kind}] ${violation.context}`
    );
  }
  console.error(`Brand copy verification found ${violations.length} violation(s).`);
  process.exitCode = 1;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  run().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
