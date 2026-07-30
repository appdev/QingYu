import assert from "node:assert/strict";
import test from "node:test";

const releaseNotesModule = await import("./generate-release-notes.mjs").catch(() => ({}));
const {
  buildCompareUrl,
  buildModelInput,
  buildReleaseFacts,
  generateReleaseNotes,
  parseConventionalSubject,
  parseGitLog,
  parseNumStat,
  renderModelReleaseNotes,
  renderReleaseNotes,
  selectPreviousRelease,
  validateModelSummary,
} = releaseNotesModule;

test("release notes module exposes focused selection and rendering helpers", () => {
  assert.equal(typeof releaseNotesModule.selectPreviousRelease, "function");
  assert.equal(typeof releaseNotesModule.parseGitLog, "function");
  assert.equal(typeof releaseNotesModule.parseNumStat, "function");
  assert.equal(typeof releaseNotesModule.parseConventionalSubject, "function");
  assert.equal(typeof releaseNotesModule.buildReleaseFacts, "function");
  assert.equal(typeof releaseNotesModule.buildCompareUrl, "function");
  assert.equal(typeof releaseNotesModule.buildModelInput, "function");
  assert.equal(typeof releaseNotesModule.validateModelSummary, "function");
  assert.equal(typeof releaseNotesModule.renderModelReleaseNotes, "function");
  assert.equal(typeof releaseNotesModule.generateReleaseNotes, "function");
  assert.equal(typeof releaseNotesModule.renderReleaseNotes, "function");
});

test("selectPreviousRelease chooses the newest published ancestor", () => {
  const releases = [
    { tag_name: "v1.7.5", draft: false, published_at: "2026-07-22T12:00:00Z" },
    { tag_name: "v1.7.4-draft", draft: true, published_at: "2026-07-22T11:00:00Z" },
    { tag_name: "v9.0.0", draft: false, published_at: "2026-07-22T10:00:00Z" },
    { tag_name: "v1.7.4", draft: false, published_at: "2026-07-21T10:00:00Z" },
    { tag_name: "v1.7.3", draft: false, published_at: "2026-07-20T10:00:00Z" },
  ];

  const selected = selectPreviousRelease(releases, {
    currentTag: "v1.7.5",
    tagExists: (tag) => tag !== "v9.0.0",
    isAncestor: (tag) => tag === "v1.7.4" || tag === "v1.7.3",
  });

  assert.equal(selected?.tag_name, "v1.7.4");
});

test("selectPreviousRelease returns null when no published release is an ancestor", () => {
  const selected = selectPreviousRelease(
    [{ tag_name: "v2.0.0", draft: false, published_at: "2026-07-22T10:00:00Z" }],
    {
      currentTag: "v1.7.5",
      tagExists: () => true,
      isAncestor: () => false,
    },
  );

  assert.equal(selected, null);
});

test("parseGitLog converts record-delimited git output", () => {
  const commits = parseGitLog(
    "0123456789abcdef\u001f0123456\u001ffeat(editor): add mobile release\u001fExplain the change.\u001fQingYu\u001e" +
      "abcdef0123456789\u001fabcdef0\u001ffix: keep tags stable\u001f\u001fContributor\u001e\n",
  );

  assert.deepEqual(commits, [
    {
      sha: "0123456789abcdef",
      shortSha: "0123456",
      subject: "feat(editor): add mobile release",
      body: "Explain the change.",
      author: "QingYu",
    },
    {
      sha: "abcdef0123456789",
      shortSha: "abcdef0",
      subject: "fix: keep tags stable",
      body: "",
      author: "Contributor",
    },
  ]);
});

test("parseNumStat records paths and aggregate insertions and deletions", () => {
  assert.deepEqual(parseNumStat("12\t3\tpackages/app/src/App.tsx\n-\t-\tassets/icon.png\n"), {
    changedPaths: ["packages/app/src/App.tsx", "assets/icon.png"],
    insertions: 12,
    deletions: 3,
  });
});

test("parseConventionalSubject extracts type and optional scope", () => {
  assert.deepEqual(parseConventionalSubject("feat(editor)!: improve paste"), {
    type: "feat",
    scope: "editor",
    breaking: true,
    description: "improve paste",
  });
  assert.deepEqual(parseConventionalSubject("plain subject"), {
    type: null,
    scope: null,
    breaking: false,
    description: "plain subject",
  });
});

test("buildReleaseFacts normalizes controls while retaining complete commit metadata", () => {
  const facts = buildReleaseFacts({
    repository: "appdev/QingYu",
    currentTag: "v2.2.0",
    previousTag: "v2.1.0",
    releaseTarget: "target-sha",
    signedRelease: false,
    commits: [
      {
        sha: "0123456789abcdef",
        shortSha: "0123456",
        subject: "feat(editor): add\u0000 paste mode",
        body: "A user-facing\u001b improvement.",
        author: "Qing\u0007Yu",
        changedPaths: ["packages/editor/src/paste.ts"],
        insertions: 12,
        deletions: 3,
      },
      {
        sha: "abcdef0123456789",
        shortSha: "abcdef0",
        subject: "ci: update runner",
        body: "",
        author: "Contributor",
        changedPaths: [".github/workflows/ci.yml"],
        insertions: 1,
        deletions: 1,
      },
    ],
  });

  assert.equal(facts.commits.length, 2);
  assert.deepEqual(facts.commits[0], {
    sha: "0123456789abcdef",
    shortSha: "0123456",
    type: "feat",
    scope: "editor",
    breaking: false,
    subject: "feat(editor): add paste mode",
    description: "add paste mode",
    body: "A user-facing improvement.",
    author: "QingYu",
    changedPaths: ["packages/editor/src/paste.ts"],
    insertions: 12,
    deletions: 3,
  });
  assert.equal(facts.compareUrl, "https://github.com/appdev/QingYu/compare/v2.1.0...v2.2.0");
  assert.equal(facts.signedRelease, false);
});

test("buildCompareUrl uses the commit view when there is no previous Release", () => {
  assert.equal(
    buildCompareUrl("appdev/QingYu", null, "v2.2.0"),
    "https://github.com/appdev/QingYu/commits/v2.2.0",
  );
});

test("renderReleaseNotes lists commits without a static artifact inventory", () => {
  const notes = renderReleaseNotes({
    repository: "appdev/QingYu",
    currentTag: "v1.7.5",
    previousTag: "v1.7.4",
    signedRelease: false,
    commits: [
      {
        sha: "0123456789abcdef",
        shortSha: "0123456",
        subject: "feat: add mobile release",
        body: "",
        author: "QingYu",
        changedPaths: ["packages/app/src/mobile.ts"],
        insertions: 4,
        deletions: 1,
      },
    ],
  });

  assert.match(notes, /^## 功能改进$/mu);
  assert.match(notes, /- add mobile release \(`0123456`\)/u);
  assert.match(notes, /\[查看 v1\.7\.4 到 v1\.7\.5 的完整变更\]\(https:\/\/github\.com\/appdev\/QingYu\/compare\/v1\.7\.4\.\.\.v1\.7\.5\)/u);
  assert.match(notes, /本次为未签名构建/u);
  assert.doesNotMatch(notes, /移动端产物说明/u);
  assert.doesNotMatch(notes, /QingYu_1\.7\.5_/u);
});

test("renderReleaseNotes explains the first Release fallback", () => {
  const notes = renderReleaseNotes({
    repository: "appdev/QingYu",
    currentTag: "v1.7.5",
    previousTag: null,
    signedRelease: true,
    commits: [],
  });

  assert.match(notes, /首个 Release/u);
  assert.match(notes, /当前版本尚无可列出的提交/u);
  assert.match(notes, /本次包含签名与自动更新元数据/u);
});

test("renderReleaseNotes keeps maintenance commits out of product highlights", () => {
  const notes = renderReleaseNotes({
    repository: "appdev/QingYu",
    currentTag: "v2.2.0",
    previousTag: "v2.1.0",
    signedRelease: false,
    commits: [
      {
        sha: "0123456789abcdef",
        shortSha: "0123456",
        subject: "fix(sync): avoid duplicate upload",
        body: "",
        author: "QingYu",
        changedPaths: ["packages/app/src/sync.ts"],
        insertions: 4,
        deletions: 2,
      },
      {
        sha: "abcdef0123456789",
        shortSha: "abcdef0",
        subject: "test: cover duplicate upload",
        body: "",
        author: "QingYu",
        changedPaths: ["packages/app/src/sync.test.ts"],
        insertions: 10,
        deletions: 0,
      },
    ],
  });

  assert.match(notes, /^## 问题修复$/mu);
  assert.match(notes, /avoid duplicate upload/u);
  assert.doesNotMatch(notes, /cover duplicate upload/u);
});

function modelFacts() {
  return buildReleaseFacts({
    repository: "appdev/QingYu",
    currentTag: "v2.2.0",
    previousTag: "v2.1.0",
    releaseTarget: "target-sha",
    signedRelease: false,
    commits: [
      {
        sha: "0123456789abcdef",
        shortSha: "0123456",
        subject: "feat(editor): improve paste",
        body: "Keep Markdown paste predictable.",
        author: "QingYu",
        changedPaths: ["packages/editor/src/paste.ts"],
        insertions: 12,
        deletions: 3,
      },
      {
        sha: "abcdef0123456789",
        shortSha: "abcdef0",
        subject: "fix(sync): prevent duplicate upload",
        body: "Deduplicate remote write scheduling.",
        author: "Contributor",
        changedPaths: ["packages/app/src/sync.ts"],
        insertions: 7,
        deletions: 2,
      },
    ],
  });
}

function validModelSummary() {
  return {
    summary: "本次更新改善了编辑体验，并提升了同步可靠性。",
    sections: [
      {
        title: "更顺畅的编辑体验",
        items: [{ text: "粘贴 Markdown 时的行为更加稳定。", commitShas: ["0123456"] }],
      },
      {
        title: "更可靠的同步",
        items: [{ text: "避免同一内容被重复上传。", commitShas: ["abcdef0123456789"] }],
      },
    ],
    notice: null,
    otherChanges: [],
  };
}

test("buildModelInput bounds prose and paths without mutating deterministic facts", () => {
  const facts = modelFacts();
  facts.commits[0].body = "a".repeat(4_000);
  facts.commits[0].changedPaths = Array.from({ length: 40 }, (_, index) => `path/${index}.ts`);

  const input = buildModelInput(facts);

  assert.ok(input.commits[0].body.length <= 1_000);
  assert.equal(input.commits[0].changedPaths.length, 20);
  assert.equal(facts.commits[0].body.length, 4_000);
  assert.equal(facts.commits[0].changedPaths.length, 40);
  assert.ok(JSON.stringify(input).length <= 48_000);
  assert.doesNotMatch(JSON.stringify(input), /author/u);
});

test("validateModelSummary accepts supported structured output and renders auditable notes", () => {
  const facts = modelFacts();
  const summary = validateModelSummary(validModelSummary(), facts);
  const notes = renderModelReleaseNotes(summary, facts);

  assert.match(notes, /本次更新改善了编辑体验/u);
  assert.match(notes, /^## 更顺畅的编辑体验$/mu);
  assert.match(notes, /粘贴 Markdown 时的行为更加稳定。/u);
  assert.match(notes, /`0123456`/u);
  assert.match(notes, /compare\/v2\.1\.0\.\.\.v2\.2\.0/u);
  assert.match(notes, /本次为未签名构建/u);
});

test("validateModelSummary rejects unknown commits and unsupported claims", () => {
  const facts = modelFacts();
  const unknownCommit = validModelSummary();
  unknownCommit.sections[0].items[0].commitShas = ["deadbee"];
  assert.throws(() => validateModelSummary(unknownCommit, facts), /unknown commit/u);

  const inventedPlatform = validModelSummary();
  inventedPlatform.summary = "本次新增 Windows 自动更新与安全迁移。";
  assert.throws(() => validateModelSummary(inventedPlatform, facts), /unsupported claim/u);

  const placeholder = validModelSummary();
  placeholder.sections[0].items[0].text = "完成 ${currentTag} 发布";
  assert.throws(() => validateModelSummary(placeholder, facts), /placeholder/u);
});

test("validateModelSummary accepts guarded Chinese terms backed by English source facts", () => {
  const facts = modelFacts();
  facts.commits[0].changedPaths.push("packages/app/src/themes/migration.ts");
  const translatedClaim = validModelSummary();
  translatedClaim.sections[0].items[0].text = "迁移旧主题设置时保留现有选择。";

  assert.doesNotThrow(() => validateModelSummary(translatedClaim, facts));
});

test("generateReleaseNotes uses a valid injected model result", async () => {
  const calls = [];
  const result = await generateReleaseNotes({
    facts: modelFacts(),
    model: "openai/gpt-4.1",
    modelClient: async (request) => {
      calls.push(request);
      return validModelSummary();
    },
  });

  assert.equal(result.usedModel, true);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].model, "openai/gpt-4.1");
  assert.match(calls[0].systemPrompt, /简体中文/u);
  assert.match(result.notes, /更可靠的同步/u);
});

test("generateReleaseNotes repairs one unsupported model summary before falling back", async () => {
  const calls = [];
  const unsupported = validModelSummary();
  unsupported.sections[0].items[0].text = "自动完成安全迁移。";
  const result = await generateReleaseNotes({
    facts: modelFacts(),
    modelClient: async (request) => {
      calls.push(request);
      return calls.length === 1 ? unsupported : validModelSummary();
    },
  });

  assert.equal(result.usedModel, true);
  assert.equal(calls.length, 2);
  assert.match(calls[1].systemPrompt, /上一次输出未通过事实校验/u);
  assert.match(calls[1].systemPrompt, /unsupported claim/u);
  assert.match(result.notes, /更可靠的同步/u);
});

test("generateReleaseNotes warns and deterministically falls back for every model failure", async () => {
  for (const failure of [
    new Error("403 denied"),
    new Error("request timed out"),
    new SyntaxError("malformed JSON"),
  ]) {
    const warnings = [];
    const result = await generateReleaseNotes({
      facts: modelFacts(),
      modelClient: async () => {
        throw failure;
      },
      warn: (message) => warnings.push(message),
    });

    assert.equal(result.usedModel, false);
    assert.equal(warnings.length, 1);
    assert.match(warnings[0], /deterministic release notes/u);
    assert.equal(result.notes, renderReleaseNotes({ ...modelFacts(), commits: modelFacts().commits }));
  }
});
