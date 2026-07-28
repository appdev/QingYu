import { readFile } from "node:fs/promises";
import { EventEmitter } from "node:events";
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

function exitingChild(code, signal = null) {
  const child = new EventEmitter();
  queueMicrotask(() => child.emit("exit", code, signal));
  return child;
}

function erroringChild() {
  const child = new EventEmitter();
  queueMicrotask(() => child.emit("error", new Error("private spawn detail")));
  return child;
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

  it("rejects orchestration with a typed safe environment error before spawning", async () => {
    const wrapper = await loadWrapper();
    expect(wrapper).not.toBeNull();
    if (!wrapper) return;

    const environment = completeEnvironment();
    delete environment.MARKRA_TEST_S3_ENDPOINT;
    const spawnProcess = vi.fn();

    await expect(
      wrapper.runAllLiveS3Tests(environment, spawnProcess)
    ).rejects.toBeInstanceOf(wrapper.LiveS3EnvironmentError);
    await expect(
      wrapper.runAllLiveS3Tests(environment, spawnProcess)
    ).rejects.toThrow("MARKRA_TEST_S3_ENDPOINT");
    expect(spawnProcess).not.toHaveBeenCalled();
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
        "live_minio_s3_",
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

  it("runs only protected S3 transport checks before the Dejavu ordinary-notes test", async () => {
    const wrapper = await loadWrapper();
    expect(wrapper).not.toBeNull();
    if (!wrapper) return;

    const environment = completeEnvironment();
    const spawnProcess = vi
      .fn()
      .mockImplementationOnce(() => exitingChild(0))
      .mockImplementationOnce(() => exitingChild(0));

    await expect(wrapper.runAllLiveS3Tests(environment, spawnProcess)).resolves.toBe(0);
    expect(spawnProcess).toHaveBeenCalledTimes(2);
    expect(spawnProcess.mock.calls[0]).toEqual([
      "cargo",
      [
        "test",
        "--manifest-path",
        "apps/desktop/src-tauri/Cargo.toml",
        "live_minio_s3_",
        "--",
        "--ignored",
        "--nocapture",
        "--test-threads=1"
      ],
      { env: environment, stdio: "inherit" }
    ]);
    expect(spawnProcess.mock.calls[1]).toEqual([
      "cargo",
      [
        "test",
        "--manifest-path",
        "apps/desktop/src-tauri/Cargo.toml",
        "-p",
        "qingyu-dejavu",
        "--test",
        "s3_minio",
        "--",
        "--nocapture",
        "--test-threads=1"
      ],
      {
        env: { ...environment, QINGYU_S3_LIVE_TESTS: "1" },
        stdio: "inherit"
      }
    ]);
    const invocations = JSON.stringify(
      spawnProcess.mock.calls.map(([command, args]) => [command, args])
    );
    expect(invocations).not.toContain("private-access-value");
    expect(invocations).not.toContain("private-secret-value");
  });

  it("still runs Dejavu after a protected transport failure and returns the first code", async () => {
    const wrapper = await loadWrapper();
    expect(wrapper).not.toBeNull();
    if (!wrapper) return;

    const spawnProcess = vi
      .fn()
      .mockImplementationOnce(() => exitingChild(9))
      .mockImplementationOnce(() => exitingChild(0));

    await expect(
      wrapper.runAllLiveS3Tests(completeEnvironment(), spawnProcess)
    ).resolves.toBe(9);
    expect(spawnProcess).toHaveBeenCalledTimes(2);
  });

  it.each([
    ["SIGINT", 130],
    ["SIGTERM", 1]
  ])(
    "stops after the protected transport suite exits on %s and returns %i",
    async (signal, expectedCode) => {
      const wrapper = await loadWrapper();
      expect(wrapper).not.toBeNull();
      if (!wrapper) return;

      const spawnProcess = vi.fn(() => exitingChild(null, signal));

      await expect(
        wrapper.runAllLiveS3Tests(completeEnvironment(), spawnProcess)
      ).resolves.toBe(expectedCode);
      expect(spawnProcess).toHaveBeenCalledTimes(1);
    }
  );

  it("continues after the protected transport child errors and preserves its failure code", async () => {
    const wrapper = await loadWrapper();
    expect(wrapper).not.toBeNull();
    if (!wrapper) return;

    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    const spawnProcess = vi
      .fn()
      .mockImplementationOnce(() => erroringChild())
      .mockImplementationOnce(() => exitingChild(0));

    await expect(
      wrapper.runAllLiveS3Tests(completeEnvironment(), spawnProcess)
    ).resolves.toBe(1);
    expect(spawnProcess).toHaveBeenCalledTimes(2);
    expect(consoleError).toHaveBeenCalledWith(
      "Failed to start protected S3 settings transport tests"
    );
    consoleError.mockRestore();
  });

  it("returns the Dejavu crate failure when the protected transport suite succeeds", async () => {
    const wrapper = await loadWrapper();
    expect(wrapper).not.toBeNull();
    if (!wrapper) return;

    const spawnProcess = vi
      .fn()
      .mockImplementationOnce(() => exitingChild(0))
      .mockImplementationOnce(() => exitingChild(7));

    await expect(
      wrapper.runAllLiveS3Tests(completeEnvironment(), spawnProcess)
    ).resolves.toBe(7);
    expect(spawnProcess).toHaveBeenCalledTimes(2);
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
