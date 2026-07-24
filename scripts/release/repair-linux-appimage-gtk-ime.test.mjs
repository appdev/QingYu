import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));

function makeTempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "markra-appimage-ime-repair-"));
}

function writeFile(filePath, content = "") {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, content);
}

function writeGtkHook(appDir, content) {
  const hookPath = path.join(appDir, "apprun-hooks", "linuxdeploy-plugin-gtk.sh");
  writeFile(hookPath, content);
  return hookPath;
}

function runRepair(appDir) {
  return spawnSync(process.execPath, ["scripts/release/repair-linux-appimage-gtk-ime.mjs", appDir], {
    cwd: repoRoot,
    encoding: "utf8",
  });
}

const tauriGtkHook = `#! /usr/bin/env bash
export APPDIR="\${APPDIR:-"\$(dirname "\$(realpath "$0")")"}"
export GTK_EXE_PREFIX="$APPDIR//usr"
export GTK_PATH="$APPDIR//usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib64/gtk-3.0:/usr/lib/x86_64-linux-gnu/gtk-3.0"
export GTK_IM_MODULE_FILE="$APPDIR//usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache"
export GDK_PIXBUF_MODULE_FILE="$APPDIR//usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders.cache"
`;

test("repair-linux-appimage-gtk-ime points GTK input methods at the host module cache", () => {
  const appDir = makeTempDir();
  const hookPath = writeGtkHook(appDir, tauriGtkHook);

  const result = runRepair(appDir);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Repaired Linux AppImage GTK input method policy/);

  const repairedHook = fs.readFileSync(hookPath, "utf8");
  assert.doesNotMatch(repairedHook, /export GTK_IM_MODULE_FILE="\$APPDIR\//);
  assert.match(repairedHook, /MARKRA_SYSTEM_GTK_IM_MODULE_FILE_CANDIDATES/);
  assert.match(repairedHook, /MARKRA_FCITX_XIM_FALLBACK/);
  assert.match(repairedHook, /export GTK_IM_MODULE=xim/);
  assert.match(repairedHook, /\/usr\/lib\/gtk-3\.0\/3\.0\.0\/immodules\.cache/);
  assert.match(repairedHook, /export GTK_PATH="\$APPDIR\/\/usr\/lib\/x86_64-linux-gnu\/gtk-3\.0:\/usr\/lib64\/gtk-3\.0:\/usr\/lib\/x86_64-linux-gnu\/gtk-3\.0:\/usr\/lib\/gtk-3\.0"/);
});

test("repair-linux-appimage-gtk-ime is idempotent", () => {
  const appDir = makeTempDir();
  const hookPath = writeGtkHook(appDir, tauriGtkHook);

  assert.equal(runRepair(appDir).status, 0);
  assert.equal(runRepair(appDir).status, 0);

  const repairedHook = fs.readFileSync(hookPath, "utf8");
  assert.equal((repairedHook.match(/MARKRA_SYSTEM_GTK_IM_MODULE_FILE_CANDIDATES=\(/gu) ?? []).length, 1);
  assert.equal((repairedHook.match(/MARKRA_FCITX_XIM_FALLBACK=1/gu) ?? []).length, 1);
  assert.equal((repairedHook.match(/\/usr\/lib\/gtk-3\.0/gu) ?? []).length, 2);
});

test("repair-linux-appimage-gtk-ime adds the Fcitx XIM fallback to previously repaired hooks", () => {
  const appDir = makeTempDir();
  const hookPath = writeGtkHook(appDir, `#! /usr/bin/env bash
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
`);

  const result = runRepair(appDir);

  assert.equal(result.status, 0, result.stderr);

  const repairedHook = fs.readFileSync(hookPath, "utf8");
  assert.match(repairedHook, /MARKRA_FCITX_XIM_FALLBACK=1/);
  assert.match(repairedHook, /export GTK_IM_MODULE=xim/);
});

test("repair-linux-appimage-gtk-ime fails when the GTK AppRun hook is missing", () => {
  const appDir = makeTempDir();
  writeFile(path.join(appDir, "usr", "bin", "markra"));

  const result = runRepair(appDir);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /GTK AppRun hook not found/);
});
