import fs from "node:fs";
import path from "node:path";

const gtkHookRelativePath = path.join("apprun-hooks", "linuxdeploy-plugin-gtk.sh");
const hostGtkInputMethodPolicyMarker = "MARKRA_SYSTEM_GTK_IM_MODULE_FILE_CANDIDATES";
const fcitxXimFallbackMarker = "MARKRA_FCITX_XIM_FALLBACK";
const archGtkModulePath = "/usr/lib/gtk-3.0";
const archGtkInputMethodCache = "/usr/lib/gtk-3.0/3.0.0/immodules.cache";

function usage() {
  return [
    "Usage: node scripts/release/verify-linux-appimage-gtk-ime.mjs <AppDir>",
    "",
    "APPIMAGE_APPDIR may be used instead of the positional argument.",
  ].join("\n");
}

function readGtkPathEntries(content) {
  const match = content.match(/^export GTK_PATH="([^"]*)"$/m);
  if (!match) return [];

  return match[1].split(":").filter(Boolean);
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

const gtkHookPath = path.join(appDir, gtkHookRelativePath);

if (!fs.existsSync(gtkHookPath)) {
  console.error(`GTK AppRun hook not found: ${gtkHookPath}`);
  process.exit(1);
}

const content = fs.readFileSync(gtkHookPath, "utf8");
const errors = [];
const gtkPathEntries = readGtkPathEntries(content);

if (/^export GTK_IM_MODULE_FILE="\$APPDIR\//m.test(content)) {
  errors.push("GTK_IM_MODULE_FILE points inside the AppImage instead of a host GTK input method cache.");
}

if (!content.includes(hostGtkInputMethodPolicyMarker)) {
  errors.push("Host GTK input method cache discovery block is missing.");
}

if (!content.includes(archGtkInputMethodCache)) {
  errors.push(`Host GTK input method cache candidates must include ${archGtkInputMethodCache}.`);
}

if (!gtkPathEntries.includes(archGtkModulePath)) {
  errors.push(`GTK_PATH must include ${archGtkModulePath} for Arch and Manjaro GTK modules.`);
}

if (!content.includes(fcitxXimFallbackMarker) || !/^ *export GTK_IM_MODULE=xim$/m.test(content)) {
  errors.push("Fcitx XIM fallback is missing from the Linux AppImage GTK AppRun hook.");
}

if (errors.length > 0) {
  console.error("Linux AppImage GTK input method policy failed:");
  for (const error of errors) {
    console.error(`- ${error}`);
  }
  process.exit(1);
}

console.log(`Linux AppImage GTK input method policy looks host-compatible in ${gtkHookPath}.`);
