import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const WORKSPACE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const RUST_MANIFEST = path.join(
  WORKSPACE_ROOT,
  "apps/desktop/src-tauri/crates/qingyu-dejavu/Cargo.toml",
);
const GO_MODULE = path.join(WORKSPACE_ROOT, "scripts/dejavu-interop-go");
const REQUEST_TIMEOUT_MS = 120_000;
const BUILD_TIMEOUT_MS = 300_000;
const MAX_OUTPUT_BYTES = 1024 * 1024;
const KEY_BASE64 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const BASE_TIME_MS = Date.UTC(2025, 0, 1, 0, 0, 0);

export const OPERATIONS = Object.freeze(["index-and-sync", "inspect"]);
export const SCENARIOS = Object.freeze([
  "go-create-rust-change-go-observe",
  "rust-create-go-change-rust-observe",
  "independent-paths-converge",
  "same-path-conflict",
  "go-identical-first-syncignore-conflict",
  "go-failure-before-ref-publication",
  "rust-failure-before-ref-publication",
]);

function runBuild(executable, args, cwd) {
  process.stdout.write(`> ${executable} ${args.join(" ")}\n`);
  const result = spawnSync(executable, args, {
    cwd,
    stdio: "inherit",
    timeout: BUILD_TIMEOUT_MS,
    killSignal: "SIGTERM",
  });
  if (result.error?.code === "ETIMEDOUT") {
    throw new Error(`${executable} build timed out`);
  }
  if (result.error || result.status !== 0) {
    throw new Error(`${executable} build failed`);
  }
}

function cargoTargetDirectory() {
  const result = spawnSync(
    "cargo",
    ["metadata", "--manifest-path", RUST_MANIFEST, "--no-deps", "--format-version=1"],
    {
      cwd: WORKSPACE_ROOT,
      encoding: "utf8",
      timeout: BUILD_TIMEOUT_MS,
      maxBuffer: MAX_OUTPUT_BYTES,
    },
  );
  if (result.error || result.status !== 0) {
    throw new Error("cargo metadata failed");
  }
  const metadata = JSON.parse(result.stdout);
  if (typeof metadata.target_directory !== "string" || !path.isAbsolute(metadata.target_directory)) {
    throw new Error("cargo metadata returned an invalid target directory");
  }
  return metadata.target_directory;
}

function buildClients(ownedRoot) {
  runBuild(
    "cargo",
    [
      "build",
      "--manifest-path",
      RUST_MANIFEST,
      "--features",
      "interop-cli",
      "--bin",
      "dejavu-interop",
    ],
    WORKSPACE_ROOT,
  );
  const executableSuffix = process.platform === "win32" ? ".exe" : "";
  const rust = path.join(cargoTargetDirectory(), "debug", `dejavu-interop${executableSuffix}`);
  const binRoot = path.join(ownedRoot, "bin");
  fs.mkdirSync(binRoot, { recursive: true });
  const go = path.join(binRoot, `dejavu-interop-go${executableSuffix}`);
  runBuild("go", ["build", "-o", go, "./"], GO_MODULE);
  for (const [name, executable] of Object.entries({ rust, go })) {
    if (!fs.statSync(executable).isFile()) {
      throw new Error(`${name} interop client was not built`);
    }
  }
  return { rust, go };
}

function createClient(scenarioRoot, language, deviceId) {
  const root = path.join(scenarioRoot, "clients", deviceId);
  const client = {
    language,
    deviceId,
    dataPath: path.join(root, "data"),
    repoPath: path.join(root, "repo"),
    historyPath: path.join(root, "history"),
    tempPath: path.join(root, "temp"),
  };
  for (const directory of [client.dataPath, client.repoPath, client.historyPath, client.tempPath]) {
    fs.mkdirSync(directory, { recursive: true });
  }
  return client;
}

function createScenario(ownedRoot, name) {
  const root = path.join(ownedRoot, "scenarios", name);
  const cloudRoot = path.join(root, "cloud", "repo");
  fs.mkdirSync(cloudRoot, { recursive: true });
  return { root, cloudRoot };
}

function requestFor(client, cloudRoot, operation = "index-and-sync", fail = false) {
  return {
    operation,
    deviceId: client.deviceId,
    dataPath: client.dataPath,
    repoPath: client.repoPath,
    historyPath: client.historyPath,
    tempPath: client.tempPath,
    keyBase64: KEY_BASE64,
    cloudRoot,
    failBeforeRefPublication: fail,
  };
}

function parseSingleResponse(stdout, language) {
  if (!/^[^\r\n]+\r?\n?$/.test(stdout)) {
    throw new Error(`${language} client did not emit exactly one JSON line`);
  }
  let response;
  try {
    response = JSON.parse(stdout.trimEnd());
  } catch {
    throw new Error(`${language} client emitted invalid JSON`);
  }
  const keys = Object.keys(response).sort();
  const expectedKeys = ["conflicts", "errorCode", "indexId", "removes", "upserts"].sort();
  if (JSON.stringify(keys) !== JSON.stringify(expectedKeys)) {
    throw new Error(`${language} client emitted an invalid response shape`);
  }
  if (
    !(response.indexId === null || /^[0-9a-f]{40}$/.test(response.indexId)) ||
    !Number.isSafeInteger(response.upserts) ||
    response.upserts < 0 ||
    !Number.isSafeInteger(response.removes) ||
    response.removes < 0 ||
    !Number.isSafeInteger(response.conflicts) ||
    response.conflicts < 0 ||
    !(response.errorCode === null || /^[a-z0-9_]+$/.test(response.errorCode))
  ) {
    throw new Error(`${language} client emitted invalid response values`);
  }
  return response;
}

function spawnClient(executables, language, input) {
  const executable = executables[language];
  const result = spawnSync(executable, [], {
    cwd: WORKSPACE_ROOT,
    input,
    encoding: "utf8",
    timeout: REQUEST_TIMEOUT_MS,
    killSignal: "SIGTERM",
    maxBuffer: MAX_OUTPUT_BYTES,
  });
  if (result.error?.code === "ETIMEDOUT") {
    throw new Error(`${language} client timed out`);
  }
  if (result.error) {
    throw new Error(`${language} client failed to start`);
  }
  return { result, response: parseSingleResponse(result.stdout, language) };
}

function runClient(executables, client, cloudRoot, { operation = "index-and-sync", fail = false } = {}) {
  const request = requestFor(client, cloudRoot, operation, fail);
  const { result, response } = spawnClient(executables, client.language, JSON.stringify(request));
  if (fail) {
    if (result.status === 0 || response.errorCode !== "ref_publication_injected") {
      throw new Error(`${client.language} client did not stop before ref publication`);
    }
    const expectedStderr = "dejavu-interop: request failed (ref_publication_injected)\n";
    if (result.stderr !== expectedStderr) {
      throw new Error(`${client.language} client emitted unsafe failure diagnostics`);
    }
    return response;
  }
  if (result.status !== 0 || result.stderr !== "" || response.errorCode !== null) {
    throw new Error(`${client.language} client operation failed`);
  }
  return response;
}

function assertCount(response, field, expected, label) {
  if (response[field] !== expected) {
    throw new Error(`${label}: expected ${field}=${expected}, got ${response[field]}`);
  }
}

function safeRelativePath(relative) {
  const components = relative.split("/");
  if (
    path.isAbsolute(relative) ||
    components.length === 0 ||
    components.some((component) => component === "" || component === "." || component === "..")
  ) {
    throw new Error("invalid scenario-relative path");
  }
  return components;
}

function writeFile(client, relative, content, minutes) {
  const destination = path.join(client.dataPath, ...safeRelativePath(relative));
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.writeFileSync(destination, content);
  const updated = new Date(BASE_TIME_MS + minutes * 60_000);
  fs.utimesSync(destination, updated, updated);
}

function assertFile(client, relative, expected, label) {
  const actual = fs.readFileSync(path.join(client.dataPath, ...safeRelativePath(relative)));
  const bytes = Buffer.isBuffer(expected) ? expected : Buffer.from(expected);
  if (!actual.equals(bytes)) {
    throw new Error(`${label}: retained bytes differ`);
  }
}

function assertMissingFile(client, relative, label) {
  if (fs.existsSync(path.join(client.dataPath, ...safeRelativePath(relative)))) {
    throw new Error(`${label}: unpublished file became visible`);
  }
}

function verifyProtocol(executables, scenario) {
  const client = createClient(scenario.root, "rust", "protocol-rust");
  const valid = requestFor(client, scenario.cloudRoot, "inspect", false);
  for (const language of ["rust", "go"]) {
    const success = spawnClient(executables, language, JSON.stringify(valid));
    if (
      success.result.status !== 0 ||
      success.result.stderr !== "" ||
      success.response.indexId !== null ||
      success.response.errorCode !== null
    ) {
      throw new Error(`${language} inspect protocol smoke failed`);
    }
    const missingFailureFlag = { ...valid };
    delete missingFailureFlag.failBeforeRefPublication;
    const wrongCase = { ...valid, deviceid: valid.deviceId };
    delete wrongCase.deviceId;
    const invalidCases = [
      [JSON.stringify({ ...valid, unknown: true }), "request_invalid"],
      [JSON.stringify(wrongCase), "request_invalid"],
      [`${JSON.stringify(valid)}${JSON.stringify(valid)}`, "request_invalid"],
      [JSON.stringify({ ...valid, deviceId: "" }), "request_invalid"],
      [JSON.stringify(missingFailureFlag), "request_invalid"],
      [JSON.stringify({ ...valid, dataPath: "relative" }), "path_invalid"],
      [JSON.stringify({ ...valid, keyBase64: "AA==" }), "key_invalid"],
      [`${" ".repeat(1024 * 1024)}x`, "request_too_large"],
    ];
    for (const [input, expectedCode] of invalidCases) {
      const rejected = spawnClient(executables, language, input);
      if (rejected.result.status === 0 || rejected.response.errorCode !== expectedCode) {
        throw new Error(`${language} client accepted invalid protocol input`);
      }
      const expectedStderr = `dejavu-interop: request failed (${expectedCode})\n`;
      if (rejected.result.stderr !== expectedStderr) {
        throw new Error(`${language} client leaked invalid request diagnostics`);
      }
    }
  }
}

function collectCloudKeys(cloudRoot, directory = cloudRoot, prefix = "") {
  const keys = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const relative = prefix === "" ? entry.name : `${prefix}/${entry.name}`;
    const absolute = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      keys.push(...collectCloudKeys(cloudRoot, absolute, relative));
    } else if (entry.isFile()) {
      keys.push(relative);
    }
  }
  return keys.sort();
}

function goCreatesRustChanges(executables, ownedRoot) {
  const scenario = createScenario(ownedRoot, SCENARIOS[0]);
  const go = createClient(scenario.root, "go", "go-a");
  const rust = createClient(scenario.root, "rust", "rust-a");
  writeFile(go, "doc.txt", "created by go\n", 0);
  runClient(executables, go, scenario.cloudRoot);
  const downloaded = runClient(executables, rust, scenario.cloudRoot);
  assertCount(downloaded, "upserts", 1, SCENARIOS[0]);
  assertFile(rust, "doc.txt", "created by go\n", SCENARIOS[0]);

  writeFile(rust, "doc.txt", "changed by rust\n", 10);
  runClient(executables, rust, scenario.cloudRoot);
  const observed = runClient(executables, go, scenario.cloudRoot);
  assertCount(observed, "upserts", 1, SCENARIOS[0]);
  assertFile(go, "doc.txt", "changed by rust\n", SCENARIOS[0]);
}

function rustCreatesGoChanges(executables, ownedRoot) {
  const scenario = createScenario(ownedRoot, SCENARIOS[1]);
  const rust = createClient(scenario.root, "rust", "rust-b");
  const go = createClient(scenario.root, "go", "go-b");
  writeFile(rust, "doc.txt", "created by rust\n", 0);
  runClient(executables, rust, scenario.cloudRoot);

  writeFile(go, "local-anchor.txt", "go anchor\n", 1);
  const downloaded = runClient(executables, go, scenario.cloudRoot);
  assertCount(downloaded, "upserts", 1, SCENARIOS[1]);
  assertFile(go, "doc.txt", "created by rust\n", SCENARIOS[1]);
  writeFile(go, "doc.txt", "changed by go\n", 10);
  runClient(executables, go, scenario.cloudRoot);
  const observed = runClient(executables, rust, scenario.cloudRoot);
  assertFile(rust, "doc.txt", "changed by go\n", SCENARIOS[1]);
  assertFile(rust, "local-anchor.txt", "go anchor\n", SCENARIOS[1]);
  if (observed.upserts < 1) {
    throw new Error(`${SCENARIOS[1]}: Rust did not observe Go changes`);
  }
}

function independentPathsConverge(executables, ownedRoot) {
  const scenario = createScenario(ownedRoot, SCENARIOS[2]);
  const go = createClient(scenario.root, "go", "go-c");
  const rust = createClient(scenario.root, "rust", "rust-c");
  writeFile(go, "anchor.txt", "base\n", 0);
  runClient(executables, go, scenario.cloudRoot);
  runClient(executables, rust, scenario.cloudRoot);

  writeFile(go, "go.txt", "go independent\n", 10);
  writeFile(rust, "rust.txt", "rust independent\n", 11);
  runClient(executables, go, scenario.cloudRoot);
  const merged = runClient(executables, rust, scenario.cloudRoot);
  assertCount(merged, "conflicts", 0, SCENARIOS[2]);
  assertCount(merged, "upserts", 1, SCENARIOS[2]);
  runClient(executables, go, scenario.cloudRoot);
  for (const client of [go, rust]) {
    assertFile(client, "go.txt", "go independent\n", SCENARIOS[2]);
    assertFile(client, "rust.txt", "rust independent\n", SCENARIOS[2]);
  }
}

function samePathConflicts(executables, ownedRoot) {
  const scenario = createScenario(ownedRoot, SCENARIOS[3]);
  const go = createClient(scenario.root, "go", "go-d");
  const rust = createClient(scenario.root, "rust", "rust-d");
  writeFile(go, "go-first.txt", "base one\n", 0);
  writeFile(go, "rust-first.txt", "base two\n", 0);
  runClient(executables, go, scenario.cloudRoot);
  runClient(executables, rust, scenario.cloudRoot);

  writeFile(go, "go-first.txt", "remote go bytes\n", 10);
  writeFile(rust, "go-first.txt", "local rust bytes\n", 11);
  runClient(executables, go, scenario.cloudRoot);
  const rustConflict = runClient(executables, rust, scenario.cloudRoot);
  assertCount(rustConflict, "conflicts", 1, SCENARIOS[3]);
  assertFile(rust, "go-first.txt", "local rust bytes\n", SCENARIOS[3]);
  const goAfterRustConflict = runClient(executables, go, scenario.cloudRoot);
  if (goAfterRustConflict.indexId !== rustConflict.indexId) {
    throw new Error(`${SCENARIOS[3]}: Go did not converge on Rust's retained index`);
  }
  assertFile(go, "go-first.txt", "local rust bytes\n", SCENARIOS[3]);

  writeFile(rust, "rust-first.txt", "remote rust bytes\n", 20);
  writeFile(go, "rust-first.txt", "local go bytes\n", 21);
  runClient(executables, rust, scenario.cloudRoot);
  const goConflict = runClient(executables, go, scenario.cloudRoot);
  assertCount(goConflict, "conflicts", 1, SCENARIOS[3]);
  assertFile(go, "rust-first.txt", "local go bytes\n", SCENARIOS[3]);
  const rustAfterGoConflict = runClient(executables, rust, scenario.cloudRoot);
  if (rustAfterGoConflict.indexId !== goConflict.indexId) {
    throw new Error(`${SCENARIOS[3]}: Rust did not converge on Go's retained index`);
  }
  for (const client of [go, rust]) {
    assertFile(client, "go-first.txt", "local rust bytes\n", SCENARIOS[3]);
    assertFile(client, "rust-first.txt", "local go bytes\n", SCENARIOS[3]);
  }
}

function goIdenticalFirstSyncignoreConflicts(executables, ownedRoot) {
  const scenario = createScenario(ownedRoot, SCENARIOS[4]);
  const first = createClient(scenario.root, "go", "go-syncignore-a");
  const second = createClient(scenario.root, "go", "go-syncignore-b");
  writeFile(first, ".siyuan/syncignore", "", 0);
  writeFile(second, ".siyuan/syncignore", "", 1);

  const uploaded = runClient(executables, first, scenario.cloudRoot);
  assertCount(uploaded, "conflicts", 0, SCENARIOS[4]);
  const merged = runClient(executables, second, scenario.cloudRoot);
  assertCount(merged, "conflicts", 1, SCENARIOS[4]);
  assertFile(second, ".siyuan/syncignore", "", SCENARIOS[4]);
}

function failureBeforePublication(executables, ownedRoot, failingLanguage) {
  const scenarioName = failingLanguage === "go" ? SCENARIOS[5] : SCENARIOS[6];
  const scenario = createScenario(ownedRoot, scenarioName);
  const failed = createClient(scenario.root, failingLanguage, `${failingLanguage}-failed`);
  writeFile(failed, "recover.txt", `created before ${failingLanguage} failure\n`, 0);
  runClient(executables, failed, scenario.cloudRoot, { fail: true });
  if (fs.existsSync(path.join(scenario.cloudRoot, "refs", "latest"))) {
    throw new Error(`${scenarioName}: refs/latest was partially published`);
  }
  const partialKeys = collectCloudKeys(scenario.cloudRoot);
  if (
    !partialKeys.some((key) => key.startsWith("indexes/")) ||
    !partialKeys.some((key) => key.startsWith("objects/"))
  ) {
    throw new Error(`${scenarioName}: failure did not occur after repository object upload`);
  }

  const recoveringLanguage = failingLanguage === "go" ? "rust" : "go";
  const recovering = createClient(
    scenario.root,
    recoveringLanguage,
    `${recoveringLanguage}-recovery`,
  );
  writeFile(recovering, "independent.txt", `${recoveringLanguage} independent\n`, 1);
  runClient(executables, recovering, scenario.cloudRoot);
  assertMissingFile(recovering, "recover.txt", scenarioName);
  if (!fs.statSync(path.join(scenario.cloudRoot, "refs", "latest")).isFile()) {
    throw new Error(`${scenarioName}: independent client did not publish refs/latest`);
  }

  const merged = runClient(executables, failed, scenario.cloudRoot);
  const converged = runClient(executables, recovering, scenario.cloudRoot);
  if (merged.indexId === null || converged.indexId !== merged.indexId) {
    throw new Error(`${scenarioName}: failed and independent clients did not converge`);
  }

  for (const client of [failed, recovering]) {
    assertFile(
      client,
      "recover.txt",
      `created before ${failingLanguage} failure\n`,
      scenarioName,
    );
    assertFile(
      client,
      "independent.txt",
      `${recoveringLanguage} independent\n`,
      scenarioName,
    );
  }
}

function removeOwnedRoot(ownedRoot) {
  const resolved = path.resolve(ownedRoot);
  if (
    path.dirname(resolved) !== path.resolve(os.tmpdir()) ||
    !path.basename(resolved).startsWith("qingyu-dejavu-interop-")
  ) {
    throw new Error("refuse to remove an unowned interoperability root");
  }
  fs.rmSync(resolved, { recursive: true, force: true });
}

export function executeInterop() {
  const ownedRoot = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-dejavu-interop-"));
  try {
    const executables = buildClients(ownedRoot);
    verifyProtocol(executables, createScenario(ownedRoot, "protocol"));
    const scenarios = [
      goCreatesRustChanges,
      rustCreatesGoChanges,
      independentPathsConverge,
      samePathConflicts,
      goIdenticalFirstSyncignoreConflicts,
      (clients, root) => failureBeforePublication(clients, root, "go"),
      (clients, root) => failureBeforePublication(clients, root, "rust"),
    ];
    scenarios.forEach((scenario, index) => {
      scenario(executables, ownedRoot);
      process.stdout.write(`ok ${index + 1} - ${SCENARIOS[index]}\n`);
    });
  } finally {
    removeOwnedRoot(ownedRoot);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  executeInterop();
}
