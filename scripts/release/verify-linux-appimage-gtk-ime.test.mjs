import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));

function makeTempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "markra-appimage-ime-policy-"));
}

function writeFile(filePath, content = "") {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, content);
}

function writeGtkHook(appDir, content) {
  writeFile(path.join(appDir, "apprun-hooks", "linuxdeploy-plugin-gtk.sh"), content);
}

function runVerify(appDir) {
  return spawnSync(process.execPath, ["scripts/release/verify-linux-appimage-gtk-ime.mjs", appDir], {
    cwd: repoRoot,
    encoding: "utf8",
  });
}

test("verify-linux-appimage-gtk-ime accepts a hook that uses host GTK input method caches", () => {
  const appDir = makeTempDir();
  writeGtkHook(appDir, `#! /usr/bin/env bash
export GTK_PATH="$APPDIR//usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib64/gtk-3.0:/usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib/gtk-3.0"
MARKRA_SYSTEM_GTK_IM_MODULE_FILE_CANDIDATES=(
  "/usr/lib/gtk-3.0/3.0.0/immodules.cache"
)
for gtk_im_module_file in "\${MARKRA_SYSTEM_GTK_IM_MODULE_FILE_CANDIDATES[@]}"; do
  if [ -r "$gtk_im_module_file" ]; then
    export GTK_IM_MODULE_FILE="$gtk_im_module_file"
    break
  fi
done
if [ "\${XMODIFIERS:-}" = "@im=fcitx" ]; then
  case "\${GTK_IM_MODULE:-}" in
    ""|"fcitx"|"fcitx5")
      export MARKRA_FCITX_XIM_FALLBACK=1
      export GTK_IM_MODULE=xim
      ;;
  esac
fi
`);

  const result = runVerify(appDir);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Linux AppImage GTK input method policy looks host-compatible/);
});

test("verify-linux-appimage-gtk-ime rejects AppImage-local GTK input method caches", () => {
  const appDir = makeTempDir();
  writeGtkHook(appDir, `#! /usr/bin/env bash
export GTK_PATH="$APPDIR//usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib64/gtk-3.0:/usr/lib/x86_64-linux-gnu/gtk-3.0"
export GTK_IM_MODULE_FILE="$APPDIR//usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache"
`);

  const result = runVerify(appDir);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /GTK_IM_MODULE_FILE points inside the AppImage/);
});

test("verify-linux-appimage-gtk-ime rejects hooks without the Arch GTK module path", () => {
  const appDir = makeTempDir();
  writeGtkHook(appDir, `#! /usr/bin/env bash
MARKRA_SYSTEM_GTK_IM_MODULE_FILE_CANDIDATES=(
  "/usr/lib/gtk-3.0/3.0.0/immodules.cache"
)
`);

  const result = runVerify(appDir);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /\/usr\/lib\/gtk-3\.0/);
});

test("verify-linux-appimage-gtk-ime rejects hooks without the Fcitx XIM fallback", () => {
  const appDir = makeTempDir();
  writeGtkHook(appDir, `#! /usr/bin/env bash
export GTK_PATH="$APPDIR//usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib64/gtk-3.0:/usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib/gtk-3.0"
MARKRA_SYSTEM_GTK_IM_MODULE_FILE_CANDIDATES=(
  "/usr/lib/gtk-3.0/3.0.0/immodules.cache"
)
`);

  const result = runVerify(appDir);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /Fcitx XIM fallback/);
});
