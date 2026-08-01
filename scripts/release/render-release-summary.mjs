import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import {
  renderModelReleaseNotes,
  validateModelSummary,
  validateReleaseFacts,
  validateReleaseFactsProvenance,
} from "./generate-release-notes.mjs";

function requireEnv(env, name) {
  const value = env[name]?.trim();
  if (!value) {
    throw new Error(`${name} is required.`);
  }
  return value;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function parseBoolean(value, name) {
  if (value === "true") {
    return true;
  }
  if (value === "false") {
    return false;
  }
  throw new Error(`${name} must be true or false.`);
}

export function renderReleaseSummary(summary, facts, expectedRelease) {
  const validatedFacts = validateReleaseFacts(facts, expectedRelease);
  return renderModelReleaseNotes(validateModelSummary(summary, validatedFacts), validatedFacts);
}

function writeFileAtomic(filePath, contents) {
  const directory = path.dirname(filePath);
  const temporaryPath = path.join(
    directory,
    `.${path.basename(filePath)}.${process.pid}.${Date.now()}.tmp`,
  );

  fs.mkdirSync(directory, { recursive: true });
  try {
    fs.writeFileSync(temporaryPath, contents, { encoding: "utf8", flag: "wx" });
    fs.renameSync(temporaryPath, filePath);
  } catch (error) {
    if (fs.existsSync(temporaryPath)) {
      fs.unlinkSync(temporaryPath);
    }
    throw error;
  }
}

export function renderReleaseSummaryFiles(env = process.env, { runGitImpl } = {}) {
  const factsPath = path.resolve(requireEnv(env, "RELEASE_FACTS_PATH"));
  const summaryPath = path.resolve(requireEnv(env, "RELEASE_SUMMARY_PATH"));
  const notesPath = path.resolve(requireEnv(env, "RELEASE_NOTES_PATH"));
  const expectedRelease = {
    repository: requireEnv(env, "GITHUB_REPOSITORY"),
    currentTag: requireEnv(env, "RELEASE_TAG"),
    previousTag: requireEnv(env, "RELEASE_PREVIOUS_TAG") === "none"
      ? null
      : requireEnv(env, "RELEASE_PREVIOUS_TAG"),
    releaseTarget: requireEnv(env, "RELEASE_TARGET"),
    signedRelease: parseBoolean(requireEnv(env, "SIGNED_RELEASE"), "SIGNED_RELEASE"),
  };
  const summary = readJson(summaryPath);
  const facts = validateReleaseFacts(readJson(factsPath), expectedRelease);
  validateReleaseFactsProvenance(facts, { runGitImpl });
  const notes = renderReleaseSummary(summary, facts, expectedRelease);

  writeFileAtomic(notesPath, notes);
}

const isCli = process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;

if (isCli) {
  try {
    renderReleaseSummaryFiles();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
