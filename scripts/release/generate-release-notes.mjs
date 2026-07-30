import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const FIELD_SEPARATOR = "\u001f";
const RECORD_SEPARATOR = "\u001e";
const MAINTENANCE_TYPES = new Set(["build", "chore", "ci", "docs", "style", "test"]);
const DEFAULT_MODEL = "openai/gpt-4.1";
const MAX_MODEL_INPUT_CHARS = 48_000;
const MAX_MODEL_BODY_CHARS = 1_000;
const MAX_MODEL_PATHS = 20;
const MODEL_SYSTEM_PROMPT = `你是 QingYu 桌面笔记应用的发布说明编辑。根据提供的确定性提交事实，以简体中文总结用户可感知的变化。
只陈述输入事实支持的内容；不要发明版本、平台、签名、安全、迁移或链接信息。把相关提交合并成主题，不要输出逐提交清单。
只返回 JSON：summary 为简短总述；sections 为 2 到 5 个主题，每项包含 title 和 items；每个 item 包含 text 与支持它的 commitShas；notice 仅在事实明确支持升级或兼容提醒时使用，否则为 null；otherChanges 为较小的用户可见变化。`;

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
    "windows",
    "macos",
    "linux",
    "android",
    "ios",
    "签名",
    "自动更新",
    "安全",
    "漏洞",
    "迁移",
    "cve",
  ];

  if (/https?:\/\//u.test(normalized)) {
    throw new Error(`Model field ${field} contains an unsupported claim (URL).`);
  }

  const allowedVersions = new Set([facts.currentTag, facts.previousTag].filter(Boolean));
  for (const version of normalized.match(/v?\d+\.\d+\.\d+(?:-[0-9a-z.-]+)?/giu) || []) {
    if (![...allowedVersions].some((allowed) => allowed.toLowerCase() === version.toLowerCase())) {
      throw new Error(`Model field ${field} contains an unsupported claim (${version}).`);
    }
  }

  for (const claim of guardedClaims) {
    if (normalized.includes(claim) && !source.includes(claim)) {
      throw new Error(`Model field ${field} contains an unsupported claim (${claim}).`);
    }
  }
}

function validateModelItem(value, facts, field, knownShas) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Model field ${field} must be an object.`);
  }
  const text = requireModelText(value.text, `${field}.text`, { maximumLength: 500 });
  assertClaimsSupported(text, facts, `${field}.text`);

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

export async function generateReleaseNotes({
  facts,
  model = DEFAULT_MODEL,
  modelClient,
  warn = (message) => console.warn(message),
}) {
  if (!modelClient) {
    return { notes: renderDeterministicReleaseNotes(facts), usedModel: false };
  }

  try {
    const modelValue = await modelClient({
      model,
      systemPrompt: MODEL_SYSTEM_PROMPT,
      userPrompt: JSON.stringify(buildModelInput(facts)),
    });
    const summary = validateModelSummary(modelValue, facts);
    return { notes: renderModelReleaseNotes(summary, facts), usedModel: true };
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    warn(`::warning::GitHub Models failed (${reason}); using deterministic release notes.`);
    return { notes: renderDeterministicReleaseNotes(facts), usedModel: false };
  }
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

function createGitHubModelsClient({ token, fetchImpl = fetch, timeoutMs = 30_000 }) {
  return async ({ model, systemPrompt, userPrompt }) => {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(new Error("GitHub Models request timed out.")), timeoutMs);

    try {
      const response = await fetchImpl("https://models.github.ai/inference/chat/completions", {
        method: "POST",
        headers: {
          Accept: "application/vnd.github+json",
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
          "X-GitHub-Api-Version": "2022-11-28",
        },
        body: JSON.stringify({
          model,
          temperature: 0.2,
          response_format: { type: "json_object" },
          messages: [
            { role: "system", content: systemPrompt },
            { role: "user", content: userPrompt },
          ],
        }),
        signal: controller.signal,
      });

      if (!response.ok) {
        throw new Error(`GitHub Models returned ${response.status} ${response.statusText}.`);
      }
      const payload = await response.json();
      const content = payload?.choices?.[0]?.message?.content;
      if (typeof content !== "string" || !content.trim()) {
        throw new Error("GitHub Models returned an empty response.");
      }
      return JSON.parse(content);
    } finally {
      clearTimeout(timeout);
    }
  };
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
  const generated = await generateReleaseNotes({
    facts,
    model: env.GITHUB_MODELS_MODEL?.trim() || DEFAULT_MODEL,
    modelClient: createGitHubModelsClient({ token }),
  });
  fs.writeFileSync(outputPath, generated.notes, "utf8");
}

const isCli = process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;

if (isCli) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
