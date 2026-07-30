import assert from "node:assert/strict";
import { chmod, mkdtemp, mkdir, rm, symlink, truncate, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const verifier = fileURLToPath(new URL("./verify-web-dist.mjs", import.meta.url));
const roots = [];

async function fixture(name) {
  const root = await mkdtemp(path.join(tmpdir(), `qingyu-web-dist-${name}-`));
  roots.push(root);
  await mkdir(path.join(root, "assets"));
  return root;
}

function verify(root) {
  return spawnSync(process.execPath, [verifier, root], {
    encoding: "utf8",
    env: { PATH: process.env.PATH ?? "" },
  });
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

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^PASS: verified 3 Web distribution files/mu);
});

test("rejects symbolic links before following their targets", async () => {
  const root = await fixture("symlink");
  await writeFile(path.join(root, "index.html"), "safe\n");
  await symlink(path.join(root, "index.html"), path.join(root, "assets", "alias.html"));

  const result = verify(root);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /symbolic links are forbidden/u);
});

test("rejects executable mode bits even for an allowed extension", async () => {
  const root = await fixture("executable");
  const script = path.join(root, "assets", "app.js");
  await writeFile(script, "console.log('unsafe');\n");
  await chmod(script, 0o755);

  const result = verify(root);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /executable files are forbidden/u);
});

test("rejects executable binary magic hidden behind an allowed extension", async () => {
  const root = await fixture("elf");
  await writeFile(path.join(root, "assets", "payload.js"), Buffer.from([0x7f, 0x45, 0x4c, 0x46]));

  const result = verify(root);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /executable binary content is forbidden/u);
});

test("rejects file types outside the passive Web allowlist", async () => {
  const root = await fixture("extension");
  await writeFile(path.join(root, "assets", "payload.bin"), "passive bytes\n");

  const result = verify(root);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /unsupported Web asset extension/u);
});

test("rejects a single oversized Web asset before reading it", async () => {
  const root = await fixture("single-limit");
  const oversized = path.join(root, "assets", "oversized.js");
  await writeFile(oversized, "");
  await truncate(oversized, 16 * 1024 * 1024 + 1);

  const result = verify(root);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /single-file byte limit/u);
});

test("rejects an oversized aggregate Web distribution", async () => {
  const root = await fixture("aggregate-limit");
  for (let index = 0; index < 9; index += 1) {
    const chunk = path.join(root, "assets", `chunk-${index}.js`);
    await writeFile(chunk, "");
    await truncate(chunk, 16 * 1024 * 1024);
  }

  const result = verify(root);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /aggregate byte limit/u);
});
