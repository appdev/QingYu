import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const FIELD_SEPARATOR = "\u001f";
const RECORD_SEPARATOR = "\u001e";
const MAINTENANCE_TYPES = new Set(["build", "chore", "ci", "docs", "style", "test"]);

function requireEnv(env, name) {
  const value = env[name]?.trim();

  if (!value) {
    throw new Error(`${name} is required.`);
  }

  return value;
}

function parseBoolean(value) {
  return value?.trim().toLowerCase() === "true";
}

function publishedAt(release) {
  return Date.parse(release.published_at || "") || 0;
}

function normalizeText(value) {
  return String(value ?? "")
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/gu, "")
    .replace(/[\t\r\n ]+/gu, " ")
    .trim();
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
    .split(RECORD_SEPARATOR)
    .map((record) => record.replace(/^\s+|\s+$/gu, ""))
    .filter(Boolean)
    .map((record) => {
      const fields = record.split(FIELD_SEPARATOR);
      const [sha, shortSha, subject] = fields;
      const body = fields.length >= 5 ? fields[3] : "";
      const author = fields.length >= 5 ? fields[4] : fields[3];

      if (!sha || !shortSha || !subject || !author) {
        throw new Error(`Unable to parse git log record: ${JSON.stringify(record)}`);
      }

      return { sha, shortSha, subject, body, author };
    });
}

export function parseNumStat(output) {
  const result = {
    changedPaths: [],
    insertions: 0,
    deletions: 0,
  };

  for (const line of output.split(/\r?\n/u)) {
    if (!line) {
      continue;
    }

    const [insertions, deletions, ...pathParts] = line.split("\t");
    const changedPath = normalizeText(pathParts.join("\t"));

    if (!changedPath) {
      continue;
    }

    result.changedPaths.push(changedPath);
    if (/^\d+$/u.test(insertions)) {
      result.insertions += Number(insertions);
    }
    if (/^\d+$/u.test(deletions)) {
      result.deletions += Number(deletions);
    }
  }

  return result;
}

export function parseConventionalSubject(subject) {
  const normalized = normalizeText(subject);
  const match = /^(?<type>[a-z][a-z0-9-]*)(?:\((?<scope>[^)]+)\))?(?<breaking>!)?:\s+(?<description>.+)$/iu.exec(
    normalized,
  );

  if (!match?.groups) {
    return {
      type: null,
      scope: null,
      breaking: false,
      description: normalized,
    };
  }

  return {
    type: match.groups.type.toLowerCase(),
    scope: match.groups.scope ? normalizeText(match.groups.scope) : null,
    breaking: Boolean(match.groups.breaking),
    description: normalizeText(match.groups.description),
  };
}

export function buildCompareUrl(repository, previousTag, currentTag) {
  const encodedRepository = repository
    .split("/")
    .map((part) => encodeURIComponent(part))
    .join("/");
  const encodedCurrentTag = encodeURIComponent(currentTag);

  if (!previousTag) {
    return `https://github.com/${encodedRepository}/commits/${encodedCurrentTag}`;
  }

  return `https://github.com/${encodedRepository}/compare/${encodeURIComponent(previousTag)}...${encodedCurrentTag}`;
}

export function buildReleaseFacts({
  repository,
  currentTag,
  previousTag,
  releaseTarget,
  signedRelease,
  commits,
}) {
  return {
    schemaVersion: 1,
    repository,
    currentTag,
    previousTag,
    releaseTarget,
    compareUrl: buildCompareUrl(repository, previousTag, currentTag),
    signedRelease: Boolean(signedRelease),
    commits: commits.map((commit) => {
      const conventional = parseConventionalSubject(commit.subject);

      return {
        sha: normalizeText(commit.sha),
        shortSha: normalizeText(commit.shortSha),
        type: conventional.type,
        scope: conventional.scope,
        breaking: conventional.breaking,
        subject: normalizeText(commit.subject),
        description: conventional.description,
        body: normalizeText(commit.body),
        author: normalizeText(commit.author),
        changedPaths: (commit.changedPaths || []).map(normalizeText).filter(Boolean),
        insertions: Number.isFinite(commit.insertions) ? commit.insertions : 0,
        deletions: Number.isFinite(commit.deletions) ? commit.deletions : 0,
      };
    }),
  };
}

function releaseSection(commit) {
  if (MAINTENANCE_TYPES.has(commit.type)) {
    return null;
  }
  if (commit.type === "feat") {
    return "功能改进";
  }
  if (commit.type === "fix" || commit.type === "revert") {
    return "问题修复";
  }
  if (commit.type === "perf") {
    return "性能优化";
  }
  if (commit.breaking || commit.type === "refactor") {
    return "行为与兼容性";
  }
  return "其他变更";
}

export function renderDeterministicReleaseNotes(facts) {
  const sections = new Map([
    ["功能改进", []],
    ["问题修复", []],
    ["性能优化", []],
    ["行为与兼容性", []],
    ["其他变更", []],
  ]);

  for (const commit of facts.commits) {
    const section = releaseSection(commit);
    if (section) {
      sections.get(section).push(commit);
    }
  }

  const lines = [];
  if (!facts.previousTag) {
    lines.push("这是当前提交历史中可追溯的首个 Release。", "");
  }

  let renderedCommit = false;
  for (const [heading, commits] of sections) {
    if (commits.length === 0) {
      continue;
    }

    renderedCommit = true;
    lines.push(`## ${heading}`, "");
    for (const commit of commits) {
      lines.push(`- ${commit.description} (\`${commit.shortSha}\`)`);
    }
    lines.push("");
  }

  if (!renderedCommit) {
    lines.push("## 其他变更", "", "- 当前版本尚无可列出的提交", "");
  }

  const comparisonLabel = facts.previousTag
    ? `查看 ${facts.previousTag} 到 ${facts.currentTag} 的完整变更`
    : `查看 ${facts.currentTag} 的完整提交历史`;
  lines.push(`[${comparisonLabel}](${facts.compareUrl})`, "");
  lines.push(
    facts.signedRelease
      ? "> 本次包含签名与自动更新元数据。"
      : "> 本次为未签名构建；不会发布自动更新元数据，macOS 首次打开时可能需要手动确认。",
  );

  return `${lines.join("\n").trim()}\n`;
}

export function renderReleaseNotes({
  repository = "appdev/QingYu",
  currentTag,
  previousTag,
  releaseTarget = currentTag,
  signedRelease = false,
  commits,
}) {
  return renderDeterministicReleaseNotes(
    buildReleaseFacts({
      repository,
      currentTag,
      previousTag,
      releaseTarget,
      signedRelease,
      commits,
    }),
  );
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

function collectCommits(range) {
  const rawLog = runGit([
    "log",
    "--no-merges",
    "--reverse",
    `--format=%H%x1f%h%x1f%s%x1f%b%x1f%an%x1e`,
    range,
  ]);

  return parseGitLog(rawLog).map((commit) => ({
    ...commit,
    ...parseNumStat(runGit(["diff-tree", "--no-commit-id", "--numstat", "-r", "--root", commit.sha])),
  }));
}

async function main(env = process.env) {
  const repository = requireEnv(env, "GITHUB_REPOSITORY");
  const token = requireEnv(env, "GITHUB_TOKEN");
  const currentTag = requireEnv(env, "RELEASE_TAG");
  const releaseTarget = requireEnv(env, "RELEASE_TARGET");
  const outputPath = path.resolve(env.RELEASE_NOTES_PATH?.trim() || "release-notes.md");
  const factsPath = path.resolve(env.RELEASE_FACTS_PATH?.trim() || "release-facts.json");

  runGit(["rev-parse", "--verify", `${releaseTarget}^{commit}`]);

  const releases = await fetchPublishedReleases({ repository, token });
  const previousRelease = selectPreviousRelease(releases, {
    currentTag,
    tagExists: gitTagExists,
    isAncestor: (tag) => gitTagIsAncestor(tag, releaseTarget),
  });
  const previousTag = previousRelease?.tag_name || null;
  const range = previousTag ? `${previousTag}^{commit}..${releaseTarget}^{commit}` : `${releaseTarget}^{commit}`;
  const facts = buildReleaseFacts({
    repository,
    currentTag,
    previousTag,
    releaseTarget,
    signedRelease: parseBoolean(env.SIGNED_RELEASE),
    commits: collectCommits(range),
  });

  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.mkdirSync(path.dirname(factsPath), { recursive: true });
  fs.writeFileSync(factsPath, `${JSON.stringify(facts, null, 2)}\n`, "utf8");
  fs.writeFileSync(outputPath, renderDeterministicReleaseNotes(facts), "utf8");
}

const isCli = process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;

if (isCli) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
