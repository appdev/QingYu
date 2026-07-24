import fs from "node:fs";
import path from "node:path";

const gtkHookRelativePath = path.join("apprun-hooks", "linuxdeploy-plugin-gtk.sh");
const archGtkModulePath = "/usr/lib/gtk-3.0";
const hostGtkInputMethodPolicyMarker = "MARKRA_SYSTEM_GTK_IM_MODULE_FILE_CANDIDATES";
const fcitxXimFallbackMarker = "MARKRA_FCITX_XIM_FALLBACK";

const hostGtkInputMethodPolicy = `# QingYu: keep Linux AppImage input methods compatible with host IBus/Fcitx.
# Tauri's GTK hook is generated on Ubuntu and can point GTK_IM_MODULE_FILE at
# the AppImage-local immodules.cache, which hides Manjaro/Arch/Fedora host IMEs.
${hostGtkInputMethodPolicyMarker}=(
  "/usr/lib/gtk-3.0/3.0.0/immodules.cache"
  "/usr/lib64/gtk-3.0/3.0.0/immodules.cache"
  "/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache"
  "/etc/gtk-3.0/gtk.immodules"
)
for gtk_im_module_file in "\${${hostGtkInputMethodPolicyMarker}[@]}"; do
  if [ -r "$gtk_im_module_file" ]; then
    export GTK_IM_MODULE_FILE="$gtk_im_module_file"
    break
  fi
done`;

const fcitxXimFallbackPolicy = `# QingYu: Fcitx GTK immodules can fail to load from AppImage-bundled GTK.
# Use the XIM fallback that works with host Fcitx on Manjaro/GNOME Wayland.
if [ "\${XMODIFIERS:-}" = "@im=fcitx" ]; then
  case "\${GTK_IM_MODULE:-}" in
    ""|"fcitx"|"fcitx5")
      export ${fcitxXimFallbackMarker}=1
      export GTK_IM_MODULE=xim
      ;;
  esac
fi`;

function usage() {
  return [
    "Usage: node scripts/release/repair-linux-appimage-gtk-ime.mjs <AppDir>",
    "",
    "APPIMAGE_APPDIR may be used instead of the positional argument.",
  ].join("\n");
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

function repairGtkPath(content) {
  let foundGtkPath = false;

  const repaired = content.replace(/^export GTK_PATH="([^"]*)"$/m, (line, value) => {
    foundGtkPath = true;
    const entries = value.split(":").filter(Boolean);
    if (entries.includes(archGtkModulePath)) return line;

    return `export GTK_PATH="${[...entries, archGtkModulePath].join(":")}"`;
  });

  if (!foundGtkPath) {
    fail("GTK_PATH export not found in the Linux AppImage GTK AppRun hook.");
  }

  return repaired;
}

function repairGtkInputMethodCache(content) {
  if (content.includes(hostGtkInputMethodPolicyMarker)) {
    return content;
  }

  const internalCachePattern = /^export GTK_IM_MODULE_FILE="\$APPDIR\/[^"]*immodules\.cache"$/m;

  if (!internalCachePattern.test(content)) {
    fail("GTK_IM_MODULE_FILE AppImage-local cache export not found in the Linux AppImage GTK AppRun hook.");
  }

  return content.replace(internalCachePattern, hostGtkInputMethodPolicy);
}

function repairFcitxXimFallback(content) {
  if (content.includes(fcitxXimFallbackMarker)) {
    return content;
  }

  if (content.includes(hostGtkInputMethodPolicy)) {
    return content.replace(hostGtkInputMethodPolicy, `${hostGtkInputMethodPolicy}\n${fcitxXimFallbackPolicy}`);
  }

  return `${content.trimEnd()}\n${fcitxXimFallbackPolicy}\n`;
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
  fail(`GTK AppRun hook not found: ${gtkHookPath}`);
}

const originalHook = fs.readFileSync(gtkHookPath, "utf8");
const repairedHook = repairFcitxXimFallback(repairGtkInputMethodCache(repairGtkPath(originalHook)));

if (repairedHook !== originalHook) {
  fs.writeFileSync(gtkHookPath, repairedHook);
  console.log(`Repaired Linux AppImage GTK input method policy in ${gtkHookPath}.`);
} else {
  console.log(`Linux AppImage GTK input method policy already repaired in ${gtkHookPath}.`);
}
