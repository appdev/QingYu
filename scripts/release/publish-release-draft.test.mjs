import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const publisherModule = await import("./publish-release-draft.mjs").catch(() => ({}));
const { publishReleaseDraft } = publisherModule;

function jsonResponse(status, body) {
  return new Response(body === null ? null : JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function createGitHubApi({
  target = "old-target",
  draft = true,
  body = "old body\n",
  hasRelease = true,
  tamperDigest = false,
  publishAfterUpload = false,
} = {}) {
  const requests = [];
  const release = {
    id: 123,
    tag_name: "v2.4.0",
    target_commitish: target,
    draft,
    prerelease: false,
    name: "QingYu v2.4.0",
    body,
  };
  let assets = [{ id: 90, name: "old.zip", size: 3 }];
  let nextAssetId = 100;
  let releaseReads = 0;

  async function fetchImpl(url, options = {}) {
    const parsed = new URL(url);
    const method = options.method || "GET";
    requests.push({ method, pathname: parsed.pathname, search: parsed.search });

    if (method === "GET" && parsed.pathname.endsWith("/releases")) {
      return jsonResponse(200, hasRelease ? [release] : []);
    }
    if (method === "POST" && parsed.pathname.endsWith("/releases")) {
      Object.assign(release, JSON.parse(options.body), { id: 123 });
      hasRelease = true;
      assets = [];
      return jsonResponse(201, release);
    }
    if (method === "PATCH" && parsed.pathname.endsWith("/releases/123")) {
      Object.assign(release, JSON.parse(options.body));
      return jsonResponse(200, release);
    }
    if (method === "GET" && parsed.pathname.endsWith("/releases/123")) {
      releaseReads += 1;
      if (publishAfterUpload && releaseReads >= 2) {
        release.draft = false;
      }
      return jsonResponse(200, release);
    }
    if (method === "GET" && parsed.pathname.endsWith("/releases/123/assets")) {
      return jsonResponse(200, assets);
    }
    if (method === "DELETE" && parsed.pathname.endsWith("/releases/assets/90")) {
      assets = assets.filter((asset) => asset.id !== 90);
      return jsonResponse(204, null);
    }
    if (method === "POST" && parsed.pathname.endsWith("/releases/123/assets")) {
      let uploadedSize = 0;
      const hash = createHash("sha256");
      for await (const chunk of options.body) {
        uploadedSize += chunk.length;
        hash.update(chunk);
      }
      assert.equal(uploadedSize, Number(options.headers["Content-Length"]));
      const asset = {
        id: nextAssetId++,
        name: parsed.searchParams.get("name"),
        size: uploadedSize,
        state: "uploaded",
        digest: `sha256:${tamperDigest ? "0".repeat(64) : hash.digest("hex")}`,
      };
      assets.push(asset);
      return jsonResponse(201, asset);
    }
    throw new Error(`Unexpected request: ${method} ${url}`);
  }

  return { fetchImpl, release, requests };
}

test("draft publisher exposes an ID-bound mutation helper", () => {
  assert.equal(typeof publishReleaseDraft, "function");
});

test("draft publisher updates and uploads only through the validated release ID", async (context) => {
  const temporaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-draft-publish-"));
  context.after(() => fs.rmSync(temporaryDirectory, { recursive: true, force: true }));
  const assetPath = path.join(temporaryDirectory, "QingYu_test.zip");
  fs.writeFileSync(assetPath, "asset", "utf8");
  const api = createGitHubApi();

  const result = await publishReleaseDraft({
    repository: "appdev/QingYu",
    token: "test-token",
    releaseTag: "v2.4.0",
    releaseTarget: "new-target",
    releaseName: "QingYu v2.4.0",
    baselineNotes: "baseline notes\n",
    prerelease: false,
    allowedStaleDraftId: "123",
    assetPaths: [assetPath],
    fetchImpl: api.fetchImpl,
  });

  assert.deepEqual(result, { releaseId: 123, uploadedAssets: ["QingYu_test.zip"] });
  assert.equal(api.release.target_commitish, "new-target");
  assert.equal(api.release.body, "baseline notes\n");
  assert.equal(api.release.draft, true);
  assert.ok(api.requests.some((request) => request.pathname.endsWith("/releases/123")));
  assert.ok(api.requests.some((request) => request.pathname.endsWith("/releases/123/assets")));
  assert.ok(api.requests.some((request) => request.pathname.endsWith("/releases/assets/90")));
  assert.equal(
    api.requests.filter(
      (request) => request.method === "GET" && request.pathname.endsWith("/releases/123"),
    ).length,
    2,
  );
  assert.equal(api.requests.some((request) => request.pathname.endsWith("/releases/tags/v2.4.0")), false);
});

test("draft publisher refuses a stale draft unless its exact ID is authorized", async (context) => {
  const temporaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-draft-publish-"));
  context.after(() => fs.rmSync(temporaryDirectory, { recursive: true, force: true }));
  const assetPath = path.join(temporaryDirectory, "QingYu_test.zip");
  fs.writeFileSync(assetPath, "asset", "utf8");
  const api = createGitHubApi();

  await assert.rejects(
    () =>
      publishReleaseDraft({
        repository: "appdev/QingYu",
        token: "test-token",
        releaseTag: "v2.4.0",
        releaseTarget: "new-target",
        releaseName: "QingYu v2.4.0",
        baselineNotes: "baseline notes\n",
        prerelease: false,
        allowedStaleDraftId: null,
        assetPaths: [assetPath],
        fetchImpl: api.fetchImpl,
      }),
    /explicit stale draft ID/u,
  );
  assert.equal(api.requests.some((request) => request.method !== "GET"), false);
});

test("draft publisher creates a new ID-bound draft when the tag is unused", async (context) => {
  const temporaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-draft-publish-"));
  context.after(() => fs.rmSync(temporaryDirectory, { recursive: true, force: true }));
  const assetPath = path.join(temporaryDirectory, "QingYu_test.zip");
  fs.writeFileSync(assetPath, "asset", "utf8");
  const api = createGitHubApi({ hasRelease: false });

  const result = await publishReleaseDraft({
    repository: "appdev/QingYu",
    token: "test-token",
    releaseTag: "v2.4.0",
    releaseTarget: "new-target",
    releaseName: "QingYu v2.4.0",
    baselineNotes: "baseline notes\n",
    prerelease: false,
    allowedStaleDraftId: null,
    assetPaths: [assetPath],
    fetchImpl: api.fetchImpl,
  });

  assert.equal(result.releaseId, 123);
  assert.ok(
    api.requests.some(
      (request) => request.method === "POST" && request.pathname.endsWith("/releases"),
    ),
  );
  assert.ok(
    api.requests.some(
      (request) => request.method === "POST" && request.pathname.endsWith("/releases/123/assets"),
    ),
  );
});

test("draft publisher rejects mismatched asset digests and a draft published during upload", async (context) => {
  const temporaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-draft-publish-"));
  context.after(() => fs.rmSync(temporaryDirectory, { recursive: true, force: true }));
  const assetPath = path.join(temporaryDirectory, "QingYu_test.zip");
  fs.writeFileSync(assetPath, "asset", "utf8");
  const options = {
    repository: "appdev/QingYu",
    token: "test-token",
    releaseTag: "v2.4.0",
    releaseTarget: "new-target",
    releaseName: "QingYu v2.4.0",
    baselineNotes: "baseline notes\n",
    prerelease: false,
    allowedStaleDraftId: "123",
    assetPaths: [assetPath],
  };

  await assert.rejects(
    () => publishReleaseDraft({ ...options, fetchImpl: createGitHubApi({ tamperDigest: true }).fetchImpl }),
    /digest/u,
  );
  await assert.rejects(
    () =>
      publishReleaseDraft({
        ...options,
        fetchImpl: createGitHubApi({ publishAfterUpload: true }).fetchImpl,
      }),
    /requested draft state/u,
  );
});
