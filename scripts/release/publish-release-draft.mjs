import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";

import { validateDraftWrite } from "./guard-release-draft-write.mjs";

function requireEnv(env, name) {
  const value = env[name]?.trim();
  if (!value) {
    throw new Error(`${name} is required.`);
  }
  return value;
}

function parseBoolean(value, name) {
  if (value === "true") {
    return true;
  }
  if (value === "false") {
    return false;
  }
  throw new Error(`${name} must be true or false.`);
}

function apiHeaders(token, extra = {}) {
  return {
    Accept: "application/vnd.github+json",
    Authorization: `Bearer ${token}`,
    "X-GitHub-Api-Version": "2022-11-28",
    ...extra,
  };
}

async function requestJson(fetchImpl, url, { token, expectedStatuses, ...options }) {
  const response = await fetchImpl(url, {
    ...options,
    headers: apiHeaders(token, options.headers),
  });
  const responseText = await response.text();
  if (!expectedStatuses.includes(response.status)) {
    throw new Error(
      `GitHub Releases API returned ${response.status} ${response.statusText}: ${responseText.slice(0, 500)}`,
    );
  }
  return responseText ? JSON.parse(responseText) : null;
}

async function fetchAll(fetchImpl, url, token) {
  const values = [];
  for (let page = 1; ; page += 1) {
    const separator = url.includes("?") ? "&" : "?";
    const pageValues = await requestJson(fetchImpl, `${url}${separator}per_page=100&page=${page}`, {
      token,
      expectedStatuses: [200],
    });
    if (!Array.isArray(pageValues)) {
      throw new Error("GitHub Releases API returned an unexpected paginated response.");
    }
    values.push(...pageValues);
    if (pageValues.length < 100) {
      return values;
    }
  }
}

function validateAssets(assetPaths) {
  if (!Array.isArray(assetPaths) || assetPaths.length === 0) {
    throw new Error("At least one release asset is required.");
  }
  const names = new Set();
  return assetPaths.map((assetPath) => {
    const resolvedPath = path.resolve(assetPath);
    const stats = fs.statSync(resolvedPath);
    if (!stats.isFile() || stats.size === 0) {
      throw new Error(`Release asset ${resolvedPath} must be a non-empty file.`);
    }
    const name = path.basename(resolvedPath);
    if (names.has(name)) {
      throw new Error(`Release asset name ${name} must be unique.`);
    }
    names.add(name);
    return { path: resolvedPath, name, size: stats.size };
  });
}

async function hashAsset(asset) {
  const hash = createHash("sha256");
  for await (const chunk of fs.createReadStream(asset.path)) {
    hash.update(chunk);
  }
  return { ...asset, digest: `sha256:${hash.digest("hex")}` };
}

function assertReleasePostcondition(release, expected) {
  if (
    release?.id !== expected.id ||
    release.tag_name !== expected.tagName ||
    release.target_commitish !== expected.target ||
    release.name !== expected.name ||
    release.body !== expected.body ||
    release.draft !== true ||
    release.prerelease !== expected.prerelease
  ) {
    throw new Error(`Release ${expected.id} does not match the requested draft state after mutation.`);
  }
}

export async function publishReleaseDraft({
  repository,
  token,
  releaseTag,
  releaseTarget,
  releaseName,
  baselineNotes,
  prerelease,
  allowedStaleDraftId,
  assetPaths,
  fetchImpl = fetch,
}) {
  const assets = await Promise.all(validateAssets(assetPaths).map(hashAsset));
  const apiBase = `https://api.github.com/repos/${repository}`;
  const releases = await fetchAll(fetchImpl, `${apiBase}/releases`, token);
  const write = validateDraftWrite(releases, {
    releaseTag,
    releaseTarget,
    baselineNotes,
    allowedStaleDraftId,
  });
  const requestedRelease = {
    tag_name: releaseTag,
    target_commitish: releaseTarget,
    name: releaseName,
    body: baselineNotes,
    draft: true,
    prerelease,
  };

  const release = write.releaseId === null
    ? await requestJson(fetchImpl, `${apiBase}/releases`, {
        token,
        expectedStatuses: [201],
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(requestedRelease),
      })
    : await requestJson(fetchImpl, `${apiBase}/releases/${write.releaseId}`, {
        token,
        expectedStatuses: [200],
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(requestedRelease),
      });

  if (!Number.isInteger(release?.id)) {
    throw new Error("GitHub Releases API did not return a numeric release ID.");
  }
  const releaseId = release.id;
  const confirmedRelease = await requestJson(fetchImpl, `${apiBase}/releases/${releaseId}`, {
    token,
    expectedStatuses: [200],
  });
  assertReleasePostcondition(confirmedRelease, {
    id: releaseId,
    tagName: releaseTag,
    target: releaseTarget,
    name: releaseName,
    body: baselineNotes,
    prerelease,
  });

  const existingAssets = await fetchAll(
    fetchImpl,
    `${apiBase}/releases/${releaseId}/assets`,
    token,
  );
  for (const existingAsset of existingAssets) {
    if (!Number.isInteger(existingAsset?.id)) {
      throw new Error(`Release ${releaseId} contains an asset without a numeric ID.`);
    }
    await requestJson(fetchImpl, `${apiBase}/releases/assets/${existingAsset.id}`, {
      token,
      expectedStatuses: [204],
      method: "DELETE",
    });
  }

  for (const asset of assets) {
    await requestJson(
      fetchImpl,
      `https://uploads.github.com/repos/${repository}/releases/${releaseId}/assets?name=${encodeURIComponent(asset.name)}`,
      {
        token,
        expectedStatuses: [201],
        method: "POST",
        headers: {
          "Content-Length": String(asset.size),
          "Content-Type": "application/octet-stream",
        },
        body: fs.createReadStream(asset.path),
        duplex: "half",
      },
    );
  }

  const publishedAssets = await fetchAll(
    fetchImpl,
    `${apiBase}/releases/${releaseId}/assets`,
    token,
  );
  if (publishedAssets.length !== assets.length) {
    throw new Error(`Release ${releaseId} does not contain the exact requested asset count.`);
  }
  const publishedByName = new Map(publishedAssets.map((asset) => [asset.name, asset]));
  if (publishedByName.size !== assets.length) {
    throw new Error(`Release ${releaseId} does not contain the exact requested asset set.`);
  }
  for (const asset of assets) {
    const publishedAsset = publishedByName.get(asset.name);
    if (publishedAsset?.state !== "uploaded") {
      throw new Error(`Release asset ${asset.name} is not in the uploaded state.`);
    }
    if (publishedAsset.size !== asset.size) {
      throw new Error(`Release asset ${asset.name} does not match the uploaded file size.`);
    }
    if (publishedAsset.digest !== asset.digest) {
      throw new Error(`Release asset ${asset.name} does not match the local SHA-256 digest.`);
    }
  }

  const finalRelease = await requestJson(fetchImpl, `${apiBase}/releases/${releaseId}`, {
    token,
    expectedStatuses: [200],
  });
  assertReleasePostcondition(finalRelease, {
    id: releaseId,
    tagName: releaseTag,
    target: releaseTarget,
    name: releaseName,
    body: baselineNotes,
    prerelease,
  });

  return { releaseId, uploadedAssets: assets.map((asset) => asset.name) };
}

async function main(env = process.env) {
  const filesPath = path.resolve(requireEnv(env, "RELEASE_FILES_PATH"));
  const assetPaths = fs
    .readFileSync(filesPath, "utf8")
    .split(/\r?\n/u)
    .map((value) => value.trim())
    .filter(Boolean);
  const result = await publishReleaseDraft({
    repository: requireEnv(env, "GITHUB_REPOSITORY"),
    token: requireEnv(env, "GITHUB_TOKEN"),
    releaseTag: requireEnv(env, "RELEASE_TAG"),
    releaseTarget: requireEnv(env, "RELEASE_TARGET"),
    releaseName: requireEnv(env, "RELEASE_NAME"),
    baselineNotes: fs.readFileSync(path.resolve(requireEnv(env, "RELEASE_NOTES_PATH")), "utf8"),
    prerelease: parseBoolean(requireEnv(env, "RELEASE_PRERELEASE"), "RELEASE_PRERELEASE"),
    allowedStaleDraftId: env.ALLOWED_STALE_DRAFT_ID?.trim() || null,
    assetPaths,
  });
  console.log(
    `Published draft release ${result.releaseId} with ${result.uploadedAssets.length} ID-bound assets.`,
  );
}

const isCli = process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;

if (isCli) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
