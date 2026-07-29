import { chmod, link, mkdir, mkdtemp, readFile, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import {
  copyKernelSidecarAtomically,
  kernelCargoBuildArgs,
  kernelSidecarPaths,
  prepareKernelSidecar,
  resolveKernelTargetTriple,
  validatePreparedKernelSidecar,
} from "./prepare-qingyu-kernel-sidecar.mjs";

const rustcHost = [
  "rustc 1.96.0",
  "binary: rustc",
  "host: aarch64-apple-darwin",
  "release: 1.96.0",
].join("\n");

describe("resolveKernelTargetTriple", () => {
  it("prefers the explicit desktop build target and validates it", () => {
    expect(
      resolveKernelTargetTriple({
        environment: { MARKRA_DESKTOP_TARGET: "x86_64-pc-windows-msvc" },
        rustcVersion: rustcHost,
      }),
    ).toBe("x86_64-pc-windows-msvc");

    expect(() =>
      resolveKernelTargetTriple({
        environment: { MARKRA_DESKTOP_TARGET: "../../escape" },
        rustcVersion: rustcHost,
      }),
    ).toThrow("valid desktop Rust target triple");
    expect(() =>
      resolveKernelTargetTriple({
        environment: { MARKRA_DESKTOP_TARGET: "aarch64-linux-android" },
        rustcVersion: rustcHost,
      }),
    ).toThrow("valid desktop Rust target triple");
  });

  it("uses the rustc host triple for a native desktop build", () => {
    expect(
      resolveKernelTargetTriple({ environment: {}, rustcVersion: rustcHost }),
    ).toBe("aarch64-apple-darwin");
  });

  it("accepts the target triple supplied by the Tauri CLI", () => {
    expect(
      resolveKernelTargetTriple({
        environment: { TAURI_ENV_TARGET_TRIPLE: "x86_64-unknown-linux-gnu" },
        rustcVersion: "",
      }),
    ).toBe("x86_64-unknown-linux-gnu");
  });
});

describe("Kernel sidecar build contract", () => {
  it("always builds the Kernel binary for an explicit Cargo target", () => {
    expect(
      kernelCargoBuildArgs("/repo/apps/kernel/Cargo.toml", "x86_64-unknown-linux-gnu"),
    ).toEqual([
      "build",
      "--manifest-path",
      "/repo/apps/kernel/Cargo.toml",
      "--bin",
      "qingyu-kernel",
      "--locked",
      "--release",
      "--target",
      "x86_64-unknown-linux-gnu",
    ]);
  });

  it("uses target-qualified source and Tauri sidecar paths", () => {
    expect(
      kernelSidecarPaths("/repo", "x86_64-pc-windows-msvc"),
    ).toEqual({
      source: join(
        "/repo",
        "apps/kernel/target/x86_64-pc-windows-msvc/release/qingyu-kernel.exe",
      ),
      destination: join(
        "/repo",
        "apps/desktop/src-tauri/binaries/qingyu-kernel-x86_64-pc-windows-msvc.exe",
      ),
    });
  });

  it("builds, validates, and atomically publishes the target-qualified sidecar", async () => {
    const root = await mkdtemp(join(tmpdir(), "qingyu-kernel-prepare-"));
    const target = "x86_64-unknown-linux-gnu";
    const paths = kernelSidecarPaths(root, target);
    await mkdir(join(root, "apps/kernel"), { recursive: true });
    await mkdir(join(paths.source, ".."), { recursive: true });
    await mkdir(join(root, "apps/desktop/src-tauri/binaries"), { recursive: true });
    await writeFile(paths.source, elfHeader(62), { mode: 0o755 });
    const calls = [];

    const result = prepareKernelSidecar({
      environment: { MARKRA_DESKTOP_TARGET: target },
      root,
      run(command, arguments_, options) {
        calls.push({ command, arguments_, options });
        return "";
      },
    });

    expect(calls).toHaveLength(1);
    expect(calls[0].command).toBe("cargo");
    expect(calls[0].arguments_).toEqual(
      kernelCargoBuildArgs(join(root, "apps/kernel/Cargo.toml"), target),
    );
    expect(result).toMatchObject({ ...paths, target, byteLength: 120 });
    expect(await readFile(paths.destination)).toEqual(elfHeader(62));
  });
});

describe("validatePreparedKernelSidecar", () => {
  it.each([
    ["aarch64-apple-darwin", machHeader(0x0100000c)],
    ["x86_64-unknown-linux-gnu", elfHeader(62)],
    ["x86_64-pc-windows-msvc", peHeader(0x8664)],
  ])("accepts a non-empty executable for %s", async (target, contents) => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-"));
    const path = join(directory, target.includes("windows") ? "kernel.exe" : "kernel");
    await writeFile(path, contents);
    if (!target.includes("windows")) await chmod(path, 0o755);

    expect(validatePreparedKernelSidecar(path, target, "darwin")).toMatchObject({
      byteLength: contents.length,
      target,
    });
  });

  it("rejects missing, empty, non-executable, and wrong-format files", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-invalid-"));
    const missing = join(directory, "missing");
    const empty = join(directory, "empty");
    const nonExecutable = join(directory, "non-executable");
    const wrongFormat = join(directory, "wrong-format");
    await writeFile(empty, "");
    await writeFile(nonExecutable, elfHeader(62));
    await chmod(nonExecutable, 0o644);
    await writeFile(wrongFormat, "not an executable");
    await chmod(wrongFormat, 0o755);

    expect(() =>
      validatePreparedKernelSidecar(missing, "x86_64-unknown-linux-gnu", "darwin"),
    ).toThrow("does not exist");
    expect(() =>
      validatePreparedKernelSidecar(empty, "x86_64-unknown-linux-gnu", "darwin"),
    ).toThrow("must not be empty");
    expect(() =>
      validatePreparedKernelSidecar(
        nonExecutable,
        "x86_64-unknown-linux-gnu",
        "darwin",
      ),
    ).toThrow("must be executable");
    expect(() =>
      validatePreparedKernelSidecar(
        wrongFormat,
        "x86_64-unknown-linux-gnu",
        "darwin",
      ),
    ).toThrow("ELF executable");
  });

  it.each([
    ["aarch64-apple-darwin", machHeader(0x01000007), "aarch64"],
    ["aarch64-unknown-linux-gnu", elfHeader(62), "aarch64"],
    ["aarch64-pc-windows-msvc", peHeader(0x8664), "aarch64"],
  ])("rejects a valid container with the wrong CPU for %s", async (target, contents, architecture) => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-arch-"));
    const path = join(directory, target.includes("windows") ? "kernel.exe" : "kernel");
    await writeFile(path, contents);
    if (!target.includes("windows")) await chmod(path, 0o755);

    expect(() => validatePreparedKernelSidecar(path, target, "darwin")).toThrow(
      `target architecture ${architecture}`,
    );
  });

  it("rejects a PE DLL even when its CPU matches", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-dll-"));
    const path = join(directory, "kernel.exe");
    await writeFile(path, peHeader(0x8664, { dll: true }));

    expect(() =>
      validatePreparedKernelSidecar(path, "x86_64-pc-windows-msvc", "win32"),
    ).toThrow("must not be a DLL");
  });

  it("validates the requested architecture inside a universal Mach-O", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-fat-"));
    const path = join(directory, "kernel");
    await writeFile(path, fatMachO(0x0100000c));
    await chmod(path, 0o755);

    expect(
      validatePreparedKernelSidecar(path, "aarch64-apple-darwin", "darwin"),
    ).toMatchObject({ target: "aarch64-apple-darwin" });
    expect(() =>
      validatePreparedKernelSidecar(path, "x86_64-apple-darwin", "darwin"),
    ).toThrow("target architecture x86_64");
  });

  it.each([
    ["aarch64-apple-darwin", machHeader(0x0100000c).subarray(0, 16), "Mach-O"],
    ["x86_64-unknown-linux-gnu", elfHeader(62).subarray(0, 32), "ELF"],
    ["x86_64-pc-windows-msvc", peHeader(0x8664).subarray(0, 90), "PE"],
  ])("rejects a truncated executable header for %s", async (target, contents, format) => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-truncated-"));
    const path = join(directory, target.includes("windows") ? "kernel.exe" : "kernel");
    await writeFile(path, contents);
    if (!target.includes("windows")) await chmod(path, 0o755);

    expect(() => validatePreparedKernelSidecar(path, target, "darwin")).toThrow(format);
  });

  it("rejects a universal Mach-O whose selected slice has an invalid declared size", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-fat-size-"));
    const path = join(directory, "kernel");
    await writeFile(path, fatMachO(0x0100000c, { declaredSize: 0 }));
    await chmod(path, 0o755);

    expect(() =>
      validatePreparedKernelSidecar(path, "aarch64-apple-darwin", "darwin"),
    ).toThrow("slice size");
  });

  it.each([
    ["aarch64-apple-darwin", machHeader(0x0100000c, { payload: false }), "Mach-O"],
    ["x86_64-unknown-linux-gnu", elfHeader(62, { payload: false }), "ELF"],
    ["x86_64-pc-windows-msvc", peHeader(0x8664, { payload: false }), "PE"],
  ])("rejects an executable mapping without a bounded payload for %s", async (target, contents, format) => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-no-payload-"));
    const path = join(directory, target.includes("windows") ? "kernel.exe" : "kernel");
    await writeFile(path, contents);
    if (!target.includes("windows")) await chmod(path, 0o755);

    expect(() => validatePreparedKernelSidecar(path, target, "darwin")).toThrow(format);
  });

  it.runIf(process.platform !== "win32")("rejects hard-linked sources before publication", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-hardlink-"));
    const source = join(directory, "source");
    const secondLink = join(directory, "second-link");
    const destination = join(directory, "destination");
    await writeFile(source, elfHeader(62));
    await chmod(source, 0o755);
    await link(source, secondLink);

    expect(() =>
      copyKernelSidecarAtomically(
        source,
        destination,
        "x86_64-unknown-linux-gnu",
        process.platform,
      ),
    ).toThrow("must not be a hard link");
  });

  it.runIf(process.platform !== "win32")("rejects symbolic links", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-kernel-sidecar-link-"));
    const target = join(directory, "target");
    const link = join(directory, "link");
    await writeFile(target, elfHeader(62));
    await chmod(target, 0o755);
    await symlink(target, link);

    expect(() =>
      validatePreparedKernelSidecar(link, "x86_64-unknown-linux-gnu", process.platform),
    ).toThrow("must not be a symbolic link");
  });
});

function machHeader(cpuType, { payload = true } = {}) {
  const contents = Buffer.alloc(104);
  contents.writeUInt32LE(0xfeedfacf, 0);
  contents.writeUInt32LE(cpuType, 4);
  contents.writeUInt32LE(2, 12);
  contents.writeUInt32LE(1, 16);
  contents.writeUInt32LE(72, 20);
  contents.writeUInt32LE(0x19, 32);
  contents.writeUInt32LE(72, 36);
  if (payload) {
    contents.writeBigUInt64LE(104n, 64);
    contents.writeBigUInt64LE(104n, 80);
  }
  contents.writeUInt32LE(5, 92);
  return contents;
}

function elfHeader(machine, { payload = true } = {}) {
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
  if (payload) {
    contents.writeBigUInt64LE(0x1000n, 80);
    contents.writeBigUInt64LE(120n, 96);
    contents.writeBigUInt64LE(120n, 104);
  }
  return contents;
}

function peHeader(machine, { dll = false, payload = true } = {}) {
  const contents = Buffer.alloc(payload ? 256 : 240);
  contents.write("MZ", 0, "ascii");
  contents.writeUInt32LE(64, 0x3c);
  contents.write("PE\0\0", 64, "binary");
  contents.writeUInt16LE(machine, 68);
  contents.writeUInt16LE(1, 70);
  contents.writeUInt16LE(112, 84);
  contents.writeUInt16LE(0x0002 | (dll ? 0x2000 : 0), 86);
  contents.writeUInt16LE(0x020b, 88);
  contents.writeUInt32LE(0x1000, 104);
  if (payload) {
    contents.writeUInt32LE(16, 208);
    contents.writeUInt32LE(0x1000, 212);
    contents.writeUInt32LE(16, 216);
    contents.writeUInt32LE(240, 220);
    contents.fill(0x90, 240, 256);
  }
  contents.writeUInt32LE(0x20000020, 236);
  return contents;
}

function fatMachO(cpuType, { declaredSize } = {}) {
  const slice = machHeader(cpuType);
  const contents = Buffer.alloc(28 + slice.length);
  contents.writeUInt32BE(0xcafebabe, 0);
  contents.writeUInt32BE(1, 4);
  contents.writeUInt32BE(cpuType, 8);
  contents.writeUInt32BE(0, 12);
  contents.writeUInt32BE(28, 16);
  contents.writeUInt32BE(declaredSize ?? slice.length, 20);
  contents.writeUInt32BE(0, 24);
  slice.copy(contents, 28);
  return contents;
}
