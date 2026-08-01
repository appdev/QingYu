import assert from "node:assert/strict";
import test from "node:test";

const guardModule = await import("./guard-release-draft-write.mjs").catch(() => ({}));
const { validateDraftWrite } = guardModule;

const options = {
  releaseTag: "v2.4.0",
  releaseTarget: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  baselineNotes: "baseline notes\n",
  allowedStaleDraftId: null,
};

function release(overrides = {}) {
  return {
    id: 123,
    tag_name: options.releaseTag,
    target_commitish: options.releaseTarget,
    draft: true,
    body: options.baselineNotes,
    ...overrides,
  };
}

test("draft write guard exposes its pure validator", () => {
  assert.equal(typeof validateDraftWrite, "function");
});

test("draft write guard permits creation and safe idempotent refresh", () => {
  assert.deepEqual(validateDraftWrite([], options), { mode: "create", releaseId: null });
  assert.deepEqual(validateDraftWrite([release()], options), {
    mode: "refresh-baseline-draft",
    releaseId: 123,
  });
});

test("draft write guard replaces a stale draft only with its explicitly allowed ID", () => {
  const staleDraft = release({
    target_commitish: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    body: "reviewed",
  });
  assert.throws(() => validateDraftWrite([staleDraft], options), /explicit stale draft ID/u);
  assert.throws(
    () => validateDraftWrite([staleDraft], { ...options, allowedStaleDraftId: "456" }),
    /explicit stale draft ID/u,
  );
  assert.deepEqual(
    validateDraftWrite([staleDraft], { ...options, allowedStaleDraftId: "123" }),
    { mode: "replace-stale-draft", releaseId: 123 },
  );
});

test("draft write guard refuses customized, published, and duplicate releases", () => {
  assert.throws(
    () => validateDraftWrite([release({ body: "Codex-reviewed notes\n" })], options),
    /customized draft/u,
  );
  assert.throws(
    () => validateDraftWrite([release({ draft: false })], options),
    /already published/u,
  );
  assert.throws(
    () => validateDraftWrite([release(), release({ id: 456 })], options),
    /multiple releases/u,
  );
});
