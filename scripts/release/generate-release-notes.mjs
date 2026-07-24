import { execFileSync, spawnSync } from "node:child_process";
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

function publishedAt(release) {
  return Date.parse(release.published_at || "") || 0;
}

export function selectPreviousRelease(releases, { currentTag, tagExists, isAncestor }) {
  const candidates = releases
    .filter((release) => !release.draft && release.published_at && release.tag_name !== currentTag)
    .sort((left, right) => publishedAt(right) - publishedAt(left));

  for (const release of candidates) {
    if (tagExists(release.tag_name) && isAncestor(release.tag_name)) {
      return release;
    }
  }

  return null;
}

export function parseGitLog(output) {
  return output
    .split("\u001e")
    .map((record) => record.trim())
    .filter(Boolean)
    .map((record) => {
      const [sha, shortSha, subject, author] = record.split("\u001f");

      if (!sha || !shortSha || !subject || !author) {
        throw new Error(`Unable to parse git log record: ${JSON.stringify(record)}`);
      }

      return { sha, shortSha, subject, author };
    });
}

export function renderReleaseNotes({ currentTag, previousTag, commits }) {
  const lines = ["\`${previousTag}..${currentTag}\`：", ""];


  if (commits.length > 0) {
    for (const commit of commits) {
      lines.push(`- \`${commit.shortSha}\` ${commit.subject} — ${commit.author}`);
    }
  } else {
    lines.push("- 当前版本尚无可列出的提交");
  }

  return lines.join("\n");
}

async function fetchPublishedReleases({ repository, token, fetchImpl = fetch }) {
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

function runGit(args) {
  return execFileSync("git", args, { encoding: "utf8" });
}

function gitTagExists(tag) {
  const result = spawnSync("git", ["cat-file", "-e", `${tag}^{commit}`], { stdio: "ignore" });
  return result.status === 0;
}

function gitTagIsAncestor(tag, target) {
  const result = spawnSync("git", ["merge-base", "--is-ancestor", `${tag}^{commit}`, target], {
    stdio: "ignore",
  });
  return result.status === 0;
}

async function main(env = process.env) {
  const repository = requireEnv(env, "GITHUB_REPOSITORY");
  const token = requireEnv(env, "GITHUB_TOKEN");
  const currentTag = requireEnv(env, "RELEASE_TAG");
  const releaseTarget = requireEnv(env, "RELEASE_TARGET");
  const outputPath = path.resolve(env.RELEASE_NOTES_PATH?.trim() || "release-notes.md");

  runGit(["rev-parse", "--verify", `${releaseTarget}^{commit}`]);

  const releases = await fetchPublishedReleases({ repository, token });
  const previousRelease = selectPreviousRelease(releases, {
    currentTag,
    tagExists: gitTagExists,
    isAncestor: (tag) => gitTagIsAncestor(tag, releaseTarget),
  });
  const previousTag = previousRelease?.tag_name || null;
  const range = previousTag ? `${previousTag}^{commit}..${releaseTarget}^{commit}` : `${releaseTarget}^{commit}`;
  const rawLog = runGit(["log", "--reverse", "--format=%H%x1f%h%x1f%s%x1f%an%x1e", range]);
  const commits = parseGitLog(rawLog);

  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, renderReleaseNotes({ currentTag, previousTag, commits }), "utf8");
}

const isCli = process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;

if (isCli) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
