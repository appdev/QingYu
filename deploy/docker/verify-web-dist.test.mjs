import assert from "node:assert/strict";
import {
  chmod,
  link,
  mkdtemp,
  mkdir,
  rm,
  symlink,
  truncate,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const verifier = fileURLToPath(new URL("./verify-web-dist.mjs", import.meta.url));
const finalVerifier = fileURLToPath(
  new URL("./verify-final-web-assets.sh", import.meta.url),
);
const roots = [];

async function fixture(name) {
  const root = await mkdtemp(path.join(tmpdir(), `qingyu-web-dist-${name}-`));
  roots.push(root);
  await mkdir(path.join(root, "assets"));
  return root;
}

function verify(root, environment = {}) {
  return spawnSync(process.execPath, [verifier, root], {
    encoding: "utf8",
    env: { PATH: process.env.PATH ?? "", ...environment },
  });
}

function verifyFinal(root, environment = {}) {
  return spawnSync("/bin/sh", [finalVerifier, root], {
    encoding: "utf8",
    env: { PATH: process.env.PATH ?? "", ...environment },
  });
}

async function createEmptyDirectories(parent, count) {
  const batchSize = 250;
  for (let offset = 0; offset < count; offset += batchSize) {
    const end = Math.min(offset + batchSize, count);
    await Promise.all(
      Array.from({ length: end - offset }, (_unused, index) =>
        mkdir(path.join(parent, `empty-${offset + index}`)),
      ),
    );
  }
}

test.after(async () => {
  await Promise.all(roots.map((root) => rm(root, { recursive: true, force: true })));
});

test("accepts a bounded passive static Web distribution", async () => {
  const root = await fixture("valid");
  await writeFile(path.join(root, "index.html"), "<!doctype html><main>QingYu</main>\n");
  await writeFile(path.join(root, "assets", "app.js"), "export const ready = true;\n");
  await writeFile(path.join(root, "assets", "app.css"), ":root { color-scheme: light dark; }\n");

  const result = verify(root);
  const finalResult = verifyFinal(root);

  assert.equal(result.status, 0, result.stderr);
  assert.equal(finalResult.status, 0, finalResult.stderr);
  assert.match(result.stdout, /^PASS: verified 3 Web distribution files/mu);
});

test("rejects symbolic links before following their targets", async () => {
  const root = await fixture("symlink");
  await writeFile(path.join(root, "index.html"), "safe\n");
  await symlink(path.join(root, "index.html"), path.join(root, "assets", "alias.html"));

  const result = verify(root);
  const finalResult = verifyFinal(root);

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /symbolic links are forbidden/u);
  assert.match(finalResult.stderr, /symbolic links and special files are forbidden/u);
});

test("rejects a regular Web asset hardlinked to a file outside the distribution", async () => {
  const root = await fixture("hardlink");
  const outside = await mkdtemp(path.join(tmpdir(), "qingyu-web-dist-hardlink-source-"));
  roots.push(outside);
  const source = path.join(outside, "private.txt");
  await writeFile(path.join(root, "index.html"), "safe\n");
  await writeFile(source, "private builder bytes\n");
  await link(source, path.join(root, "assets", "leak.txt"));

  const result = verify(root);
  const finalResult = verifyFinal(root);

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /hard links are forbidden/u);
  assert.match(finalResult.stderr, /hard links are forbidden/u);
});

test("rejects executable mode bits even for an allowed extension", async () => {
  const root = await fixture("executable");
  const script = path.join(root, "assets", "app.js");
  await writeFile(script, "console.log('unsafe');\n");
  await chmod(script, 0o755);

  const result = verify(root);
  const finalResult = verifyFinal(root);

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /executable files are forbidden/u);
  assert.match(finalResult.stderr, /executable files are forbidden/u);
});

test("rejects executable binary magic hidden behind an allowed extension", async () => {
  const root = await fixture("elf");
  await writeFile(path.join(root, "assets", "payload.js"), Buffer.from([0x7f, 0x45, 0x4c, 0x46]));

  const result = verify(root);
  const finalResult = verifyFinal(root);

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /executable binary content is forbidden/u);
  assert.match(finalResult.stderr, /executable binary content is forbidden/u);
});

test("rejects file types outside the passive Web allowlist", async () => {
  const root = await fixture("extension");
  await writeFile(path.join(root, "assets", "payload.bin"), "passive bytes\n");

  const result = verify(root);
  const finalResult = verifyFinal(root);

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /unsupported Web asset extension/u);
  assert.match(finalResult.stderr, /unsupported Web asset extension/u);
});

test("rejects a single oversized Web asset before reading it", async () => {
  const root = await fixture("single-limit");
  const oversized = path.join(root, "assets", "oversized.js");
  await writeFile(oversized, "");
  await truncate(oversized, 16 * 1024 * 1024 + 1);

  const result = verify(root);
  const finalResult = verifyFinal(root);

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /single-file byte limit/u);
  assert.match(finalResult.stderr, /single-file byte limit/u);
});

test("rejects an oversized aggregate Web distribution", async () => {
  const root = await fixture("aggregate-limit");
  for (let index = 0; index < 9; index += 1) {
    const chunk = path.join(root, "assets", `chunk-${index}.js`);
    await writeFile(chunk, "");
    await truncate(chunk, 16 * 1024 * 1024);
  }

  const result = verify(root);
  const finalResult = verifyFinal(root);

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /aggregate byte limit/u);
  assert.match(finalResult.stderr, /aggregate byte limit/u);
});

test("counts directories and files against a small injected node limit", async () => {
  const root = await fixture("small-node-limit");
  await writeFile(path.join(root, "index.html"), "safe\n");
  await writeFile(path.join(root, "assets", "app.js"), "export {};\n");

  const result = verify(root, { QINGYU_VERIFY_WEB_MAX_NODES: "2" });
  const finalResult = verifyFinal(root, { QINGYU_VERIFY_WEB_MAX_NODES: "2" });

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /node-count limit/u);
  assert.match(finalResult.stderr, /node-count limit/u);
});

test("rejects 10001 empty directories with the default node limit", async () => {
  const root = await fixture("default-node-limit");
  await writeFile(path.join(root, "index.html"), "safe\n");
  await createEmptyDirectories(path.join(root, "assets"), 10_001);

  const result = verify(root);
  const finalResult = verifyFinal(root);

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /node-count limit/u);
  assert.match(finalResult.stderr, /node-count limit/u);
});

test("charges every logical path against a small injected metadata limit", async () => {
  const root = await fixture("path-metadata-limit");
  await writeFile(path.join(root, "index.html"), "safe\n");
  await writeFile(path.join(root, "assets", "long-logical-name.js"), "export {};\n");

  const result = verify(root, { QINGYU_VERIFY_WEB_MAX_PATH_BYTES: "16" });
  const finalResult = verifyFinal(root, { QINGYU_VERIFY_WEB_MAX_PATH_BYTES: "16" });

  assert.notEqual(result.status, 0);
  assert.notEqual(finalResult.status, 0);
  assert.match(result.stderr, /path-metadata byte limit/u);
  assert.match(finalResult.stderr, /path-metadata byte limit/u);
});
