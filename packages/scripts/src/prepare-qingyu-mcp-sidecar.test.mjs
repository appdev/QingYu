import { chmod, mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { describe, expect, it, vi } from "vitest";

import {
  mcpCargoBuildArgs,
  mcpSidecarPaths,
  prepareMcpSidecar,
  resolveMcpTargetTriple,
  validatePreparedMcpSidecar,
} from "./prepare-qingyu-mcp-sidecar.mjs";

const rustcHost = [
  "rustc 1.96.0",
  "binary: rustc",
  "host: aarch64-apple-darwin",
  "release: 1.96.0",
].join("\n");

describe("resolveMcpTargetTriple", () => {
  it("uses the desktop target priority shared with Kernel packaging", () => {
    expect(
      resolveMcpTargetTriple({
        environment: {
          MARKRA_DESKTOP_TARGET: "x86_64-pc-windows-msvc",
          TAURI_ENV_TARGET_TRIPLE: "x86_64-unknown-linux-gnu",
          CARGO_BUILD_TARGET: "aarch64-apple-darwin",
        },
        rustcVersion: rustcHost,
      }),
    ).toBe("x86_64-pc-windows-msvc");

    expect(
      resolveMcpTargetTriple({
        environment: { TAURI_ENV_TARGET_TRIPLE: "x86_64-unknown-linux-gnu" },
        rustcVersion: rustcHost,
      }),
    ).toBe("x86_64-unknown-linux-gnu");

    expect(
      resolveMcpTargetTriple({
        environment: { CARGO_BUILD_TARGET: "x86_64-pc-windows-msvc" },
        rustcVersion: rustcHost,
      }),
    ).toBe("x86_64-pc-windows-msvc");

    expect(
      resolveMcpTargetTriple({ environment: {}, rustcVersion: rustcHost }),
    ).toBe("aarch64-apple-darwin");
  });
});

describe("MCP sidecar build contract", () => {
  it("uses a locked Cargo build for the explicit desktop target", () => {
    expect(
      mcpCargoBuildArgs(
        "/repo/apps/desktop/src-tauri/Cargo.toml",
        "x86_64-unknown-linux-gnu",
      ),
    ).toEqual([
      "build",
      "--manifest-path",
      "/repo/apps/desktop/src-tauri/Cargo.toml",
      "--bin",
      "qingyu-mcp",
      "--features",
      "desktop-sidecar",
      "--locked",
      "--release",
      "--target",
      "x86_64-unknown-linux-gnu",
    ]);
  });

  it("reads target-qualified output and writes the Tauri target-qualified sidecar", () => {
    expect(
      mcpSidecarPaths("/repo", "x86_64-pc-windows-msvc"),
    ).toEqual({
      source: join(
        "/repo",
        "apps/desktop/src-tauri/target/x86_64-pc-windows-msvc/release/qingyu-mcp.exe",
      ),
      destination: join(
        "/repo",
        "apps/desktop/src-tauri/binaries/qingyu-mcp-x86_64-pc-windows-msvc.exe",
      ),
    });
  });

  it("copies and validates the sidecar produced for the explicit target", async () => {
    const root = await mkdtemp(join(tmpdir(), "qingyu-mcp-prepare-"));
    const target = "aarch64-apple-darwin";
    const paths = mcpSidecarPaths(root, target);
    const calls = [];
    const run = vi.fn((command, args) => {
      calls.push([command, args]);
      if (command !== "cargo") return rustcHost;
      return undefined;
    });

    await writeFileWithParents(paths.source, machHeader(0x0100000c));
    await chmod(paths.source, 0o755);

    const prepared = prepareMcpSidecar({
      environment: { MARKRA_DESKTOP_TARGET: target },
      root,
      run,
    });

    expect(calls).toEqual([
      [
        "cargo",
        mcpCargoBuildArgs(
          join(root, "apps/desktop/src-tauri/Cargo.toml"),
          target,
        ),
      ],
    ]);
    expect(await readFile(paths.destination)).toEqual(machHeader(0x0100000c));
    expect(prepared).toMatchObject({ ...paths, target });
  });
});

describe("validatePreparedMcpSidecar", () => {
  it("rejects an executable container for the wrong target CPU", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qingyu-mcp-arch-"));
    const path = join(directory, "qingyu-mcp");
    await writeFile(path, machHeader(0x01000007));
    await chmod(path, 0o755);

    expect(() =>
      validatePreparedMcpSidecar(path, "aarch64-apple-darwin", "darwin"),
    ).toThrow("target architecture aarch64");
  });
});

async function writeFileWithParents(path, contents) {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, contents);
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
