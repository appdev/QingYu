import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "../../..");
const manifestPath = join(repositoryRoot, "apps/desktop/src-tauri/Cargo.toml");
const tauriRoot = dirname(manifestPath);
const rustcVersion = execFileSync("rustc", ["-vV"], {
  cwd: repositoryRoot,
  encoding: "utf8"
});
const targetTriple = rustcVersion
  .split(/\r?\n/u)
  .find((line) => line.startsWith("host: "))
  ?.slice("host: ".length)
  .trim();

if (!targetTriple) {
  throw new Error("Could not determine the Rust host target triple.");
}

execFileSync("cargo", [
  "build",
  "--manifest-path",
  manifestPath,
  "--bin",
  "qingyu-mcp",
  "--features",
  "desktop-sidecar",
  "--release"
], {
  cwd: repositoryRoot,
  stdio: "inherit"
});

const executableSuffix = process.platform === "win32" ? ".exe" : "";
const source = join(tauriRoot, "target/release", `qingyu-mcp${executableSuffix}`);
const destinationDirectory = join(tauriRoot, "binaries");
const destination = join(
  destinationDirectory,
  `qingyu-mcp-${targetTriple}${executableSuffix}`
);

mkdirSync(destinationDirectory, { recursive: true });
copyFileSync(source, destination);
if (process.platform !== "win32") chmodSync(destination, 0o755);

process.stdout.write(`Prepared ${destination}\n`);
