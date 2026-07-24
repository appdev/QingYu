function readEnv(name) {
  return process.env[name]?.trim() || "";
}

function splitArgs(value) {
  return value.split(/\s+/).filter(Boolean);
}

function hasBundleOverride(args) {
  return args.some((arg) => arg === "--bundles" || arg === "-b" || arg.startsWith("--bundles="));
}

function isPrereleaseTag(tag) {
  const version = tag.replace(/^v/u, "");
  return version.includes("-");
}

const args = splitArgs(readEnv("TAURI_BUILD_ARGS"));
const platform = readEnv("ASSET_PLATFORM");
const releaseTag = readEnv("RELEASE_TAG");
const signedRelease = readEnv("ENABLE_SIGNED_RELEASE");

if (platform === "windows" && isPrereleaseTag(releaseTag) && !hasBundleOverride(args)) {
  args.push("--bundles", "nsis");
}

if (signedRelease === "true") {
  args.push("--config", "tauri.updater.conf.json");
} else if (signedRelease === "false") {
  args.push("--no-sign");
}

process.stdout.write(`${args.join(" ")}\n`);
