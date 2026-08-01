import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

function requireEnv(env, name) {
  const value = env[name]?.trim();
  if (!value) {
    throw new Error(`${name} is required.`);
  }
  return value;
}

export function validateDraftWrite(
  releases,
  { releaseTag, releaseTarget, baselineNotes, allowedStaleDraftId },
) {
  if (!Array.isArray(releases)) {
    throw new Error("GitHub Releases API must return an array.");
  }
  const matches = releases.filter((release) => release?.tag_name === releaseTag);
  if (matches.length > 1) {
    throw new Error(`Found multiple releases for ${releaseTag}; refusing to choose one.`);
  }
  if (matches.length === 0) {
    return { mode: "create", releaseId: null };
  }

  const [release] = matches;
  if (!release.draft) {
    throw new Error(`Release ${releaseTag} is already published; refusing to modify it.`);
  }
  if (release.target_commitish !== releaseTarget) {
    if (String(release.id) !== allowedStaleDraftId) {
      throw new Error(
        `Release ${releaseTag} targets ${release.target_commitish}; pass its explicit stale draft ID to replace it.`,
      );
    }
    return { mode: "replace-stale-draft", releaseId: release.id };
  }
  if (release.body !== baselineNotes) {
    throw new Error(`Release ${releaseTag} contains a customized draft body; refusing to overwrite it.`);
  }
  return { mode: "refresh-baseline-draft", releaseId: release.id };
}

async function fetchReleases({ repository, token, fetchImpl = fetch }) {
  const releases = [];
  for (let page = 1; ; page += 1) {
    const response = await fetchImpl(
      `https://api.github.com/repos/${repository}/releases?per_page=100&page=${page}`,
      {
        headers: {
          Accept: "application/vnd.github+json",
          Authorization: `Bearer ${token}`,
          "X-GitHub-Api-Version": "2022-11-28",
        },
      },
    );
    if (!response.ok) {
      throw new Error(`GitHub Releases API returned ${response.status} ${response.statusText}.`);
    }
    const pageReleases = await response.json();
    if (!Array.isArray(pageReleases)) {
      throw new Error("GitHub Releases API returned an unexpected response.");
    }
    releases.push(...pageReleases);
    if (pageReleases.length < 100) {
      return releases;
    }
  }
}

async function main(env = process.env) {
  const repository = requireEnv(env, "GITHUB_REPOSITORY");
  const token = requireEnv(env, "GITHUB_TOKEN");
  const releaseTag = requireEnv(env, "RELEASE_TAG");
  const releaseTarget = requireEnv(env, "RELEASE_TARGET");
  const notesPath = path.resolve(requireEnv(env, "RELEASE_NOTES_PATH"));
  const allowedStaleDraftId = env.ALLOWED_STALE_DRAFT_ID?.trim() || null;
  const result = validateDraftWrite(await fetchReleases({ repository, token }), {
    releaseTag,
    releaseTarget,
    baselineNotes: fs.readFileSync(notesPath, "utf8"),
    allowedStaleDraftId,
  });
  console.log(`Draft write guard: ${result.mode}${result.releaseId ? ` (${result.releaseId})` : ""}.`);
}

const isCli = process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;

if (isCli) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
