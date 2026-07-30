import assert from "node:assert/strict";
import test from "node:test";

const validatorModule = await import("./validate-release-draft.mjs").catch(() => ({}));
const { validateReleaseDraft } = validatorModule;

const version = "2.2.0";
const tag = `v${version}`;
const targetSha = "0123456789abcdef0123456789abcdef01234567";

function unsignedAssetNames() {
  return [
    `QingYu_${version}_android_arm64_unsigned.apk`,
    `QingYu_${version}_ios_simulator_arm64_unsigned.app.zip`,
    `QingYu_${version}_linux_arm64.AppImage`,
    `QingYu_${version}_linux_arm64.deb`,
    `QingYu_${version}_linux_arm64.rpm`,
    `QingYu_${version}_linux_x64.AppImage`,
    `QingYu_${version}_linux_x64.deb`,
    `QingYu_${version}_linux_x64.rpm`,
    `QingYu_${version}_macos_arm64.dmg`,
    `QingYu_${version}_macos_x64.dmg`,
    `QingYu_${version}_windows_x64_portable.zip`,
    `QingYu_${version}_windows_x64_setup.exe`,
  ];
}

function signedAssetNames() {
  return [
    `QingYu_${version}_macos_arm64_updater.app.tar.gz`,
    `QingYu_${version}_macos_arm64_updater.app.tar.gz.sig`,
    `QingYu_${version}_macos_x64_updater.app.tar.gz`,
    `QingYu_${version}_macos_x64_updater.app.tar.gz.sig`,
    `QingYu_${version}_linux_arm64.AppImage.sig`,
    `QingYu_${version}_linux_x64.AppImage.sig`,
    `QingYu_${version}_windows_x64_setup.exe.sig`,
    "latest.json",
  ];
}

function releaseFixture(overrides = {}) {
  return {
    tag_name: tag,
    target_commitish: targetSha,
    draft: true,
    prerelease: false,
    name: `QingYu ${tag}`,
    body: `本次更新改善了编辑体验。\n\n[查看完整变更](https://github.com/appdev/QingYu/compare/v2.1.0...${tag})`,
    assets: unsignedAssetNames().map((name) => ({ name, size: 1_024 })),
    ...overrides,
  };
}

test("release draft validator exposes its pure validation helper", () => {
  assert.equal(typeof validateReleaseDraft, "function");
});

test("validateReleaseDraft accepts a complete unsigned draft without mutating title or body", () => {
  const release = releaseFixture();
  const result = validateReleaseDraft({
    release,
    repository: "appdev/QingYu",
    version,
    tag,
    resolvedTargetSha: targetSha,
  });

  assert.equal(result.signedRelease, false);
  assert.equal(result.alreadyPublished, false);
  assert.equal(result.title, release.name);
  assert.equal(result.body, release.body);
  assert.deepEqual(result.assetNames, unsignedAssetNames().sort());
});

test("validateReleaseDraft fails closed for target, body, and asset errors", async (t) => {
  const base = {
    repository: "appdev/QingYu",
    version,
    tag,
    resolvedTargetSha: targetSha,
  };

  await t.test("release target differs from resolved tag", () => {
    assert.throws(
      () => validateReleaseDraft({ ...base, release: releaseFixture({ target_commitish: "wrong" }) }),
      /target commit/u,
    );
  });
  await t.test("body is empty", () => {
    assert.throws(() => validateReleaseDraft({ ...base, release: releaseFixture({ body: "" }) }), /body/u);
  });
  await t.test("body contains a generator placeholder", () => {
    assert.throws(
      () => validateReleaseDraft({ ...base, release: releaseFixture({ body: "${currentTag}" }) }),
      /placeholder/u,
    );
  });
  await t.test("required asset is missing", () => {
    const assets = releaseFixture().assets.slice(1);
    assert.throws(
      () => validateReleaseDraft({ ...base, release: releaseFixture({ assets }) }),
      /missing required release asset/iu,
    );
  });
  await t.test("asset names are duplicated", () => {
    const assets = releaseFixture().assets;
    assert.throws(
      () => validateReleaseDraft({ ...base, release: releaseFixture({ assets: [...assets, assets[0]] }) }),
      /duplicate release asset/iu,
    );
  });
  await t.test("asset is empty", () => {
    const assets = releaseFixture().assets.map((asset, index) =>
      index === 0 ? { ...asset, size: 0 } : asset,
    );
    assert.throws(
      () => validateReleaseDraft({ ...base, release: releaseFixture({ assets }) }),
      /non-zero/u,
    );
  });
});

test("validateReleaseDraft accepts complete signed updater assets and rejects partial signing", () => {
  const baseAssets = releaseFixture().assets;
  const signedAssets = signedAssetNames().map((name) => ({ name, size: 2_048 }));
  const argumentsBase = {
    repository: "appdev/QingYu",
    version,
    tag,
    resolvedTargetSha: targetSha,
  };

  const result = validateReleaseDraft({
    ...argumentsBase,
    release: releaseFixture({ assets: [...baseAssets, ...signedAssets] }),
  });
  assert.equal(result.signedRelease, true);

  assert.throws(
    () =>
      validateReleaseDraft({
        ...argumentsBase,
        release: releaseFixture({ assets: [...baseAssets, signedAssets[0]] }),
      }),
    /incomplete signed updater assets/iu,
  );
});

test("validateReleaseDraft enforces prerelease state and permits explicit distribution retry", () => {
  assert.throws(
    () =>
      validateReleaseDraft({
        release: releaseFixture({ prerelease: true }),
        repository: "appdev/QingYu",
        version,
        tag,
        resolvedTargetSha: targetSha,
      }),
    /prerelease state/u,
  );

  assert.throws(
    () =>
      validateReleaseDraft({
        release: releaseFixture({ draft: false }),
        repository: "appdev/QingYu",
        version,
        tag,
        resolvedTargetSha: targetSha,
      }),
    /not a draft/u,
  );

  const retry = validateReleaseDraft({
    release: releaseFixture({ draft: false }),
    repository: "appdev/QingYu",
    version,
    tag,
    resolvedTargetSha: targetSha,
    allowPublishedRetry: true,
  });
  assert.equal(retry.alreadyPublished, true);
});
