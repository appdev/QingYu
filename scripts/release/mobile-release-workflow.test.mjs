import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const workflow = fs.readFileSync(path.join(repoRoot, ".github", "workflows", "release.yml"), "utf8");

function readJson(relativePath) {
  return JSON.parse(fs.readFileSync(path.join(repoRoot, relativePath), "utf8"));
}

function readJob(name) {
  const start = workflow.indexOf(`  ${name}:`);
  assert.notEqual(start, -1, `${name} job should exist`);

  const next = workflow.slice(start + 1).search(/^  [a-z][a-z0-9_]*:/m);
  return next === -1 ? workflow.slice(start) : workflow.slice(start, start + 1 + next);
}

test("release validation builds and tags the workflow dispatch commit", () => {
  const job = readJob("validate_release");

  assert.match(job, /fetch-depth: 0/);
  assert.match(job, /ref: \$\{\{ github\.sha \}\}/);
  assert.match(job, /apps\/desktop\/package\.json/);
  assert.match(job, /apps\/web\/package\.json/);
  assert.match(job, /apps\/site\/package\.json/);
  assert.match(job, /src-tauri\/tauri\.conf\.json/);
  assert.match(job, /src-tauri\/Cargo\.toml/);
  assert.match(job, /refs\/tags\/\$\{RELEASE_TAG\}/);
  assert.match(job, /GITHUB_SHA/);
});

test("release workflow builds and uploads an unsigned Android ARM64 APK", () => {
  const job = readJob("build_android");

  assert.match(job, /needs: validate_release/);
  assert.match(job, /runs-on: ubuntu-22\.04/);
  assert.match(job, /uses: actions\/setup-java@v4/);
  assert.match(job, /targets: aarch64-linux-android/);
  assert.match(job, /pnpm app build android --apk --target aarch64 --ci/);
  assert.match(job, /QingYu_\$\{version\}_android_arm64_unsigned\.apk/);
  assert.match(job, /name: \$\{\{ env\.APP_SLUG \}\}-Android-arm64-bundles/);
});

test("release workflow uses the proven NSIS-only Windows bundle", () => {
  const job = readJob("build");

  assert.match(
    job,
    /name: Windows \(x64\)[\s\S]*?args: --target x86_64-pc-windows-msvc --bundles nsis/,
  );
});

test("release workflow installs xdg-open for Linux AppImage bundling", () => {
  const job = readJob("build");

  assert.match(job, /sudo apt-get install -y[\s\S]*?xdg-utils/);
});

test("release workflow builds and uploads an unsigned iOS Simulator app", () => {
  const job = readJob("build_ios");

  assert.match(job, /needs: validate_release/);
  assert.match(job, /runs-on: macos-15/);
  assert.match(job, /targets: aarch64-apple-ios-sim/);
  assert.match(job, /pnpm app build ios --target aarch64-sim --no-sign --ci/);
  assert.match(job, /ditto -c -k --sequesterRsrc --keepParent/);
  assert.match(job, /QingYu_\$\{version\}_ios_simulator_arm64_unsigned\.app\.zip/);
  assert.match(job, /name: \$\{\{ env\.APP_SLUG \}\}-iOS-simulator-arm64-bundles/);
});

test("release publication waits for every platform and uploads deterministic notes", () => {
  const job = readJob("publish_release");

  assert.match(job, /needs: \[validate_release, build, build_android, build_ios\]/);
  assert.match(job, /node scripts\/release\/generate-release-notes\.mjs/);
  assert.match(job, /RELEASE_TARGET: \$\{\{ github\.sha \}\}/);
  assert.match(job, /GITHUB_TOKEN: \$\{\{ github\.token \}\}/);
  assert.match(job, /pattern: \$\{\{ env\.APP_SLUG \}\}-\*-bundles/);
  assert.match(job, /target_commitish: \$\{\{ github\.sha \}\}/);
  assert.doesNotMatch(job, /pnpm dlx changelogen@latest/);
});

test("desktop artifact collection excludes package internals", () => {
  const job = readJob("build");

  assert.match(job, /const releaseFilePrefix = `\$\{process\.env\.APP_PRODUCT_NAME\}_\$\{releaseVersion\}_`/);
  assert.match(job, /fileName\.startsWith\(releaseFilePrefix\)/);
});

test("release workflow keeps signed desktop releases optional", () => {
  const validationJob = readJob("validate_release");
  const job = readJob("build");

  assert.match(validationJob, /signed_release: \$\{\{ steps\.release_capabilities\.outputs\.signed_release \}\}/);
  assert.match(job, /ENABLE_SIGNED_RELEASE: \$\{\{ needs\.validate_release\.outputs\.signed_release \}\}/);
  assert.match(job, /if: needs\.validate_release\.outputs\.signed_release == 'true'/);
  assert.match(job, /TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(job, /tauri\.updater\.conf\.json/);
});

test("release version sources are ready for v1.7.9", () => {
  const expectedVersion = "1.7.9";
  const cargoToml = fs.readFileSync(path.join(repoRoot, "apps/desktop/src-tauri/Cargo.toml"), "utf8");
  const cargoLock = fs.readFileSync(path.join(repoRoot, "apps/desktop/src-tauri/Cargo.lock"), "utf8");
  const versions = [
    readJson("package.json").version,
    readJson("apps/desktop/package.json").version,
    readJson("apps/web/package.json").version,
    readJson("apps/site/package.json").version,
    readJson("apps/desktop/src-tauri/tauri.conf.json").version,
    cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1],
    cargoLock.match(/\[\[package\]\]\nname = "markra"\nversion = "([^"]+)"/)?.[1],
  ];

  assert.deepEqual(versions, Array(versions.length).fill(expectedVersion));
});
