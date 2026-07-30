#!/usr/bin/env node

import { constants } from "node:fs";
import { lstat, open, opendir } from "node:fs/promises";
import path from "node:path";

const MAX_NODES = 10_000;
const MAX_DEPTH = 32;
const MAX_FILE_BYTES = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES = 128 * 1024 * 1024;
const MAX_PATH_METADATA_BYTES = 16 * 1024 * 1024;
const HEADER_BYTES = 4096;
const ALLOWED_EXTENSIONS = new Set([
  ".css",
  ".gif",
  ".html",
  ".ico",
  ".jpeg",
  ".jpg",
  ".js",
  ".json",
  ".mjs",
  ".otf",
  ".png",
  ".ttf",
  ".txt",
  ".webp",
  ".woff",
  ".woff2",
]);
const MACH_O_MAGICS = new Set([
  0xcafebabe,
  0xbebafeca,
  0xcafebabf,
  0xbfbafeca,
  0xfeedface,
  0xcefaedfe,
  0xfeedfacf,
  0xcffaedfe,
]);

function fail(message) {
  throw new Error(message);
}

function relativeLabel(root, absolutePath) {
  return path.relative(root, absolutePath).split(path.sep).join("/") || ".";
}

function injectedLimit(name, maximum) {
  const raw = process.env[name];
  if (raw === undefined) {
    return maximum;
  }
  if (!/^[1-9][0-9]*$/u.test(raw)) {
    fail(`${name} must be a positive decimal integer`);
  }
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value > maximum) {
    fail(`${name} may only inject a limit at or below ${maximum}`);
  }
  return value;
}

function verifierLimits() {
  return {
    maximumNodes: injectedLimit("QINGYU_VERIFY_WEB_MAX_NODES", MAX_NODES),
    maximumDepth: MAX_DEPTH,
    maximumFileBytes: MAX_FILE_BYTES,
    maximumTotalBytes: MAX_TOTAL_BYTES,
    maximumPathMetadataBytes: injectedLimit(
      "QINGYU_VERIFY_WEB_MAX_PATH_BYTES",
      MAX_PATH_METADATA_BYTES,
    ),
  };
}

function sameIdentity(left, right) {
  return left.dev === right.dev && left.ino === right.ino && left.nlink === right.nlink;
}

function sameStableMetadata(left, right) {
  return (
    sameIdentity(left, right) &&
    left.size === right.size &&
    left.mode === right.mode &&
    left.mtimeNs === right.mtimeNs &&
    left.ctimeNs === right.ctimeNs
  );
}

function requireSingleLink(metadata, label) {
  if (metadata.nlink !== 1n) {
    fail(`hard links are forbidden in the Web distribution: ${label}`);
  }
}

function chargeEntry(state, label, limits) {
  state.nodes += 1;
  if (state.nodes > limits.maximumNodes) {
    fail("Web distribution exceeds the node-count limit");
  }
  state.pathMetadataBytes += Buffer.byteLength(label);
  if (state.pathMetadataBytes > limits.maximumPathMetadataBytes) {
    fail("Web distribution exceeds the path-metadata byte limit");
  }
}

function hasExecutableBinaryMagic(header) {
  if (
    header.length >= 4 &&
    header[0] === 0x7f &&
    header[1] === 0x45 &&
    header[2] === 0x4c &&
    header[3] === 0x46
  ) {
    return true;
  }
  if (header.length >= 4 && MACH_O_MAGICS.has(header.readUInt32BE(0))) {
    return true;
  }
  return header.length >= 2 && header[0] === 0x4d && header[1] === 0x5a;
}

async function inspectRegularFile(root, absolutePath, initial, state, limits) {
  const label = relativeLabel(root, absolutePath);
  requireSingleLink(initial, label);
  if ((initial.mode & 0o111n) !== 0n) {
    fail(`executable files are forbidden in the Web distribution: ${label}`);
  }
  const extension = path.extname(absolutePath).toLowerCase();
  if (!ALLOWED_EXTENSIONS.has(extension)) {
    fail(`unsupported Web asset extension: ${label}`);
  }
  if (initial.size > BigInt(limits.maximumFileBytes)) {
    fail(`Web asset exceeds the single-file byte limit: ${label}`);
  }
  state.totalBytes += Number(initial.size);
  if (state.totalBytes > limits.maximumTotalBytes) {
    fail("Web distribution exceeds the aggregate byte limit");
  }
  state.files += 1;

  if (constants.O_NOFOLLOW === undefined) {
    fail("this platform cannot verify Web assets without following symbolic links");
  }
  const handle = await open(absolutePath, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const opened = await handle.stat({ bigint: true });
    requireSingleLink(opened, label);
    if (!opened.isFile() || !sameStableMetadata(initial, opened)) {
      fail(`Web asset changed while it was being verified: ${label}`);
    }
    const header = Buffer.alloc(Math.min(HEADER_BYTES, Number(opened.size)));
    if (header.length > 0) {
      const { bytesRead } = await handle.read(header, 0, header.length, 0);
      if (bytesRead !== header.length) {
        fail(`Web asset could not be read completely: ${label}`);
      }
    }
    if (hasExecutableBinaryMagic(header)) {
      fail(`executable binary content is forbidden in the Web distribution: ${label}`);
    }
    const after = await handle.stat({ bigint: true });
    requireSingleLink(after, label);
    if (!sameStableMetadata(opened, after)) {
      fail(`Web asset changed while it was being verified: ${label}`);
    }
    const named = await lstat(absolutePath, { bigint: true });
    requireSingleLink(named, label);
    if (named.isSymbolicLink() || !named.isFile() || !sameStableMetadata(after, named)) {
      fail(`Web asset changed while it was being verified: ${label}`);
    }
    return named;
  } finally {
    await handle.close();
  }
}

async function inspectDirectory(root, directory, initial, depth, state, limits) {
  if (depth > limits.maximumDepth) {
    fail("Web distribution exceeds the directory-depth limit");
  }
  const retained = await opendir(directory);
  let iterationStarted = false;
  try {
    const openedNamed = await lstat(directory, { bigint: true });
    if (
      openedNamed.isSymbolicLink() ||
      !openedNamed.isDirectory() ||
      !sameStableMetadata(initial, openedNamed)
    ) {
      fail(`Web directory changed while it was being verified: ${relativeLabel(root, directory)}`);
    }
    iterationStarted = true;
    for await (const entry of retained) {
      const absolutePath = path.join(directory, entry.name);
      const label = relativeLabel(root, absolutePath);
      chargeEntry(state, label, limits);
      const metadata = await lstat(absolutePath, { bigint: true });
      if (metadata.isSymbolicLink()) {
        fail(`symbolic links are forbidden in the Web distribution: ${label}`);
      }
      if (metadata.isDirectory()) {
        await inspectDirectory(root, absolutePath, metadata, depth + 1, state, limits);
      } else if (metadata.isFile()) {
        const named = await inspectRegularFile(
          root,
          absolutePath,
          metadata,
          state,
          limits,
        );
        if (label === "index.html") {
          state.rootIndex = named;
        }
      } else {
        fail(`only regular files and directories are allowed in the Web distribution: ${label}`);
      }
    }
  } finally {
    if (!iterationStarted) {
      await retained.close();
    }
  }
  const after = await lstat(directory, { bigint: true });
  if (after.isSymbolicLink() || !after.isDirectory() || !sameStableMetadata(initial, after)) {
    fail(`Web directory changed while it was being verified: ${relativeLabel(root, directory)}`);
  }
}

async function main() {
  if (process.argv.length !== 3 || process.argv[2] === "") {
    fail("usage: verify-web-dist.mjs <Web distribution directory>");
  }
  const root = path.resolve(process.argv[2]);
  const rootMetadata = await lstat(root, { bigint: true });
  if (rootMetadata.isSymbolicLink() || !rootMetadata.isDirectory()) {
    fail("Web distribution root must be a retained directory, not a symbolic link");
  }
  const limits = verifierLimits();
  const state = {
    files: 0,
    nodes: 0,
    pathMetadataBytes: 0,
    rootIndex: null,
    totalBytes: 0,
  };
  await inspectDirectory(root, root, rootMetadata, 0, state, limits);
  if (state.files === 0) {
    fail("Web distribution must not be empty");
  }
  if (state.rootIndex === null) {
    fail("Web distribution must contain a regular root index.html");
  }
  const indexMetadata = await lstat(path.join(root, "index.html"), { bigint: true });
  requireSingleLink(indexMetadata, "index.html");
  if (
    !indexMetadata.isFile() ||
    indexMetadata.isSymbolicLink() ||
    !sameStableMetadata(state.rootIndex, indexMetadata)
  ) {
    fail("Web distribution root index.html changed after verification");
  }
  process.stdout.write(
    `PASS: verified ${state.files} Web distribution files (${state.totalBytes} bytes).\n`,
  );
}

main().catch((error) => {
  const message = error instanceof Error ? error.message : "unknown Web distribution error";
  process.stderr.write(`FAIL: ${message}\n`);
  process.exitCode = 1;
});
