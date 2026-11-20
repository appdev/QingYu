import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const tauriDir = fileURLToPath(new URL("../src-tauri", import.meta.url));

function optionValue(args, ...names) {
  const index = args.findIndex(arg => names.some(name => arg === name || arg.startsWith(name + "=")));
  if (index === -1) return undefined;
  return args[index].includes("=") ? args[index].slice(args[index].indexOf("=") + 1) : args[index + 1];
}

function debsAt(targetDir, target, profile) {
  const debs = new Map();
  if (!existsSync(targetDir)) return debs;
  // Cargo may put a configured target triple between the output root and the profile.
  const roots = target ? [join(targetDir, target)] : [
    targetDir,
    ...readdirSync(targetDir, { withFileTypes: true })
      .filter(entry => entry.isDirectory())
      .map(entry => join(targetDir, entry.name)),
  ];
  for (const root of roots) {
    const dir = join(root, profile, "bundle", "deb");
    if (!existsSync(dir)) continue;
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (!entry.isFile() || !entry.name.endsWith(".deb")) continue;
      const path = join(dir, entry.name);
      debs.set(path, statSync(path, { bigint: true }).ctimeNs);
    }
  }
  return debs;
}

function setDebPackage(path, identifier) {
  const temp = mkdtempSync(join(dirname(path), ".deb-package-"));
  try {
    const tree = join(temp, "tree");
    const output = join(temp, "package.deb");
    execFileSync("dpkg-deb", ["--raw-extract", path, tree]);
    const control = join(tree, "DEBIAN", "control");
    const original = readFileSync(control, "utf8");
    const field = /^Package:[^\r\n]*$/m;
    if (!field.test(original)) throw new Error(`Missing Package field in ${path}`);
    writeFileSync(control, original.replace(field, () => `Package: ${identifier}`));
    execFileSync("dpkg-deb", ["--root-owner-group", "--build", tree, output]);
    const name = execFileSync("dpkg-deb", ["--field", output, "Package"], { encoding: "utf8" }).trim();
    if (name !== identifier) throw new Error(`Expected Package: ${identifier}, got ${name}`);
    renameSync(output, path);
    console.log(`Debian Package: ${name} (${path})`);
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
}

export function prepareDebPackaging(options, cargoArgs, debug) {
  const targetOverride = optionValue(cargoArgs, "--target-dir");
  const targetDir = targetOverride ? resolve(tauriDir, targetOverride) : JSON.parse(
    execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], { cwd: tauriDir, encoding: "utf8" }),
  ).target_directory;
  const target = optionValue(options, "--target", "-t");
  const cargoProfile = optionValue(cargoArgs, "--profile") ?? (debug ? "dev" : "release");
  const profile = cargoProfile === "dev" ? "debug" : cargoProfile;
  const before = debsAt(targetDir, target, profile);
  const expectsDeb = options.some(option => option.replace(/^(--bundles|-b)=/, "").split(",").includes("deb"));
  return identifier => {
    const built = [...debsAt(targetDir, target, profile)].filter(([path, changedAt]) => before.get(path) !== changedAt);
    if (expectsDeb && built.length === 0) throw new Error(`No new Debian package found in ${targetDir}`);
    for (const [path] of built) setDebPackage(path, identifier);
  };
}
