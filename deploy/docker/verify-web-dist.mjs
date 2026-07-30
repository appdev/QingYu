#!/usr/bin/env node

import { constants } from "node:fs";
import { lstat, open, readdir } from "node:fs/promises";
import path from "node:path";

const MAX_FILES = 10_000;
const MAX_DEPTH = 32;
const MAX_FILE_BYTES = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES = 128 * 1024 * 1024;
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

async function inspectRegularFile(root, absolutePath, initial, state) {
  const label = relativeLabel(root, absolutePath);
  if ((initial.mode & 0o111) !== 0) {
    fail(`executable files are forbidden in the Web distribution: ${label}`);
  }
  const extension = path.extname(absolutePath).toLowerCase();
  if (!ALLOWED_EXTENSIONS.has(extension)) {
    fail(`unsupported Web asset extension: ${label}`);
  }
  if (initial.size > MAX_FILE_BYTES) {
    fail(`Web asset exceeds the single-file byte limit: ${label}`);
  }
  state.totalBytes += initial.size;
  if (state.totalBytes > MAX_TOTAL_BYTES) {
    fail("Web distribution exceeds the aggregate byte limit");
  }
  state.files += 1;
  if (state.files > MAX_FILES) {
    fail("Web distribution exceeds the file-count limit");
  }

  const noFollow = constants.O_NOFOLLOW ?? 0;
  const handle = await open(absolutePath, constants.O_RDONLY | noFollow);
  try {
    const opened = await handle.stat();
    if (!opened.isFile() || opened.size !== initial.size || opened.mode !== initial.mode) {
      fail(`Web asset changed while it was being verified: ${label}`);
    }
    const header = Buffer.alloc(Math.min(HEADER_BYTES, opened.size));
    if (header.length > 0) {
      const { bytesRead } = await handle.read(header, 0, header.length, 0);
      if (bytesRead !== header.length) {
        fail(`Web asset could not be read completely: ${label}`);
      }
    }
    if (hasExecutableBinaryMagic(header)) {
      fail(`executable binary content is forbidden in the Web distribution: ${label}`);
    }
    const after = await handle.stat();
    if (after.size !== opened.size || after.mode !== opened.mode || after.mtimeMs !== opened.mtimeMs) {
      fail(`Web asset changed while it was being verified: ${label}`);
    }
  } finally {
    await handle.close();
  }
}

async function inspectDirectory(root, directory, depth, state) {
  if (depth > MAX_DEPTH) {
    fail("Web distribution exceeds the directory-depth limit");
  }
  const entries = await readdir(directory, { withFileTypes: true });
  entries.sort((left, right) => left.name.localeCompare(right.name, "en"));
  for (const entry of entries) {
    const absolutePath = path.join(directory, entry.name);
    const metadata = await lstat(absolutePath);
    const label = relativeLabel(root, absolutePath);
    if (metadata.isSymbolicLink()) {
      fail(`symbolic links are forbidden in the Web distribution: ${label}`);
    }
    if (metadata.isDirectory()) {
      await inspectDirectory(root, absolutePath, depth + 1, state);
    } else if (metadata.isFile()) {
      await inspectRegularFile(root, absolutePath, metadata, state);
    } else {
      fail(`only regular files and directories are allowed in the Web distribution: ${label}`);
    }
  }
}

async function main() {
  if (process.argv.length !== 3 || process.argv[2] === "") {
    fail("usage: verify-web-dist.mjs <Web distribution directory>");
  }
  const root = path.resolve(process.argv[2]);
  const rootMetadata = await lstat(root);
  if (rootMetadata.isSymbolicLink() || !rootMetadata.isDirectory()) {
    fail("Web distribution root must be a retained directory, not a symbolic link");
  }
  const state = { files: 0, totalBytes: 0 };
  await inspectDirectory(root, root, 0, state);
  if (state.files === 0) {
    fail("Web distribution must not be empty");
  }
  const indexMetadata = await lstat(path.join(root, "index.html"));
  if (!indexMetadata.isFile() || indexMetadata.isSymbolicLink()) {
    fail("Web distribution must contain a regular root index.html");
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
