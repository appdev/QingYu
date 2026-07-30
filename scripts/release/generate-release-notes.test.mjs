import assert from "node:assert/strict";
import test from "node:test";

const releaseNotesModule = await import("./generate-release-notes.mjs").catch(() => ({}));
const {
  buildCompareUrl,
  buildReleaseFacts,
  parseConventionalSubject,
  parseGitLog,
  parseNumStat,
  renderReleaseNotes,
  selectPreviousRelease,
} = releaseNotesModule;

test("release notes module exposes focused selection and rendering helpers", () => {
  assert.equal(typeof releaseNotesModule.selectPreviousRelease, "function");
  assert.equal(typeof releaseNotesModule.parseGitLog, "function");
  assert.equal(typeof releaseNotesModule.parseNumStat, "function");
  assert.equal(typeof releaseNotesModule.parseConventionalSubject, "function");
  assert.equal(typeof releaseNotesModule.buildReleaseFacts, "function");
  assert.equal(typeof releaseNotesModule.buildCompareUrl, "function");
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
