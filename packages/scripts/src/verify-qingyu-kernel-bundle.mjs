import { execFileSync } from "node:child_process";
import {
  lstatSync,
  mkdtempSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

import { validatePreparedKernelSidecar } from "./prepare-qingyu-kernel-sidecar.mjs";

const supportedPlatforms = new Set(["linux", "macos", "windows"]);

export function verifyQingyuKernelBundle({
  bundleRoot,
  platform,
  productName = "QingYu",
  requireSignature = false,
  run = execFileSync,
  target,
  temporaryRoot = tmpdir(),
}) {
  if (!supportedPlatforms.has(platform)) {
    throw new Error(`Unsupported desktop bundle platform: ${platform || "<missing>"}`);
  }
  if (!bundleRoot || !target) {
    throw new Error("Kernel bundle verification requires bundleRoot and target.");
  }
  bundleRoot = resolve(bundleRoot);
  assertDirectory(bundleRoot, "Desktop bundle root");

  if (platform === "macos") {
    return verifyMacosBundle({
      bundleRoot,
      productName,
      requireSignature,
      run,
      target,
    });
  }
  if (platform === "linux") {
    return verifyLinuxBundle({ bundleRoot, run, target, temporaryRoot });
  }
  return verifyWindowsBundle({ bundleRoot, productName, run, target, temporaryRoot });
}

function verifyMacosBundle({
  bundleRoot,
  productName,
  requireSignature,
  run,
  target,
}) {
  const app = join(bundleRoot, "macos", `${productName}.app`);
  assertDirectory(app, "macOS app bundle");
  const kernel = join(app, "Contents", "MacOS", "qingyu-kernel");
  if (!pathExists(kernel)) {
    throw new Error(`${app} does not contain qingyu-kernel in Contents/MacOS.`);
  }
  const validation = validatePreparedKernelSidecar(kernel, target, "darwin");
  if (requireSignature) {
    run("codesign", ["--verify", "--strict", "--verbose=2", kernel], {
      encoding: "utf8",
      stdio: "pipe",
    });
  }
  return { ...validation, app, kernel, platform: "macos" };
}

function verifyLinuxBundle({ bundleRoot, run, target, temporaryRoot }) {
  const appImageDirectory = join(bundleRoot, "appimage");
  assertDirectory(appImageDirectory, "Linux AppImage bundle directory");
  const appImages = findFilesNamed(appImageDirectory, undefined).filter((path) =>
    basename(path).endsWith(".AppImage"),
  );
  if (appImages.length !== 1) {
    throw new Error(
      `${appImageDirectory} must contain exactly one final AppImage; found ${appImages.length}.`,
    );
  }

  const [appImage] = appImages;
  const extractionDirectory = mkdtempSync(join(temporaryRoot, "qingyu-kernel-appimage-"));
  try {
    run(appImage, ["--appimage-extract"], {
      cwd: extractionDirectory,
      encoding: "utf8",
      stdio: "pipe",
    });
    const extractedRoot = join(extractionDirectory, "squashfs-root");
    assertDirectory(extractedRoot, "Extracted final AppImage filesystem");
    const matches = findFilesNamed(extractedRoot, "qingyu-kernel");
    if (matches.length !== 1) {
      throw new Error(
        `${appImage} must contain exactly one qingyu-kernel; found ${matches.length}.`,
      );
    }
    const validation = validatePreparedKernelSidecar(matches[0], target, "linux");
    return {
      ...validation,
      appImage,
      kernelName: "qingyu-kernel",
      platform: "linux",
    };
  } finally {
    rmSync(extractionDirectory, { force: true, recursive: true });
  }
}

function verifyWindowsBundle({
  bundleRoot,
  productName,
  run,
  target,
  temporaryRoot,
}) {
  const nsisDirectory = join(bundleRoot, "nsis");
  assertDirectory(nsisDirectory, "Windows NSIS bundle directory");
  const installers = findFilesNamed(nsisDirectory, undefined).filter((path) => {
    const name = basename(path).toLocaleLowerCase("en-US");
    return name.endsWith(".exe") && name.includes(productName.toLocaleLowerCase("en-US"));
  });
  if (installers.length !== 1) {
    throw new Error(
      `${nsisDirectory} must contain exactly one ${productName} NSIS installer; found ${installers.length}.`,
    );
  }

  const [installer] = installers;
  const extractionDirectory = mkdtempSync(join(temporaryRoot, "qingyu-kernel-nsis-"));
  try {
    run("7z", ["x", "-y", `-o${extractionDirectory}`, installer], {
      encoding: "utf8",
      stdio: "pipe",
    });
    const matches = findFilesNamed(extractionDirectory, "qingyu-kernel.exe");
    if (matches.length !== 1) {
      throw new Error(
        `${installer} must contain exactly one qingyu-kernel.exe; found ${matches.length}.`,
      );
    }
    const validation = validatePreparedKernelSidecar(matches[0], target, "win32");
    return {
      ...validation,
      installer,
      kernelName: "qingyu-kernel.exe",
      platform: "windows",
    };
  } finally {
    rmSync(extractionDirectory, { force: true, recursive: true });
  }
}

function findFilesNamed(root, expectedName) {
  const matches = [];
  const pending = [root];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const path = join(current, entry.name);
      if (entry.isDirectory()) {
        pending.push(path);
      } else if (expectedName === undefined || entry.name === expectedName) {
        matches.push(path);
      }
    }
  }
  return matches.sort();
}

function assertDirectory(path, label) {
  let metadata;
  try {
    metadata = lstatSync(path);
  } catch (error) {
    if (error?.code === "ENOENT") {
      throw new Error(`${label} does not exist: ${path}`);
    }
    throw error;
  }
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
    throw new Error(`${label} must be a real directory: ${path}`);
  }
}

function pathExists(path) {
  try {
    lstatSync(path);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function parseArguments(arguments_) {
  const options = {};
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === "--require-signature") {
      options.requireSignature = true;
      continue;
    }
    const key = {
      "--bundle-root": "bundleRoot",
      "--platform": "platform",
      "--product-name": "productName",
      "--target": "target",
    }[argument];
    if (!key || !arguments_[index + 1]) {
      throw new Error(`Unknown or incomplete Kernel bundle verification option: ${argument}`);
    }
    options[key] = arguments_[index + 1];
    index += 1;
  }
  return options;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const result = verifyQingyuKernelBundle(parseArguments(process.argv.slice(2)));
  process.stdout.write(
    `Verified bundled qingyu-kernel for ${result.platform}/${result.target} (${result.byteLength} bytes).\n`,
  );
}
