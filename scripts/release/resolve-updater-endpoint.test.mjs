import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));

function runResolver(releaseTag) {
  return spawnSync(process.execPath, ["scripts/release/resolve-updater-endpoint.mjs"], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      GITHUB_REPOSITORY: "example/markra",
      RELEASE_TAG: releaseTag,
    },
  });
}

test("resolve-updater-endpoint keeps stable releases on the GitHub latest channel", () => {
  const result = runResolver("v2.0.0");

  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), "https://github.com/example/markra/releases/latest/download/latest.json");
});

test("resolve-updater-endpoint routes prereleases to the rolling preview channel", () => {
  for (const releaseTag of ["v2.0.0-alpha.1", "v2.0.0-beta.1", "v2.0.0-rc.1"]) {
    const result = runResolver(releaseTag);

    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), "https://github.com/example/markra/releases/download/preview/latest.json");
  }
});
