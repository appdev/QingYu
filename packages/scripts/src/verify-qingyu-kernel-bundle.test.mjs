import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { chmod, mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";

import { verifyQingyuKernelBundle } from "./verify-qingyu-kernel-bundle.mjs";

describe("verifyQingyuKernelBundle", () => {
  it("validates the Kernel inside the macOS app and requires its signature", async () => {
    const root = await temporaryBundleRoot();
    const kernel = join(root, "macos/QingYu.app/Contents/MacOS/qingyu-kernel");
    await writeExecutable(kernel, machHeader(0x0100000c));
    const run = vi.fn();

    const result = verifyQingyuKernelBundle({
      bundleRoot: root,
      platform: "macos",
      productName: "QingYu",
      requireSignature: true,
      run,
      target: "aarch64-apple-darwin",
    });

    expect(result).toMatchObject({ kernel, platform: "macos" });
    expect(run).toHaveBeenCalledTimes(1);
    expect(run).toHaveBeenCalledWith(
      "codesign",
      ["--verify", "--strict", "--verbose=2", kernel],
      expect.objectContaining({ stdio: "pipe" }),
    );
  });

  it("does not claim a signed Kernel for an unsigned macOS release", async () => {
    const root = await temporaryBundleRoot();
    const kernel = join(root, "macos/QingYu.app/Contents/MacOS/qingyu-kernel");
    await writeExecutable(kernel, machHeader(0x01000007));
    const run = vi.fn();

    verifyQingyuKernelBundle({
      bundleRoot: root,
      platform: "macos",
      productName: "QingYu",
      requireSignature: false,
      run,
      target: "x86_64-apple-darwin",
    });

    expect(run).not.toHaveBeenCalled();
  });

  it("fails closed when the macOS app has no Kernel", async () => {
    const root = await temporaryBundleRoot();
    await mkdir(join(root, "macos/QingYu.app/Contents/MacOS"), { recursive: true });

    expect(() =>
      verifyQingyuKernelBundle({
        bundleRoot: root,
        platform: "macos",
        productName: "QingYu",
        target: "aarch64-apple-darwin",
      }),
    ).toThrow("does not contain qingyu-kernel");
  });

  it("extracts the final Linux AppImage and validates its unique Kernel", async () => {
    const root = await temporaryBundleRoot();
    const appImage = join(root, "appimage/QingYu_2.2.0_arm64.AppImage");
    await writeExecutable(appImage, "APPIMAGE");
    const run = vi.fn((_command, _args, options) => {
      writeExecutableSync(
        join(options.cwd, "squashfs-root/usr/bin/qingyu-kernel"),
        elfHeader(183),
      );
    });

    expect(
      verifyQingyuKernelBundle({
        bundleRoot: root,
        platform: "linux",
        productName: "QingYu",
        run,
        target: "aarch64-unknown-linux-gnu",
      }),
    ).toMatchObject({ appImage, platform: "linux" });
    expect(run).toHaveBeenCalledWith(
      appImage,
      ["--appimage-extract"],
      expect.objectContaining({
        cwd: expect.stringContaining("qingyu-kernel-appimage-"),
        stdio: "pipe",
      }),
    );
  });

  it("fails closed when the final Linux AppImage contains ambiguous Kernel files", async () => {
    const root = await temporaryBundleRoot();
    const appImage = join(root, "appimage/QingYu_2.2.0_x64.AppImage");
    await writeExecutable(appImage, "APPIMAGE");
    const run = vi.fn((_command, _args, options) => {
      writeExecutableSync(
        join(options.cwd, "squashfs-root/usr/bin/qingyu-kernel"),
        elfHeader(62),
      );
      writeExecutableSync(
        join(options.cwd, "squashfs-root/extra/qingyu-kernel"),
        elfHeader(62),
      );
    });

    expect(() =>
      verifyQingyuKernelBundle({
        bundleRoot: root,
        platform: "linux",
        productName: "QingYu",
        run,
        target: "x86_64-unknown-linux-gnu",
      }),
    ).toThrow("exactly one qingyu-kernel");
  });

  it("does not accept a Kernel that exists only in the intermediate AppDir", async () => {
    const root = await temporaryBundleRoot();
    await writeExecutable(
      join(root, "appimage/QingYu.AppDir/usr/bin/qingyu-kernel"),
      elfHeader(62),
    );
    await writeExecutable(
      join(root, "appimage/QingYu_2.2.0_x64.AppImage"),
      "APPIMAGE",
    );

    expect(() =>
      verifyQingyuKernelBundle({
        bundleRoot: root,
        platform: "linux",
        productName: "QingYu",
        run: vi.fn(),
        target: "x86_64-unknown-linux-gnu",
      }),
    ).toThrow("final AppImage");
  });

  it("executes a final AppImage by absolute path when the bundle root is relative", async () => {
    const root = await temporaryBundleRoot();
    const appImage = join(root, "appimage/QingYu_2.2.0_x64.AppImage");
    await writeExecutable(appImage, "APPIMAGE");
    const run = vi.fn((_command, _args, options) => {
      writeExecutableSync(
        join(options.cwd, "squashfs-root/usr/bin/qingyu-kernel"),
        elfHeader(62),
      );
    });

    verifyQingyuKernelBundle({
      bundleRoot: relative(process.cwd(), root),
      platform: "linux",
      run,
      target: "x86_64-unknown-linux-gnu",
    });

    expect(run.mock.calls[0][0]).toBe(appImage);
  });

  it("extracts the NSIS installer and validates its Kernel", async () => {
    const root = await temporaryBundleRoot();
    const installer = join(root, "nsis/QingYu_2.2.0_x64-setup.exe");
    await writeExecutable(installer, "NSIS");
    const run = vi.fn((_command, args) => {
      const outputDirectory = args.find((argument) => argument.startsWith("-o"))?.slice(2);
      writeExecutableSync(
        join(outputDirectory, "app/qingyu-kernel.exe"),
        peHeader(0x8664),
      );
    });

    const result = verifyQingyuKernelBundle({
      bundleRoot: root,
      platform: "windows",
      productName: "QingYu",
      run,
      target: "x86_64-pc-windows-msvc",
    });

    expect(result).toMatchObject({ installer, platform: "windows" });
    expect(run).toHaveBeenCalledTimes(1);
    expect(run).toHaveBeenCalledWith(
      "7z",
      ["x", "-y", expect.stringMatching(/^-o/u), installer],
      expect.objectContaining({ stdio: "pipe" }),
    );
  });

  it("propagates a macOS signature verification failure", async () => {
    const root = await temporaryBundleRoot();
    await writeExecutable(
      join(root, "macos/QingYu.app/Contents/MacOS/qingyu-kernel"),
      machHeader(0x0100000c),
    );

    expect(() =>
      verifyQingyuKernelBundle({
        bundleRoot: root,
        platform: "macos",
        productName: "QingYu",
        requireSignature: true,
        run() {
          throw new Error("not signed");
        },
        target: "aarch64-apple-darwin",
      }),
    ).toThrow("not signed");
  });

  it("rejects unsupported platforms before inspecting artifacts", async () => {
    const root = await temporaryBundleRoot();

    expect(() =>
      verifyQingyuKernelBundle({
        bundleRoot: root,
        platform: "android",
        target: "aarch64-linux-android",
      }),
    ).toThrow("Unsupported desktop bundle platform");
  });
});

describe("release workflow integration", () => {
  it("verifies every desktop bundle before artifacts are normalized or uploaded", () => {
    const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
    const workflow = readFileSync(join(repositoryRoot, ".github/workflows/release.yml"), "utf8");
    const verification = workflow.indexOf("- name: Verify bundled Kernel");
    const normalization = workflow.indexOf("- name: Normalize release asset names");
    const upload = workflow.indexOf("- name: Upload workflow artifacts");

    expect(verification).toBeGreaterThan(0);
    expect(verification).toBeLessThan(normalization);
    expect(verification).toBeLessThan(upload);
    expect(workflow).toContain("verify-qingyu-kernel-bundle.mjs");
    expect(workflow).toContain("--platform \"${ASSET_PLATFORM}\"");
    expect(workflow).toContain("--target \"${TARGET}\"");
    expect(workflow).toContain("--require-signature");
  });
});

async function temporaryBundleRoot() {
  return mkdtemp(join(tmpdir(), "qingyu-kernel-bundle-"));
}

async function writeExecutable(path, contents) {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, contents);
  await chmod(path, 0o755);
}

function writeExecutableSync(path, contents) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents);
  chmodSync(path, 0o755);
}

function machHeader(cpuType) {
  const contents = Buffer.alloc(104);
  contents.writeUInt32LE(0xfeedfacf, 0);
  contents.writeUInt32LE(cpuType, 4);
  contents.writeUInt32LE(2, 12);
  contents.writeUInt32LE(1, 16);
  contents.writeUInt32LE(72, 20);
  contents.writeUInt32LE(0x19, 32);
  contents.writeUInt32LE(72, 36);
  contents.writeBigUInt64LE(104n, 64);
  contents.writeBigUInt64LE(104n, 80);
  contents.writeUInt32LE(5, 92);
  return contents;
}

function elfHeader(machine) {
  const contents = Buffer.alloc(120);
  contents.set([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1], 0);
  contents.writeUInt16LE(3, 16);
  contents.writeUInt16LE(machine, 18);
  contents.writeBigUInt64LE(0x1000n, 24);
  contents.writeBigUInt64LE(64n, 32);
  contents.writeUInt16LE(64, 52);
  contents.writeUInt16LE(56, 54);
  contents.writeUInt16LE(1, 56);
  contents.writeUInt32LE(1, 64);
  contents.writeUInt32LE(5, 68);
  contents.writeBigUInt64LE(0x1000n, 80);
  contents.writeBigUInt64LE(120n, 96);
  contents.writeBigUInt64LE(120n, 104);
  return contents;
}

function peHeader(machine) {
  const contents = Buffer.alloc(256);
  contents.write("MZ", 0, "ascii");
  contents.writeUInt32LE(64, 0x3c);
  contents.write("PE\0\0", 64, "binary");
  contents.writeUInt16LE(machine, 68);
  contents.writeUInt16LE(1, 70);
  contents.writeUInt16LE(112, 84);
  contents.writeUInt16LE(0x0002, 86);
  contents.writeUInt16LE(0x020b, 88);
  contents.writeUInt32LE(0x1000, 104);
  contents.writeUInt32LE(16, 208);
  contents.writeUInt32LE(0x1000, 212);
  contents.writeUInt32LE(16, 216);
  contents.writeUInt32LE(240, 220);
  contents.writeUInt32LE(0x20000020, 236);
  contents.fill(0x90, 240, 256);
  return contents;
}
