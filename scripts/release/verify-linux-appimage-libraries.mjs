import fs from "node:fs";
import path from "node:path";

const forbiddenLibraryPrefixes = ["libwayland-client.so"];

function usage() {
  return [
    "Usage: node scripts/release/verify-linux-appimage-libraries.mjs <AppDir>",
    "",
    "APPIMAGE_APPDIR may be used instead of the positional argument.",
  ].join("\n");
}

function walkFiles(root) {
  const pending = [root];
  const files = [];

  while (pending.length > 0) {
    const current = pending.pop();
    const stat = fs.lstatSync(current);

    if (stat.isDirectory()) {
      for (const entry of fs.readdirSync(current)) {
        pending.push(path.join(current, entry));
      }
      continue;
    }

    files.push(current);
  }

  return files;
}

function isForbiddenLibrary(filePath) {
  const basename = path.basename(filePath);
  return forbiddenLibraryPrefixes.some((prefix) => basename.startsWith(prefix));
}

const appDir = process.argv[2] || process.env.APPIMAGE_APPDIR;

if (!appDir) {
  console.error(usage());
  process.exit(2);
}

if (!fs.existsSync(appDir)) {
  console.error(`AppDir not found: ${appDir}`);
  process.exit(2);
}

const forbiddenLibraries = walkFiles(appDir).filter(isForbiddenLibrary).sort();

if (forbiddenLibraries.length > 0) {
  console.error("Forbidden Linux AppImage libraries were bundled:");
  for (const filePath of forbiddenLibraries) {
    console.error(`- ${filePath}`);
  }
  console.error("");
  console.error("Keep libwayland-client on the host system to avoid Wayland/Mesa ABI conflicts.");
  process.exit(1);
}

console.log(`No forbidden AppImage libraries found in ${appDir}.`);
