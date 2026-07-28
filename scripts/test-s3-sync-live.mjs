import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const REQUIRED_ENVIRONMENT_NAMES = [
  "MARKRA_TEST_S3_ENDPOINT",
  "MARKRA_TEST_S3_BUCKET",
  "MARKRA_TEST_S3_ACCESS_KEY_ID",
  "MARKRA_TEST_S3_SECRET_ACCESS_KEY"
];

export class LiveS3EnvironmentError extends Error {}

export function validateLiveS3Environment(environment) {
  const missing = REQUIRED_ENVIRONMENT_NAMES.filter((name) => {
    const value = environment[name];
    return typeof value !== "string" || value.trim().length === 0;
  });
  if (missing.length > 0) {
    throw new LiveS3EnvironmentError(
      `Missing required live S3 test variable(s): ${missing.join(", ")}`
    );
  }
}

export function runLiveS3Tests(environment = process.env, spawnProcess = spawn) {
  validateLiveS3Environment(environment);
  const child = spawnProcess(
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

  child.once("error", (error) => {
    console.error(`Failed to start live S3 tests: ${error.message}`);
    process.exitCode = 1;
  });
  child.once("exit", (code, signal) => {
    process.exitCode = code ?? (signal === "SIGINT" ? 130 : 1);
  });

  return child;
}

function waitForTestProcess(child, label) {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      resolve(result);
    };
    child.once("error", () => {
      console.error(`Failed to start ${label}`);
      finish({ code: 1, signal: null, cancelled: false });
    });
    child.once("exit", (code, signal) => {
      finish({
        code: code ?? (signal === "SIGINT" ? 130 : 1),
        signal: signal ?? null,
        cancelled: signal !== null
      });
    });
  });
}

export async function runAllLiveS3Tests(
  environment = process.env,
  spawnProcess = spawn
) {
  validateLiveS3Environment(environment);
  const existing = spawnProcess(
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
  const existingResult = await waitForTestProcess(
    existing,
    "protected S3 settings transport tests"
  );
  if (existingResult.cancelled) return existingResult.code;

  const dejavu = spawnProcess(
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
  );
  const dejavuResult = await waitForTestProcess(
    dejavu,
    "Dejavu Rust live S3 test"
  );
  return existingResult.code !== 0 ? existingResult.code : dejavuResult.code;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  runAllLiveS3Tests()
    .then((code) => {
      process.exitCode = code;
    })
    .catch((error) => {
      if (error instanceof LiveS3EnvironmentError) {
        console.error(error.message);
        process.exitCode = 2;
      } else {
        console.error("Live S3 test orchestration failed");
        process.exitCode = 1;
      }
    });
}
