import assert from "node:assert/strict";
import test from "node:test";

const releaseNotesModule = await import("./generate-release-notes.mjs").catch(() => ({}));
const { parseGitLog, renderReleaseNotes, selectPreviousRelease } = releaseNotesModule;

test("release notes module exposes focused selection and rendering helpers", () => {
  assert.equal(typeof releaseNotesModule.selectPreviousRelease, "function");
  assert.equal(typeof releaseNotesModule.parseGitLog, "function");
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
    "0123456789abcdef\u001f0123456\u001ffeat: add mobile release\u001fQingYu\u001e" +
      "abcdef0123456789\u001fabcdef0\u001ffix: keep tags stable\u001fContributor\u001e\n",
  );

  assert.deepEqual(commits, [
    {
      sha: "0123456789abcdef",
      shortSha: "0123456",
      subject: "feat: add mobile release",
      author: "QingYu",
    },
    {
      sha: "abcdef0123456789",
      shortSha: "abcdef0",
      subject: "fix: keep tags stable",
      author: "Contributor",
    },
  ]);
});

test("renderReleaseNotes lists commits without a static artifact inventory", () => {
  const notes = renderReleaseNotes({
    currentTag: "v1.7.5",
    previousTag: "v1.7.4",
    commits: [
      {
        sha: "0123456789abcdef",
        shortSha: "0123456",
        subject: "feat: add mobile release",
        author: "QingYu",
      },
    ],
  });

  assert.match(notes, /^## 提交记录$/mu);
  assert.match(notes, /`v1\.7\.4\.\.v1\.7\.5`/u);
  assert.match(notes, /- `0123456` feat: add mobile release — QingYu/u);
  assert.doesNotMatch(notes, /移动端产物说明/u);
  assert.doesNotMatch(notes, /QingYu_1\.7\.5_/u);
});

test("renderReleaseNotes explains the first Release fallback", () => {
  const notes = renderReleaseNotes({ currentTag: "v1.7.5", previousTag: null, commits: [] });

  assert.match(notes, /首个 Release/u);
  assert.match(notes, /当前版本尚无可列出的提交/u);
});
