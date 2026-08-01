import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";

const FIELD_SEPARATOR = "\u001f";
const RECORD_SEPARATOR = "\u001e";
const MAINTENANCE_TYPES = new Set(["build", "chore", "ci", "docs", "style", "test"]);
const MAX_MODEL_INPUT_CHARS = 48_000;
const MAX_MODEL_BODY_CHARS = 1_000;
const MAX_MODEL_PATHS = 20;

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

function requireFactString(value, field, { allowNull = false, allowEmpty = false } = {}) {
  if (allowNull && value === null) {
    return null;
  }
  if (typeof value !== "string" || (!allowEmpty && !value)) {
    throw new Error(`Release facts field ${field} must be ${allowEmpty ? "a string" : "a non-empty string"}.`);
  }
  if (value !== normalizeText(value)) {
    throw new Error(`Release facts field ${field} must already be normalized.`);
  }
  return value;
}

export function validateReleaseFacts(
  value,
  {
    repository: expectedRepository,
    currentTag: expectedCurrentTag,
    previousTag: expectedPreviousTag,
    releaseTarget: expectedTarget,
    signedRelease: expectedSignedRelease,
  },
) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Release facts must be a JSON object.");
  }
  if (value.schemaVersion !== 1) {
    throw new Error("Release facts schemaVersion must be 1.");
  }

  const repository = requireFactString(value.repository, "repository");
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/u.test(repository) || repository !== expectedRepository) {
    throw new Error(`Release facts repository must equal ${expectedRepository}.`);
  }
  const currentTag = requireFactString(value.currentTag, "currentTag");
  if (currentTag !== expectedCurrentTag) {
    throw new Error(`Release facts currentTag must equal ${expectedCurrentTag}.`);
  }
  const previousTag = requireFactString(value.previousTag, "previousTag", { allowNull: true });
  if (previousTag !== expectedPreviousTag) {
    throw new Error(`Release facts previousTag must equal ${expectedPreviousTag ?? "null"}.`);
  }
  const releaseTarget = requireFactString(value.releaseTarget, "releaseTarget");
  if (!/^[0-9a-f]{40}$/u.test(releaseTarget) || releaseTarget !== expectedTarget) {
    throw new Error(`Release facts releaseTarget must equal ${expectedTarget} as a full commit SHA.`);
  }
  if (typeof value.signedRelease !== "boolean") {
    throw new Error("Release facts signedRelease must be a boolean.");
  }
  if (value.signedRelease !== expectedSignedRelease) {
    throw new Error(`Release facts signedRelease must equal ${expectedSignedRelease}.`);
  }

  const expectedCompareUrl = buildCompareUrl(repository, previousTag, currentTag);
  if (value.compareUrl !== expectedCompareUrl) {
    throw new Error(`Release facts compareUrl must equal ${expectedCompareUrl}.`);
  }
  if (!Array.isArray(value.commits)) {
    throw new Error("Release facts commits must be an array.");
  }

  const fullShas = new Set();
  const shortShas = new Set();
  const allReferences = new Set();
  for (const [index, commit] of value.commits.entries()) {
    const field = `commits[${index}]`;
    if (!commit || typeof commit !== "object" || Array.isArray(commit)) {
      throw new Error(`Release facts field ${field} must be an object.`);
    }
    const sha = requireFactString(commit.sha, `${field}.sha`);
    const shortSha = requireFactString(commit.shortSha, `${field}.shortSha`);
    if (!/^[0-9a-f]{40}$/u.test(sha)) {
      throw new Error(`Release facts commit SHA ${sha} must contain 40 lowercase hexadecimal characters.`);
    }
    if (!/^[0-9a-f]{7,40}$/u.test(shortSha) || !sha.startsWith(shortSha)) {
      throw new Error(`Release facts short SHA ${shortSha} must be a lowercase prefix of ${sha}.`);
    }
    if (fullShas.has(sha)) {
      throw new Error(`Release facts commit SHA ${sha} must be unique.`);
    }
    if (shortShas.has(shortSha)) {
      throw new Error(`Release facts short SHA ${shortSha} must be unique.`);
    }
    if (allReferences.has(sha) || allReferences.has(shortSha)) {
      throw new Error(`Release facts SHA reference ${shortSha} must be unique.`);
    }
    fullShas.add(sha);
    shortShas.add(shortSha);
    allReferences.add(sha);
    allReferences.add(shortSha);

    if (commit.type !== null) {
      requireFactString(commit.type, `${field}.type`);
    }
    if (commit.scope !== null) {
      requireFactString(commit.scope, `${field}.scope`);
    }
    if (typeof commit.breaking !== "boolean") {
      throw new Error(`Release facts field ${field}.breaking must be a boolean.`);
    }
    requireFactString(commit.subject, `${field}.subject`);
    requireFactString(commit.description, `${field}.description`);
    requireFactString(commit.body, `${field}.body`, { allowEmpty: true });
    requireFactString(commit.author, `${field}.author`);
    if (!Array.isArray(commit.changedPaths)) {
      throw new Error(`Release facts field ${field}.changedPaths must be an array.`);
    }
    for (const [pathIndex, changedPath] of commit.changedPaths.entries()) {
      requireFactString(changedPath, `${field}.changedPaths[${pathIndex}]`);
    }
    for (const numericField of ["insertions", "deletions"]) {
      if (!Number.isInteger(commit[numericField]) || commit[numericField] < 0) {
        throw new Error(`Release facts field ${field}.${numericField} must be a non-negative integer.`);
      }
    }
  }

  for (const shortSha of shortShas) {
    const matchingFullShas = [...fullShas].filter((sha) => sha.startsWith(shortSha));
    if (matchingFullShas.length !== 1) {
      throw new Error(`Release facts short SHA ${shortSha} must be an unambiguous commit prefix.`);
    }
  }

  return value;
}

function truncateText(value, maximumLength) {
  const normalized = normalizeText(value);
  if (normalized.length <= maximumLength) {
    return normalized;
  }
  return `${normalized.slice(0, Math.max(0, maximumLength - 1))}…`;
}

export function buildModelInput(facts) {
  const input = {
    schemaVersion: facts.schemaVersion,
    repository: facts.repository,
    currentTag: facts.currentTag,
    previousTag: facts.previousTag,
    signedRelease: facts.signedRelease,
    commits: [],
    omittedCommitCount: 0,
  };

  for (const commit of facts.commits) {
    const modelCommit = {
      sha: commit.sha,
      shortSha: commit.shortSha,
      type: commit.type,
      scope: commit.scope,
      breaking: commit.breaking,
      description: truncateText(commit.description, 300),
      body: truncateText(commit.body, MAX_MODEL_BODY_CHARS),
      changedPaths: commit.changedPaths.slice(0, MAX_MODEL_PATHS),
      insertions: commit.insertions,
      deletions: commit.deletions,
    };
    const candidate = { ...input, commits: [...input.commits, modelCommit] };

    if (JSON.stringify(candidate).length > MAX_MODEL_INPUT_CHARS) {
      input.omittedCommitCount += 1;
      continue;
    }

    input.commits.push(modelCommit);
  }

  return input;
}

function requireModelText(value, field, { allowNull = false, maximumLength = 1_000 } = {}) {
  if (allowNull && value === null) {
    return null;
  }
  if (typeof value !== "string" || !value.trim()) {
    throw new Error(`Model field ${field} must be a non-empty string.`);
  }

  const normalized = normalizeText(value);
  if (normalized.length > maximumLength) {
    throw new Error(`Model field ${field} exceeds ${maximumLength} characters.`);
  }
  if (/\$\{[^}]+\}|\{\{[^}]+\}\}|<[^>]+>/u.test(normalized)) {
    throw new Error(`Model field ${field} contains a placeholder.`);
  }

  return normalized;
}

function assertClaimsSupported(text, facts, field) {
  const source = facts.commits
    .flatMap((commit) => [commit.subject, commit.body, ...commit.changedPaths])
    .join(" ")
    .toLowerCase();
  const normalized = text.toLowerCase();
  const guardedClaims = [
    ["windows", ["windows"]],
    ["macos", ["macos"]],
    ["linux", ["linux"]],
    ["android", ["android"]],
    ["ios", ["ios"]],
    ["签名", ["签名", "signed", "signing"]],
    ["自动更新", ["自动更新", "auto update", "updater"]],
    ["安全", ["安全", "security", "secure"]],
    ["漏洞", ["漏洞", "vulnerability", "cve"]],
    ["迁移", ["迁移", "migrate", "migration"]],
    ["cve", ["cve"]],
  ];

  if (
    /!?\[[^\]\n]*\]\s*\([^\n)]*\)/u.test(normalized) ||
    /[A-Za-z][A-Za-z0-9+.-]*:/u.test(normalized) ||
    /\/\/[A-Za-z0-9]/u.test(normalized) ||
    /\bwww\.[A-Za-z0-9]/iu.test(normalized) ||
    /\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/iu.test(normalized)
  ) {
    throw new Error(`Model field ${field} contains an unsupported claim (URL).`);
  }

  const allowedVersions = new Set([facts.currentTag, facts.previousTag].filter(Boolean));
  for (const version of normalized.match(/v?\d+\.\d+\.\d+(?:-[0-9a-z.-]+)?/giu) || []) {
    if (![...allowedVersions].some((allowed) => allowed.toLowerCase() === version.toLowerCase())) {
      throw new Error(`Model field ${field} contains an unsupported claim (${version}).`);
    }
  }

  for (const [claim, sourceTerms] of guardedClaims) {
    if (normalized.includes(claim) && !sourceTerms.some((term) => source.includes(term))) {
      throw new Error(`Model field ${field} contains an unsupported claim (${claim}).`);
    }
  }
}

function validateModelItem(value, facts, field, knownShas) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Model field ${field} must be an object.`);
  }
  if (!Array.isArray(value.commitShas) || value.commitShas.length === 0) {
    throw new Error(`Model field ${field}.commitShas must contain at least one commit SHA.`);
  }
  const commitShas = value.commitShas.map((sha, index) => {
    const normalized = requireModelText(sha, `${field}.commitShas[${index}]`, {
      maximumLength: 64,
    });
    if (!knownShas.has(normalized)) {
      throw new Error(`Model field ${field} references unknown commit ${normalized}.`);
    }
    return normalized;
  });

  const referencedShas = new Set(commitShas);
  const referencedFacts = {
    ...facts,
    commits: facts.commits.filter(
      (commit) => referencedShas.has(commit.sha) || referencedShas.has(commit.shortSha),
    ),
  };
  const text = requireModelText(value.text, `${field}.text`, { maximumLength: 500 });
  assertClaimsSupported(text, referencedFacts, `${field}.text`);

  return { text, commitShas: [...new Set(commitShas)] };
}

export function validateModelSummary(value, facts) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Model response must be a JSON object.");
  }

  const knownShas = new Set(facts.commits.flatMap((commit) => [commit.sha, commit.shortSha]));
  const summary = requireModelText(value.summary, "summary", { maximumLength: 600 });
  assertClaimsSupported(summary, facts, "summary");

  if (!Array.isArray(value.sections) || value.sections.length < 2 || value.sections.length > 5) {
    throw new Error("Model field sections must contain between 2 and 5 sections.");
  }
  const sections = value.sections.map((section, sectionIndex) => {
    if (!section || typeof section !== "object" || Array.isArray(section)) {
      throw new Error(`Model section ${sectionIndex} must be an object.`);
    }
    const title = requireModelText(section.title, `sections[${sectionIndex}].title`, {
      maximumLength: 80,
    });
    assertClaimsSupported(title, facts, `sections[${sectionIndex}].title`);
    if (!Array.isArray(section.items) || section.items.length === 0 || section.items.length > 8) {
      throw new Error(`Model section ${sectionIndex} must contain between 1 and 8 items.`);
    }
    return {
      title,
      items: section.items.map((item, itemIndex) =>
        validateModelItem(item, facts, `sections[${sectionIndex}].items[${itemIndex}]`, knownShas),
      ),
    };
  });

  const notice = requireModelText(value.notice, "notice", {
    allowNull: true,
    maximumLength: 600,
  });
  if (notice) {
    assertClaimsSupported(notice, facts, "notice");
  }

  if (!Array.isArray(value.otherChanges) || value.otherChanges.length > 10) {
    throw new Error("Model field otherChanges must be an array with at most 10 items.");
  }
  const otherChanges = value.otherChanges.map((item, itemIndex) =>
    validateModelItem(item, facts, `otherChanges[${itemIndex}]`, knownShas),
  );

  return { summary, sections, notice, otherChanges };
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

function releaseFooter(facts) {
  const comparisonLabel = facts.previousTag
    ? `查看 ${facts.previousTag} 到 ${facts.currentTag} 的完整变更`
    : `查看 ${facts.currentTag} 的完整提交历史`;
  const disclosure = facts.signedRelease
    ? "> 本次包含签名与自动更新元数据。"
    : "> 本次为未签名构建；不会发布自动更新元数据，macOS 首次打开时可能需要手动确认。";
  return [`[${comparisonLabel}](${facts.compareUrl})`, "", disclosure];
}

function renderModelItem(item) {
  return `- ${item.text} (${item.commitShas.map((sha) => `\`${sha}\``).join(", ")})`;
}

export function renderModelReleaseNotes(summary, facts) {
  const lines = [summary.summary, ""];

  for (const section of summary.sections) {
    lines.push(`## ${section.title}`, "", ...section.items.map(renderModelItem), "");
  }
  if (summary.notice) {
    lines.push("## 升级与兼容性", "", summary.notice, "");
  }
  if (summary.otherChanges.length > 0) {
    lines.push("## 其他变更", "", ...summary.otherChanges.map(renderModelItem), "");
  }
  lines.push(...releaseFooter(facts));

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

function assignStableShortShas(commits) {
  return commits.map((commit) => {
    let length = Math.min(8, commit.sha.length);
    while (
      length < commit.sha.length &&
      commits.some(
        (candidate) => candidate.sha !== commit.sha && candidate.sha.startsWith(commit.sha.slice(0, length)),
      )
    ) {
      length += 1;
    }
    return { ...commit, shortSha: commit.sha.slice(0, length) };
  });
}

function collectCommits(range, runGitImpl = runGit) {
  const rawLog = runGitImpl([
    "log",
    "--no-merges",
    "--reverse",
    `--format=%H%x1f%H%x1f%s%x1f%b%x1f%an%x1e`,
    range,
  ]);

  return assignStableShortShas(parseGitLog(rawLog)).map((commit) => ({
    ...commit,
    ...parseNumStat(
      runGitImpl([
        "-c",
        "core.quotePath=false",
        "diff-tree",
        "--no-commit-id",
        "--numstat",
        "-r",
        "--root",
        commit.sha,
      ]),
    ),
  }));
}

export function validateReleaseFactsProvenance(facts, { runGitImpl = runGit } = {}) {
  const resolvedTarget = runGitImpl([
    "rev-parse",
    "--verify",
    `${facts.releaseTarget}^{commit}`,
  ]).trim();
  if (resolvedTarget !== facts.releaseTarget) {
    throw new Error(`Release facts target ${facts.releaseTarget} does not resolve to that commit.`);
  }

  if (facts.previousTag) {
    runGitImpl(["rev-parse", "--verify", `${facts.previousTag}^{commit}`]);
    runGitImpl(["merge-base", "--is-ancestor", `${facts.previousTag}^{commit}`, facts.releaseTarget]);
  }

  const range = facts.previousTag
    ? `${facts.previousTag}^{commit}..${facts.releaseTarget}^{commit}`
    : `${facts.releaseTarget}^{commit}`;
  const expectedFacts = buildReleaseFacts({
    repository: facts.repository,
    currentTag: facts.currentTag,
    previousTag: facts.previousTag,
    releaseTarget: facts.releaseTarget,
    signedRelease: facts.signedRelease,
    commits: collectCommits(range, runGitImpl),
  });
  if (!isDeepStrictEqual(facts, expectedFacts)) {
    throw new Error("Release facts do not match the local Git range.");
  }
  return facts;
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
