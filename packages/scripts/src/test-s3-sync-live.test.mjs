import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { describe, expect, it, vi } from "vitest";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const wrapperUrl = pathToFileURL(resolve(repositoryRoot, "scripts/test-s3-sync-live.mjs")).href;

async function loadWrapper() {
  return import(wrapperUrl).catch(() => null);
}

function completeEnvironment() {
  return {
    MARKRA_TEST_S3_ACCESS_KEY_ID: "private-access-value",
    MARKRA_TEST_S3_BUCKET: "private-bucket-value",
    MARKRA_TEST_S3_ENDPOINT: "https://private-endpoint.example.test",
    MARKRA_TEST_S3_SECRET_ACCESS_KEY: "private-secret-value"
  };
}

describe("live S3 test wrapper", () => {
  it("reports only missing environment variable names", async () => {
    const wrapper = await loadWrapper();
    expect(wrapper).not.toBeNull();
    if (!wrapper) return;

    const environment = completeEnvironment();
    delete environment.MARKRA_TEST_S3_ENDPOINT;

    let message = "";
    try {
      wrapper.validateLiveS3Environment(environment);
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    }

    expect(message).toContain("MARKRA_TEST_S3_ENDPOINT");
    expect(message).not.toContain("private-access-value");
    expect(message).not.toContain("private-secret-value");
  });

  it("treats whitespace-only required values as missing", async () => {
    const wrapper = await loadWrapper();
    expect(wrapper).not.toBeNull();
    if (!wrapper) return;

    const environment = completeEnvironment();
    environment.MARKRA_TEST_S3_BUCKET = "   ";

    expect(() => wrapper.validateLiveS3Environment(environment)).toThrow(
      "MARKRA_TEST_S3_BUCKET"
    );
  });

  it("spawns Cargo with inherited environment and no credential-bearing arguments", async () => {
    const wrapper = await loadWrapper();
    expect(wrapper).not.toBeNull();
    if (!wrapper) return;

    const environment = completeEnvironment();
    const child = { once: vi.fn() };
    const spawnProcess = vi.fn(() => child);

    expect(wrapper.runLiveS3Tests(environment, spawnProcess)).toBe(child);
    expect(spawnProcess).toHaveBeenCalledWith(
      "cargo",
      [
        "test",
        "--manifest-path",
        "apps/desktop/src-tauri/Cargo.toml",
        "live_minio_",
        "--",
        "--ignored",
        "--nocapture",
        "--test-threads=1"
      ],
      { env: environment, stdio: "inherit" }
    );
    const invocation = JSON.stringify(spawnProcess.mock.calls[0].slice(0, 2));
    expect(invocation).not.toContain("private-access-value");
    expect(invocation).not.toContain("private-secret-value");
  });

  it("is the canonical package script entry point", async () => {
    const packageDocument = JSON.parse(
      await readFile(resolve(repositoryRoot, "package.json"), "utf8")
    );

    expect(packageDocument.scripts["test:s3-sync:live"]).toBe(
      "node scripts/test-s3-sync-live.mjs"
    );
  });
});
