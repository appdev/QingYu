import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  buildReleaseFacts,
  validateReleaseFactsProvenance,
} from "./generate-release-notes.mjs";

const rendererModule = await import("./render-release-summary.mjs").catch(() => ({}));
const { renderReleaseSummary, renderReleaseSummaryFiles } = rendererModule;

const expectedRelease = {
  repository: "appdev/QingYu",
  currentTag: "v2.2.0",
  previousTag: "v2.1.0",
  releaseTarget: "fedcba9876543210fedcba9876543210fedcba98",
  signedRelease: false,
};

function releaseFacts() {
  return buildReleaseFacts({
    repository: "appdev/QingYu",
    currentTag: "v2.2.0",
    previousTag: "v2.1.0",
    releaseTarget: expectedRelease.releaseTarget,
    signedRelease: false,
    commits: [
      {
        sha: "0123456789abcdef0123456789abcdef01234567",
        shortSha: "01234567",
        subject: "feat(editor): improve paste",
        body: "Keep Markdown paste predictable.",
        author: "QingYu",
        changedPaths: ["packages/editor/src/paste.ts"],
        insertions: 12,
        deletions: 3,
      },
      {
        sha: "abcdef0123456789abcdef0123456789abcdef01",
        shortSha: "abcdef01",
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

function releaseSummary() {
  return {
    summary: "本次更新改善了编辑体验，并提升了同步可靠性。",
    sections: [
      {
        title: "编辑体验",
        items: [{ text: "粘贴 Markdown 时的行为更加稳定。", commitShas: ["01234567"] }],
      },
      {
        title: "同步可靠性",
        items: [{ text: "避免同一内容被重复上传。", commitShas: ["abcdef01"] }],
      },
    ],
    notice: null,
    otherChanges: [],
  };
}

test("release summary module exposes a focused renderer", () => {
  assert.equal(typeof renderReleaseSummary, "function");
  assert.equal(typeof renderReleaseSummaryFiles, "function");
});

test("renderReleaseSummary validates facts and renders final Markdown", () => {
  const notes = renderReleaseSummary(releaseSummary(), releaseFacts(), expectedRelease);

  assert.match(notes, /^本次更新改善了编辑体验，并提升了同步可靠性。$/mu);
  assert.match(notes, /^## 编辑体验$/mu);
  assert.match(notes, /粘贴 Markdown 时的行为更加稳定。 \(`01234567`\)/u);
  assert.match(notes, /^## 同步可靠性$/mu);
  assert.match(notes, /compare\/v2\.1\.0\.\.\.v2\.2\.0/u);
  assert.match(notes, /本次为未签名构建/u);
});

test("renderReleaseSummary rejects unknown commits and unsupported claims", () => {
  const unknownCommit = releaseSummary();
  unknownCommit.sections[0].items[0].commitShas = ["deadbee"];
  assert.throws(
    () => renderReleaseSummary(unknownCommit, releaseFacts(), expectedRelease),
    /unknown commit/u,
  );

  const unsupportedClaim = releaseSummary();
  unsupportedClaim.summary = "本次新增 Windows 自动更新与安全迁移。";
  assert.throws(
    () => renderReleaseSummary(unsupportedClaim, releaseFacts(), expectedRelease),
    /unsupported claim/u,
  );
});

test("renderReleaseSummary validates release facts and expected provenance", () => {
  const invalidCases = [
    {
      mutate(facts) {
        facts.schemaVersion = 999;
      },
      error: /schemaVersion/u,
    },
    {
      mutate(facts) {
        facts.signedRelease = "false";
      },
      error: /signedRelease/u,
    },
    {
      mutate(facts) {
        facts.compareUrl = "javascript:alert(1)";
      },
      error: /compareUrl/u,
    },
    {
      mutate(facts) {
        facts.commits[0].sha = "not-a-sha";
      },
      error: /commit SHA/u,
    },
    {
      mutate(facts) {
        facts.commits[0].sha = "abcdef0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        facts.commits[0].shortSha = "abcdef0";
        facts.commits[1].sha = "abcdef0fbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        facts.commits[1].shortSha = "abcdef0f";
      },
      error: /short SHA.*unambiguous/u,
    },
  ];

  for (const invalidCase of invalidCases) {
    const facts = releaseFacts();
    invalidCase.mutate(facts);
    assert.throws(
      () => renderReleaseSummary(releaseSummary(), facts, expectedRelease),
      invalidCase.error,
    );
  }

  assert.throws(
    () =>
      renderReleaseSummary(releaseSummary(), releaseFacts(), {
        ...expectedRelease,
        releaseTarget: "different-target",
      }),
    /releaseTarget/u,
  );

  const tamperedSigningFacts = releaseFacts();
  tamperedSigningFacts.signedRelease = true;
  assert.throws(
    () => renderReleaseSummary(releaseSummary(), tamperedSigningFacts, expectedRelease),
    /signedRelease/u,
  );

  assert.throws(
    () =>
      renderReleaseSummary(releaseSummary(), releaseFacts(), {
        ...expectedRelease,
        previousTag: "v2.0.0",
      }),
    /previousTag/u,
  );
});

function releaseFactsGitRunner(facts) {
  return (args) => {
    if (args[0] === "rev-parse" && args[2] === `${facts.releaseTarget}^{commit}`) {
      return `${facts.releaseTarget}\n`;
    }
    if (args[0] === "rev-parse" && args[2] === `${facts.previousTag}^{commit}`) {
      return "cccccccccccccccccccccccccccccccccccccccc\n";
    }
    if (args[0] === "merge-base") {
      return "";
    }
    if (args[0] === "log") {
      return facts.commits
        .map(
          (commit) =>
            `${commit.sha}\u001f${commit.shortSha}\u001f${commit.subject}\u001f${commit.body}\u001f${commit.author}\u001e`,
        )
        .join("");
    }
    if (args.includes("diff-tree")) {
      const commit = facts.commits.find((candidate) => candidate.sha === args.at(-1));
      assert.ok(commit, `Unexpected diff-tree SHA: ${args.at(-1)}`);
      return commit.changedPaths
        .map((changedPath, index) =>
          index === 0 ? `${commit.insertions}\t${commit.deletions}\t${changedPath}\n` : `0\t0\t${changedPath}\n`,
        )
        .join("");
    }
    throw new Error(`Unexpected git args: ${args.join(" ")}`);
  };
}

test("release facts provenance must match the local Git range exactly", () => {
  const facts = releaseFacts();
  const runGitImpl = releaseFactsGitRunner(facts);
  assert.doesNotThrow(() => validateReleaseFactsProvenance(facts, { runGitImpl }));

  const tamperedFacts = structuredClone(facts);
  tamperedFacts.commits[0].subject = "feat(editor): invented change";
  assert.throws(
    () => validateReleaseFactsProvenance(tamperedFacts, { runGitImpl }),
    /do not match the local Git range/u,
  );
});

test("render-release-summary files writes validated notes and rejects malformed JSON", (context) => {
  const temporaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-release-summary-"));
  context.after(() => fs.rmSync(temporaryDirectory, { recursive: true, force: true }));

  const factsPath = path.join(temporaryDirectory, "release-facts.json");
  const summaryPath = path.join(temporaryDirectory, "release-summary.json");
  const notesPath = path.join(temporaryDirectory, "release-notes.md");
  fs.writeFileSync(factsPath, `${JSON.stringify(releaseFacts(), null, 2)}\n`, "utf8");
  fs.writeFileSync(summaryPath, `${JSON.stringify(releaseSummary(), null, 2)}\n`, "utf8");

  const rendererEnv = {
    RELEASE_FACTS_PATH: factsPath,
    RELEASE_SUMMARY_PATH: summaryPath,
    RELEASE_NOTES_PATH: notesPath,
    GITHUB_REPOSITORY: expectedRelease.repository,
    RELEASE_TAG: expectedRelease.currentTag,
    RELEASE_PREVIOUS_TAG: expectedRelease.previousTag,
    RELEASE_TARGET: expectedRelease.releaseTarget,
    SIGNED_RELEASE: "false",
  };
  const facts = releaseFacts();
  renderReleaseSummaryFiles(rendererEnv, { runGitImpl: releaseFactsGitRunner(facts) });
  assert.equal(
    fs.readFileSync(notesPath, "utf8"),
    renderReleaseSummary(releaseSummary(), releaseFacts(), expectedRelease),
  );

  const reviewedNotes = fs.readFileSync(notesPath, "utf8");
  fs.writeFileSync(summaryPath, "{ malformed\n", "utf8");
  assert.throws(
    () => renderReleaseSummaryFiles(rendererEnv, { runGitImpl: releaseFactsGitRunner(facts) }),
    /JSON|Expected property name/u,
  );
  assert.equal(fs.readFileSync(notesPath, "utf8"), reviewedNotes);
});
