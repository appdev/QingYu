# QingYu Product Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the current downstream fork into an independently installable and runnable QingYu product without reading SiYuan's global configuration, claiming SiYuan's fixed ports or protocol, or launching a `SiYuan-Kernel` binary.

**Architecture:** Preserve SiYuan's existing multi-workspace and internal compatibility architecture while replacing only externally visible and collision-causing identity values. The main repository owns the shared kernel, desktop/Web identity, ports, protocol generation, packaging and localized product copy; the separately cloned Android repository consumes the shared kernel and performs a full Java Namespace/package migration to `com.apkdv.qingyu`.

**Tech Stack:** Go 1.26, Cobra, Gin, TypeScript, Electron 42, webpack, pnpm 11.12.0, Android Gradle Plugin 9.1.0, Java, Gradle.

## Global Constraints

- Chinese product name is `轻语`; non-Chinese product name is `QingYu`.
- Application ID is `com.apkdv.qingyu`; Android Debug uses `com.apkdv.qingyu.debug`.
- Registered operating-system protocol is `qingyu://`; never register or claim `siyuan://`.
- Global configuration is `~/.config/qingyu`; Electron data is `QingYu-Electron`; CLI default workspace is `~/QingYu` subject to the existing macOS platform-specific Application Support location.
- Fixed kernel proxy port is `9806`; default publish port is `9808`.
- Keep the existing system-assigned dynamic-port behavior and honor every user-specified port, including ports outside `9XXX`.
- Desktop kernel files are `QingYu-Kernel` and `QingYu-Kernel.exe`; keep the existing resource directory names such as `kernel-darwin-arm64/`.
- Keep `.sy`, `window.siyuan`, `/api/*`, Go Module/import paths, internal IPC/event names and other non-colliding compatibility identifiers.
- Generate new application links with `qingyu://`; continue parsing legacy `siyuan://blocks/...` content links.
- Preserve AGPL text, copyright headers, upstream dependency paths and issue/source attribution links.
- Do not alter the SiYuan official application-update behavior; add only the explicitly requested searchable `TODO(QingYu)` marker. Plugin and marketplace updates are unrelated.
- Preserve the already implemented feature-removal boundary, including MCP Server, S3/WebDAV/local sync and marketplace download statistics.
- Do not hand-edit `app/stage/build/**`, `app/src/types/dist/**`, `app/changelogs/**`, bundled Lute, Pandoc files or generated kernel artifacts other than compiling the explicitly requested `app/kernel/QingYu-Kernel`.
- After i18n edits run `python scripts/check-lang-keys.py`; after frontend edits run `cd app && pnpm run lint`; do not run `pnpm build`.
- Do not compile or restart a kernel except in the final user-authorized build/run verification task.
- Do not commit, push, publish or deploy either repository; the user has not authorized Git writes beyond working-tree edits.

---

## File Structure and Responsibility Map

### Main repository

- `app/src/util/qingyuBranding.test.ts`: static product-identity and branding contracts across source/configuration files.
- `kernel/util/qingyu_identity_test.go`: kernel fixed-port and User-Agent contracts.
- `kernel/conf/qingyu_identity_test.go`: publish-service default-port contract.
- `kernel/util/path.go`, `kernel/util/working.go`, `kernel/util/working_mobile.go`: kernel runtime identity, global configuration, default workspace, fixed port and banner.
- `kernel/cli/cmd/root.go`, `kernel/cli/cmd/workspace.go`: CLI executable name and default workspace.
- `kernel/conf/publish.go`, `kernel/server/proxy/fixedport.go`, `kernel/model/export.go`: publish/fixed-port behavior and removal of hard-coded `6806` assumptions.
- `app/electron/main.js`, `app/electron/window.js`, `app/electron/*.html`: Electron application identity, configuration isolation, protocol registration, kernel launch and startup/error copy.
- `app/src/util/pathName.ts`, `app/src/util/uri.ts` and URI-producing editor/menu files: new QingYu URI generation plus legacy SiYuan block-link parsing.
- `app/package.json`, `app/electron-builder*.yml`, `app/appx/*.xml`, `app/nsis/installer.nsh`, `kernel/versioninfo.json`: packaged application identity.
- `app/stage/manifest.webmanifest`: QingYu PWA name/protocol without links to SiYuan store listings.
- `scripts/*.sh`, `scripts/*.bat`, `.github/workflows/cd.yml`: platform kernel output names and packaging inputs.
- `app/appearance/langs/*.json`, templates and frontend product-copy consumers: localized QingYu presentation.
- `Dockerfile`, `README*.md`, `docs/API*.md`, `.github/CONTRIBUTING*.md`: current runtime/build instructions and default-port examples.
- `kernel/model/updater.go`: one deferred-update location marker only; behavior remains unchanged.

### Android repository

- `app/build.gradle`, `settings.gradle`, `flavors.gradle`: Gradle Namespace, application IDs, archive names and localized flavor names.
- `build.gradle`, `signings.templates.gradle`: QingYu version-variable and signing-configuration names without adding credentials.
- `app/src/main/AndroidManifest.xml`: QingYu component identity, provider authority, task affinity, theme and protocol.
- `app/src/main/java/com/apkdv/qingyu/*.java`: fully migrated source package.
- `app/src/main/res/values*/themes.xml`, `strings.xml`, `xml/*.xml`: QingYu resource names, labels, shortcuts and component metadata.
- `app/proguard-rules.pro`, `buildRelease.gradle`: package-sensitive build rules.
- `README*.md`: Android build/output names while retaining upstream attribution links.

---

### Task 1: Add Failing Main-Repository Identity Contracts

**Files:**
- Modify: `app/src/util/qingyuBranding.test.ts`
- Create: `kernel/util/qingyu_identity_test.go`
- Create: `kernel/conf/qingyu_identity_test.go`

**Interfaces:**
- Consumes: existing source/config files and exported `util.FixedPort`, `util.UserAgent`, `conf.NewPublish()`.
- Produces: executable contracts protecting `QingYu`, `com.apkdv.qingyu`, `~/.config/qingyu`, `9806`, `9808`, `qingyu://` and `QingYu-Kernel`.

- [ ] **Step 1: Extend the Node identity test with exact runtime assertions**

Add a test that reads `app/electron/main.js`, `app/electron/window.js`, `app/package.json`, all six Electron Builder YAML files, the two AppX manifests, `app/nsis/installer.nsh`, the three platform build scripts and `.github/workflows/cd.yml`. Assert the new values exist and the collision-causing values do not remain in runtime fields.

```ts
test("QingYu runtime identity is isolated from SiYuan", async () => {
    const electronMain = (await readRepositoryFile("app/electron/main.js")).toString();
    const packageJSON = (await readRepositoryFile("app/package.json")).toString();

    assert.match(electronMain, /\.config", "qingyu"/);
    assert.match(electronMain, /QingYu-Electron/);
    assert.match(electronMain, /com\.apkdv\.qingyu/);
    assert.match(electronMain, /let kernelPort = 9806/);
    assert.match(electronMain, /QingYu-Kernel/);
    assert.match(electronMain, /setAsDefaultProtocolClient\("qingyu"/);
    assert.doesNotMatch(electronMain, /\.config", "siyuan"/);
    assert.doesNotMatch(electronMain, /setAsDefaultProtocolClient\("siyuan"/);
    assert.match(packageJSON, /"name": "QingYu"/);
    assert.match(packageJSON, /"desktopName": "com\.apkdv\.qingyu"/);
});
```

For packaging files, assert `productName: "QingYu"`, `appId: "com.apkdv.qingyu"`, `qingyu-${version}`, `QingYu-Kernel`, and `qingyu` protocol. Do not ban lowercase `siyuan` globally because upstream URLs and internal compatibility identifiers are intentional.

- [ ] **Step 2: Add the kernel default tests**

Create `kernel/util/qingyu_identity_test.go`:

```go
package util

import "testing"

func TestQingYuKernelIdentityDefaults(t *testing.T) {
	if FixedPort != "9806" {
		t.Fatalf("FixedPort = %q, want 9806", FixedPort)
	}
	if UserAgent != "QingYu/"+Ver {
		t.Fatalf("UserAgent = %q, want QingYu/%s", UserAgent, Ver)
	}
}
```

Create `kernel/conf/qingyu_identity_test.go`:

```go
package conf

import "testing"

func TestQingYuPublishPortDefault(t *testing.T) {
	if got := NewPublish().Port; got != 9808 {
		t.Fatalf("NewPublish().Port = %d, want 9808", got)
	}
}
```

- [ ] **Step 3: Run the focused tests and confirm the red state**

Run:

```bash
cd app && pnpm test -- src/util/qingyuBranding.test.ts
cd ../kernel && go test ./util ./conf -run 'TestQingYu(KernelIdentityDefaults|PublishPortDefault)' -count=1
```

Expected: Node assertions fail on SiYuan runtime identity and Go tests fail with `6806`, `6808` and `SiYuan/<version>` values.

- [ ] **Step 4: Review the test boundary**

Confirm the tests do not reject copyright headers, `github.com/siyuan-note/*`, `window.siyuan`, `/api/*`, `.sy`, internal `siyuan-*` event names or legacy URI parsing.

---

### Task 2: Implement Kernel and Electron Runtime Isolation

**Files:**
- Modify: `kernel/util/path.go`
- Modify: `kernel/util/working.go`
- Modify: `kernel/util/working_mobile.go`
- Modify: `kernel/cli/cmd/root.go`
- Modify: `kernel/cli/cmd/workspace.go`
- Modify: `kernel/cli/cmd/serve.go`
- Modify: `kernel/conf/publish.go`
- Modify: `kernel/server/proxy/fixedport.go`
- Modify: `kernel/model/export.go`
- Modify: `kernel/versioninfo.json`
- Modify: `app/electron/main.js`
- Modify: `app/electron/window.js`
- Modify: `app/electron/boot.html`
- Modify: `app/electron/error.html`
- Modify: `app/electron/init.html`
- Modify: `app/electron/workspace.html`
- Modify: `app/package.json`
- Modify: `app/stage/manifest.webmanifest`

**Interfaces:**
- Consumes: Task 1 contracts.
- Produces: independently namespaced QingYu desktop/kernel startup, configuration paths, default workspaces and fixed ports.

- [ ] **Step 1: Change kernel identity constants and defaults**

Apply these exact changes:

```go
// kernel/util/path.go
UserAgent = "QingYu/" + Ver

// kernel/util/working.go
FixedPort = "9806"
userHomeConfDir := filepath.Join(HomeDir, ".config", "qingyu")
defaultWorkspaceDir := filepath.Join(HomeDir, "QingYu")
```

Keep the existing platform branching, changing only the terminal directory component from `SiYuan` to `QingYu`, including macOS `~/Library/Application Support/QingYu`. Change the mobile boot figure to `QingYu`, CLI `Use` to `QingYu-Kernel`, CLI/workspace fallback paths to `QingYu`, and publish default to `9808`.

Use `util.FixedPort` instead of literal `6806` in export URL cleanup so future fixed-port changes cannot diverge. Update comments that describe the active product/port, but preserve license headers and upstream issue URLs.

- [ ] **Step 2: Isolate Electron global state and kernel startup**

In `app/electron/main.js`, make the runtime values exact:

```js
const confDir = path.join(app.getPath("home"), ".config", "qingyu");
let kernelPort = 9806;
app.setPath("userData", path.join(app.getPath("appData"), "QingYu-Electron"));
app.setAppUserModelId("com.apkdv.qingyu");
const kernelName = "win32" === process.platform ? "QingYu-Kernel.exe" : "QingYu-Kernel";
```

Register only `qingyu`, change command-line and second-instance protocol detection to `qingyu://`, set window/menu/tray titles to `QingYu`, set desktop User-Agent prefixes to `QingYu/`, and change error text to `轻语`/`QingYu`. Keep internal IPC channel constants such as `siyuan-open-url` unchanged.

In `app/electron/window.js`, change localized product titles and platform-specific default workspace directory components to `QingYu`. In boot/error/init/workspace HTML, change titles, logo alt text, visible version labels and default port to QingYu/`9806`.

In `app/stage/manifest.webmanifest`, set the product/shortcut names to QingYu, the short name to `qingyu`, and the protocol handler to `web+qingyu`. Remove the `related_applications` entries that point to SiYuan's Microsoft Store, Google Play and App Store listings; do not invent QingYu store URLs.

- [ ] **Step 3: Update package metadata without inventing new services**

Set `app/package.json` runtime identity to:

```json
{
  "name": "QingYu",
  "desktopName": "com.apkdv.qingyu"
}
```

Change the product description only if it names SiYuan; do not fabricate a QingYu homepage, support address, signing identity or publisher. Preserve upstream author/copyright/license data.

- [ ] **Step 4: Format Go and run the focused identity tests**

Run:

```bash
cd kernel && gofmt -w util/path.go util/working.go util/working_mobile.go cli/cmd/root.go cli/cmd/workspace.go cli/cmd/serve.go conf/publish.go server/proxy/fixedport.go model/export.go util/qingyu_identity_test.go conf/qingyu_identity_test.go
go test ./util ./conf -run 'TestQingYu(KernelIdentityDefaults|PublishPortDefault)' -count=1
cd ../app && pnpm test -- src/util/qingyuBranding.test.ts
```

Expected: focused Go tests pass. The Node test may remain red only on packaging files intentionally deferred to Task 4; runtime-source assertions must pass.

---

### Task 3: Generate QingYu Links While Preserving Legacy Block-Link Parsing

**Files:**
- Modify: `app/src/util/pathName.ts`
- Modify: `app/src/util/uri.ts`
- Modify: `app/src/block/popover.ts`
- Modify: `app/src/protyle/render/av/action.ts`
- Modify: `app/src/protyle/wysiwyg/index.ts`
- Modify: `app/src/protyle/toolbar/util.ts`
- Modify: `app/src/menus/protyle.ts`
- Modify: `app/src/boot/globalEvent/searchKeydown.ts`
- Modify: `app/src/protyle/util/compatibility.ts`
- Modify: `kernel/treenode/node.go`
- Modify: `kernel/model/import.go`
- Modify: `kernel/model/search.go`
- Modify: `kernel/model/template.go`
- Modify: `kernel/model/export.go`
- Modify: `app/src/util/qingyuBranding.test.ts`
- Modify: `app/stage/manifest.webmanifest`

**Interfaces:**
- Consumes: `qingyu://` operating-system registration from Task 2.
- Produces: `qingyu://blocks/...` for every newly copied/generated application link while recognizing legacy `siyuan://blocks/...` inside existing content.

- [ ] **Step 1: Add red protocol-generation and compatibility assertions**

Extend `qingyuBranding.test.ts` to read every URI producer listed above. Assert all template literals and generated Markdown links use `qingyu://blocks/`. Assert `pathName.ts` accepts `qingyu:`, `web+qingyu:`, legacy `siyuan:` and legacy `web+siyuan:`. Assert Electron source registers/detects only `qingyu://`.

Assert the Web Manifest registers `web+qingyu`, does not register `web+siyuan`, and contains no SiYuan Microsoft Store, Google Play or App Store listing.

The source-level contract must distinguish generation from compatibility; do not use a global `doesNotMatch(/siyuan:\/\//)` assertion.

- [ ] **Step 2: Run the protocol contract and verify it fails on old generators**

Run:

```bash
cd app && pnpm test -- src/util/qingyuBranding.test.ts
```

Expected: FAIL naming at least one `siyuan://blocks/` generator.

- [ ] **Step 3: Update frontend protocol helpers and consumers**

Keep the existing exported function names for plugin compatibility. Make `isSiYuanUriProtocol` return true for both QingYu and legacy schemes:

```ts
return ["qingyu:", "web+qingyu:", "siyuan:", "web+siyuan:"].includes(uriObj.protocol);
```

Add focused helpers in `pathName.ts`:

```ts
export const isAppBlockURI = (value?: string): boolean =>
    Boolean(value && (value.startsWith("qingyu://blocks/") || value.startsWith("siyuan://blocks/")));

export const getAppBlockURI = (id: string): string => `qingyu://blocks/${id}`;
```

Use these helpers in popovers, menus and editor conditions. Replace every new block-link generator with `getAppBlockURI(id)` or an equivalent `qingyu://blocks/` template. Update plugin/bazaar protocol examples to QingYu without renaming internal event names such as `open-siyuan-url-plugin`.

- [ ] **Step 4: Update Go generation and dual parsing**

Change new block URI generation to `qingyu://blocks/`. Where Go trims or detects existing block URIs, accept both prefixes explicitly and preserve current behavior for old content. Avoid changing upstream Go import paths or `.sy` handling.

- [ ] **Step 5: Run focused tests and lint the touched frontend**

Run:

```bash
cd kernel && gofmt -w treenode/node.go model/import.go model/search.go model/template.go model/export.go
go test ./treenode ./model -run 'Test.*(Import|Export|Template|Search|Node)' -count=1
cd ../app && pnpm test -- src/util/qingyuBranding.test.ts
pnpm run lint
```

Expected: protocol contract passes; existing targeted Go tests and frontend lint pass.

---

### Task 4: Rename Packaged Artifacts and Platform Build Inputs

**Files:**
- Modify: `app/electron-builder.yml`
- Modify: `app/electron-builder-arm64.yml`
- Modify: `app/electron-builder-darwin.yml`
- Modify: `app/electron-builder-darwin-arm64.yml`
- Modify: `app/electron-builder-linux.yml`
- Modify: `app/electron-builder-linux-arm64.yml`
- Modify: `app/appx/AppxManifest.xml`
- Modify: `app/appx/AppxManifest-arm64.xml`
- Modify: `app/nsis/installer.nsh`
- Modify: `scripts/darwin-build.sh`
- Modify: `scripts/linux-build.sh`
- Modify: `scripts/win-build.bat`
- Modify: `.github/workflows/cd.yml`
- Modify: `.github/CONTRIBUTING.md`
- Modify: `.github/CONTRIBUTING.zh-CN.md`
- Modify: `kernel/versioninfo.json`
- Modify: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Consumes: runtime kernel filename from Task 2.
- Produces: platform packaging configurations that place `QingYu-Kernel` where Electron expects it and identify the application as QingYu.

- [ ] **Step 1: Complete the red packaging assertions**

Ensure the Task 1 Node test asserts all builder variants use:

```yaml
productName: "QingYu"
appId: "com.apkdv.qingyu"
artifactName: "qingyu-${version}-${os}.${ext}"
```

Arm64 variants retain their `-arm64` suffix. Darwin protocol name is `QingYu` with scheme `qingyu`; Linux uses `executableName: "qingyu"` and desktop `Name: "QingYu"`; Windows shortcut names use `QingYu`.

- [ ] **Step 2: Update all Electron Builder variants**

Apply the values above without changing architecture targets, bundled resource directories, icons, compression or output directories. Keep existing signing identity/profile inputs unchanged because no QingYu signing credentials were provided; record this as a packaging residual risk rather than inventing credentials.

- [ ] **Step 3: Update Windows AppX and NSIS identity**

Set AppX identity/display/application/protocol values to QingYu equivalents, including executable `app\QingYu.exe` and protocol `qingyu`. In NSIS, terminate `QingYu.exe` and `QingYu-Kernel.exe`, use `QingYu-install.log`, clean only QingYu updater/config/default-workspace paths, and create any required compatibility hard link with QingYu filenames. Never delete `.config\siyuan` or `%PROFILE%\SiYuan` from the QingYu uninstaller.

- [ ] **Step 4: Update kernel build outputs and CI packaging paths**

Change macOS/Linux outputs to `QingYu-Kernel`, Windows outputs to `QingYu-Kernel.exe`, and workflow `kernel_path` values accordingly. Change Windows helper executable links from `siyuan.exe` to `qingyu.exe` only where the script still requires a helper name. Update contribution build commands to the new binary name.

- [ ] **Step 5: Update Windows version metadata**

In `kernel/versioninfo.json`, change user-visible file/product descriptions and original filename to QingYu values while preserving version numbers and copyright ownership.

- [ ] **Step 6: Run source contracts and shell syntax checks**

Run:

```bash
cd app && pnpm test -- src/util/qingyuBranding.test.ts
cd .. && bash -n scripts/darwin-build.sh scripts/linux-build.sh
git diff --check -- app/electron-builder*.yml app/appx app/nsis scripts .github/workflows/cd.yml kernel/versioninfo.json
```

Expected: identity test passes for all packaging/build files; shell scripts parse; no whitespace errors. Do not run production packaging.

---

### Task 5: Replace User-Visible Product Copy and Current Port Documentation

**Files:**
- Modify: `app/appearance/langs/ar.json`
- Modify: `app/appearance/langs/de.json`
- Modify: `app/appearance/langs/en.json`
- Modify: `app/appearance/langs/es.json`
- Modify: `app/appearance/langs/fr.json`
- Modify: `app/appearance/langs/he.json`
- Modify: `app/appearance/langs/hi.json`
- Modify: `app/appearance/langs/id.json`
- Modify: `app/appearance/langs/it.json`
- Modify: `app/appearance/langs/ja.json`
- Modify: `app/appearance/langs/ko.json`
- Modify: `app/appearance/langs/nl.json`
- Modify: `app/appearance/langs/pl.json`
- Modify: `app/appearance/langs/pt-BR.json`
- Modify: `app/appearance/langs/ru.json`
- Modify: `app/appearance/langs/sk.json`
- Modify: `app/appearance/langs/th.json`
- Modify: `app/appearance/langs/tr.json`
- Modify: `app/appearance/langs/uk.json`
- Modify: `app/appearance/langs/zh-CN.json`
- Modify: `app/appearance/langs/zh-TW.json`
- Modify: `app/src/assets/template/app/index.tpl`
- Modify: `app/src/assets/template/app/window.tpl`
- Modify: `app/src/assets/template/desktop/index.tpl`
- Modify: `app/src/assets/template/mobile/index.tpl`
- Review and modify product-copy matches in: `app/src/assets/scss/protyle/_wysiwyg.scss`
- Review and modify product-copy matches in: `app/src/block/popover.ts`
- Review and modify product-copy matches in: `app/src/emoji/index.ts`
- Review and modify product-copy matches in: `app/src/menus/commonMenuItem.ts`
- Review and modify product-copy matches in: `app/src/menus/dataMigration.ts`
- Review and modify product-copy matches in: `app/src/menus/navigation.ts`
- Review and modify product-copy matches in: `app/src/plugin/platformUtils.ts`
- Review and modify product-copy matches in: `app/src/protyle/export/index.ts`
- Review and modify product-copy matches in: `app/src/protyle/util/compatibility.ts`
- Review and modify product-copy matches in: `app/src/protyle/util/table.ts`
- Review and modify product-copy matches in: `app/src/types/config.d.ts`
- Review and modify product-copy matches in: `app/src/types/index.d.ts`
- Review and modify product-copy matches in: `app/src/util/assets.ts`
- Review and modify product-copy matches in: `app/src/util/functions.ts`
- Review and modify product-copy matches in: `app/src/util/mount.ts`
- Review and modify product-copy matches in: `app/src/util/pathName.ts`
- Review and modify product-copy matches in: `app/src/util/processTitle.ts`
- Modify: `Dockerfile`
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `README.ja.md`
- Modify: `README.tr.md`
- Modify: `docs/API.md`
- Modify: `docs/API.zh-CN.md`
- Modify: `docs/API.ja.md`
- Modify: current build/runtime passages in `.github/CONTRIBUTING.md` and `.github/CONTRIBUTING.zh-CN.md`

**Interfaces:**
- Consumes: QingYu runtime names and ports from Tasks 2–4.
- Produces: consistent visible `轻语`/`QingYu` copy and current `9806`/`9808` instructions without rewriting protected generated or upstream-attribution content.

- [ ] **Step 1: Capture a categorized pre-change inventory**

Run:

```bash
rg -n '思源笔记|\bSiYuan\b|思源|\b6806\b|\b6808\b' app/appearance/langs app/src/assets/template app/src app/electron Dockerfile README*.md docs/API*.md .github/CONTRIBUTING*.md \
  --glob '!app/src/asset/pdf/**' --glob '!app/changelogs/**' --glob '!app/stage/build/**'
```

Classify each result as product copy, current-port documentation, protected upstream attribution, internal compatibility, or unrelated numeric text. Do not edit copyright headers, issue URLs, `github.com/siyuan-note`, `.sy`, `window.siyuan` or generated changelogs.

- [ ] **Step 2: Update all language JSON product copy**

Within JSON string values, replace product-name use with `轻语` for `zh-CN.json` and `zh-TW.json`, and `QingYu` for all other languages. Change fixed-port explanations and links from `6806` to `9806`, and publish defaults from `6808` to `9808` where present. Preserve lowercase upstream URLs and internal keys.

Use three ASCII periods for localized ellipses and do not reorder JSON keys. Review every changed line rather than applying an unrestricted repository-wide replacement.

- [ ] **Step 3: Update templates and frontend-visible product copy**

Change HTML titles, process titles, exported-document producer labels and visible help/status text to QingYu. Keep `window.siyuan`, plugin event contracts, CSS class names and PDF.js upstream internals unchanged. Do not hand-edit generated `app/stage/build/**`.

- [ ] **Step 4: Update current runtime/build documentation**

Change the Docker exposed/default port and current invocation examples to `9806`; change API endpoints to `http://127.0.0.1:9806`; rename current product/build commands and binary names to QingYu. Preserve upstream repository links and license attribution. Historical changelogs remain untouched.

- [ ] **Step 5: Validate language structure and visible-brand residue**

Run:

```bash
python scripts/check-lang-keys.py
cd app && pnpm run lint
cd .. && rg -n '思源笔记|\bSiYuan\b|思源|\b6806\b|\b6808\b' app/appearance/langs app/src/assets/template app/src app/electron Dockerfile README*.md docs/API*.md .github/CONTRIBUTING*.md \
  --glob '!app/src/asset/pdf/**' --glob '!app/changelogs/**' --glob '!app/stage/build/**'
```

Expected: language-key check and lint pass. Every remaining search hit is explicitly one of: copyright/upstream attribution, approved internal compatibility, a historical reference, or the deferred official updater. Fix any remaining current product copy or default-port instruction.

---

### Task 6: Add the Deferred Official-Updater Location Marker

**Files:**
- Modify: `kernel/model/updater.go`
- Modify: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Consumes: approved decision to leave official update behavior unchanged.
- Produces: exactly one searchable `TODO(QingYu)` marker identifying the future application-update replacement boundary.

- [ ] **Step 1: Add a red static assertion for the marker and preserved behavior**

Extend `qingyuBranding.test.ts` to assert `kernel/model/updater.go` contains exactly one `TODO(QingYu)` marker and still contains `util.GetRhyResult`, the official `siyuan-` package prefix and official release URLs. This prevents the marker task from silently changing behavior.

- [ ] **Step 2: Run the test and verify it fails only because the marker is absent**

Run:

```bash
cd app && pnpm test -- src/util/qingyuBranding.test.ts
```

Expected: FAIL on marker count; preservation assertions pass.

- [ ] **Step 3: Add the exact Chinese marker near `getUpdatePkg`**

Add a wrapped comment no longer than 120 characters per line:

```go
// TODO(QingYu): 自有发布服务上线前，替换思源版本接口、安装包命名、下载地址、校验与安装启动链路，
// 并同步调整设置页中的检查更新和自动下载入口；插件、主题和在线集市更新不属于此范围。
```

Do not change any updater function, route, setting, URL or package-building expression.

- [ ] **Step 4: Run the marker contract and inspect the behavior diff**

Run:

```bash
cd kernel && gofmt -w model/updater.go
cd ../app && pnpm test -- src/util/qingyuBranding.test.ts
cd .. && git diff --word-diff=plain -- kernel/model/updater.go
```

Expected: test passes and the updater diff contains only the two comment lines.

---

### Task 7: Clone and Fully Migrate the Android Application Namespace

**Files:**
- Create repository: `/Volumes/extendData/Data/IdeaProjects/siyuan-android`
- Modify: `siyuan-android/app/build.gradle`
- Modify: `siyuan-android/build.gradle`
- Modify: `siyuan-android/settings.gradle`
- Modify: `siyuan-android/flavors.gradle`
- Modify: `siyuan-android/signings.templates.gradle`
- Modify: `siyuan-android/app/src/main/AndroidManifest.xml`
- Move: `siyuan-android/app/src/main/java/org/b3log/siyuan/*.java` → `siyuan-android/app/src/main/java/com/apkdv/qingyu/*.java`
- Modify: moved Java files
- Modify: `siyuan-android/app/src/main/res/values/themes.xml`
- Modify: `siyuan-android/app/src/main/res/values-night/themes.xml`
- Modify: `siyuan-android/app/src/main/res/values/strings.xml`
- Modify: `siyuan-android/app/src/main/res/values-zh/strings.xml`
- Modify: `siyuan-android/app/src/main/res/values-ru-rRU/strings.xml`
- Modify: `siyuan-android/app/src/main/res/xml/shortcuts.xml`
- Verify unchanged because the current upstream files contain no product package identity: `siyuan-android/app/proguard-rules.pro`
- Verify unchanged because the current upstream file is empty: `siyuan-android/buildRelease.gradle`
- Modify: `siyuan-android/README.md`
- Modify: `siyuan-android/README.zh-CN.md`
- Modify: `siyuan-android/README.ja.md`

**Interfaces:**
- Consumes: user authorization to clone `siyuan-note/siyuan-android`; shared kernel fixed port `9806` from Task 2.
- Produces: Android source/build identity fully rooted at `com.apkdv.qingyu`, QingYu labels/artifacts and `qingyu://` deep links.

- [ ] **Step 1: Clone the exact authorized repository and load its rules**

Before cloning, verify `/Volumes/extendData/Data/IdeaProjects/siyuan-android` does not exist. Then run:

```bash
git clone https://github.com/siyuan-note/siyuan-android.git /Volumes/extendData/Data/IdeaProjects/siyuan-android
```

Read the cloned repository's `AGENTS.md` if present and its README/build instructions. Record `git rev-parse HEAD` and `git status --short`. If an `AGENTS.md` rule contradicts this plan, stop this task and report the exact conflict before editing.

- [ ] **Step 2: Establish the red identity inventory**

Run from the Android repository:

```bash
rg -n 'org\.b3log\.siyuan|package org\.b3log\.siyuan|Theme\.SiYuan|siyuan://|archivesName = "siyuan|fileName = "siyuan|思源笔记|\bSiYuan\b' \
  app/src app/build.gradle settings.gradle flavors.gradle app/proguard-rules.pro buildRelease.gradle README*.md
```

Expected: matches include Gradle Namespace/application IDs, every Java package declaration, Manifest theme/task/protocol values, flavor labels and archive names.

- [ ] **Step 3: Change Gradle application and artifact identity**

Set:

```gradle
// build.gradle
ext {
    qingyuVersionCode = 345
    qingyuVersionName = "3.8.0-beta.2"
}

// app/build.gradle
base {
    archivesName = "qingyu-${qingyuVersionName}"
}

android {
    namespace 'com.apkdv.qingyu'
    defaultConfig {
        applicationId "com.apkdv.qingyu"
    }
}
```

Keep the version values and channel names, but rename `siyuanVersionCode`/`siyuanVersionName` to `qingyuVersionCode`/`qingyuVersionName`. Change output filenames to `qingyu-${versionName}-${flavorName}-${buildType}.apk`, release `app_package_name` to `com.apkdv.qingyu`, Debug to `com.apkdv.qingyu.debug`, root project name to `QingYu`, Chinese flavor labels to `轻语`, and non-Chinese flavor labels to `QingYu`. In `signings.templates.gradle`, rename `siyuanConfig` to `qingyuConfig` and the example keystore filename to `qingyu-android.jks`; update the release build to reference `signingConfigs.qingyuConfig`. Do not invent or commit signing credentials.

- [ ] **Step 4: Move and rewrite the Java package tree**

Use a Git-aware directory move:

```bash
mkdir -p app/src/main/java/com/apkdv
git mv app/src/main/java/org/b3log/siyuan app/src/main/java/com/apkdv/qingyu
```

Change every moved file's package declaration from `org.b3log.siyuan` to `com.apkdv.qingyu`, plus imports, fully qualified class names, reflection targets, intent actions, notification channels or authorities that use the old runtime package. Preserve upstream issue URLs and copyright comments.

- [ ] **Step 5: Update Manifest and resources**

Use relative component names under the new Namespace. Change task affinity to `com.apkdv.qingyu.shorthand`, deep-link scheme to `qingyu`, FileProvider authority to the `app_package_name` resource, and every `Theme.SiYuan` reference/name to `Theme.QingYu`. Change visible Chinese names to `轻语` and non-Chinese names to `QingYu`. Update shortcuts or resource XML only when they contain old package/product identity.

- [ ] **Step 6: Update package-sensitive build rules and Android documentation**

Verify ProGuard and the currently empty release-helper script contain no old product package reference. Update build output examples from `siyuan-*` to `qingyu-*` and product text to QingYu/轻语 while retaining upstream source links and license attribution.

- [ ] **Step 7: Run namespace residue checks**

Run:

```bash
test -d app/src/main/java/com/apkdv/qingyu
test ! -d app/src/main/java/org/b3log/siyuan
! rg -n 'org\.b3log\.siyuan|package org\.b3log\.siyuan|Theme\.SiYuan|android:scheme="siyuan"|siyuanVersion|siyuanConfig' \
  app/src app/build.gradle build.gradle settings.gradle flavors.gradle signings.templates.gradle app/proguard-rules.pro buildRelease.gradle
rg -n 'com\.apkdv\.qingyu|Theme\.QingYu|android:scheme="qingyu"|archivesName = "qingyu|fileName = "qingyu|qingyuVersion|qingyuConfig' \
  app/src app/build.gradle build.gradle settings.gradle flavors.gradle signings.templates.gradle
git diff --check
```

Expected: old runtime/Namespace checks return no matches; new identity checks name every required layer.

- [ ] **Step 8: Run the narrowest available Gradle configuration check**

Create local untracked `signings.gradle` from the repository's template only when it is absent; never stage or commit it. Run:

```bash
test -f signings.gradle || cp signings.templates.gradle signings.gradle
./gradlew :app:tasks --all
```

If the checked-out repository already has the required `kernel.aar` and staged Web assets, additionally run `./gradlew :app:processOfficialDebugMainManifest`. Otherwise record that Android compilation remains blocked specifically on those generated inputs, which are intentionally deferred with device integration.

---

### Task 8: Final Cross-Repository Verification and Isolated Desktop Run

**Files:**
- Verify all modified files in both repositories.
- Generate: `app/kernel/QingYu-Kernel` for the authorized local run.
- Remove exact obsolete generated file if present: `app/kernel/SiYuan-Kernel`.

**Interfaces:**
- Consumes: all previous tasks.
- Produces: evidence that QingYu builds and starts from an isolated workspace without touching SiYuan configuration, plus an explicit Android/static-platform residual-risk report.

- [ ] **Step 1: Freeze and inspect both working-tree scopes**

Run `git status --short` and `git diff --stat` in both repositories. Confirm main-repository pre-existing feature-removal/icon changes remain present and no Android file outside the identity migration was changed. Do not stage or commit.

- [ ] **Step 2: Run the complete focused verification set**

Run in the main repository:

```bash
cd app && pnpm test
pnpm run lint
cd .. && python scripts/check-lang-keys.py
cd kernel && go test ./util ./conf ./cli/cmd ./treenode ./model ./server/proxy -count=1
cd .. && bash -n scripts/darwin-build.sh scripts/linux-build.sh
git diff --check
```

Run the Task 7 residue and Gradle configuration checks in the Android repository. Report any broader pre-existing test failures separately; do not hide a failure caused by the identity changes.

- [ ] **Step 3: Capture the SiYuan configuration fingerprint before runtime verification**

Read-only capture:

```bash
stat -f '%m %z %N' /Users/ying/.config/siyuan/workspace.json 2>/dev/null || true
shasum -a 256 /Users/ying/.config/siyuan/workspace.json 2>/dev/null || true
lsof -nP -iTCP:6806 -sTCP:LISTEN || true
```

Save the output in the execution notes. Do not start, stop or modify the installed SiYuan application.

- [ ] **Step 4: Build the renamed ARM64 kernel**

From `kernel/` run the user-authorized build:

```bash
go build -tags "fts5 sqlcipher" -o ../app/kernel/QingYu-Kernel .
file ../app/kernel/QingYu-Kernel
```

Expected: successful Mach-O arm64 executable named `QingYu-Kernel`. If the exact obsolete generated `app/kernel/SiYuan-Kernel` from the earlier development run exists, first run `git ls-files --error-unmatch app/kernel/SiYuan-Kernel`. Remove only that exact file when the command confirms it is untracked by returning nonzero; if it is tracked, stop and report instead. Never delete the `app/kernel/` directory.

- [ ] **Step 5: Start QingYu against a fresh temporary workspace**

Create a dedicated directory with `mktemp -d`, start `QingYu-Kernel serve` with `--port 9806`, `--wd` pointing to the repository `app/`, `--mode dev`, `--attach-ui`, `--workspace` pointing to that temporary directory and `--lang zh-CN`. Start frontend webpack with `cd app && pnpm dev`, then start Electron with `pnpm start` after the kernel reports booted.

Expected runtime evidence:

- `curl http://127.0.0.1:9806/api/system/version` returns HTTP 200 and the current version.
- Process list contains `QingYu-Kernel`, not `SiYuan-Kernel`.
- Electron window title/product menu shows QingYu/轻语.
- QingYu creates/reads `~/.config/qingyu`, not `~/.config/siyuan`.
- Computer-use inspection shows the QingYu first-run/editor surface without displaying the user's original SiYuan workspace.

Stop Electron, webpack and the kernel gracefully after collecting evidence.

- [ ] **Step 6: Verify explicit non-9XXX port behavior**

Using a second fresh temporary workspace, start only `QingYu-Kernel serve` with an available explicit port outside `9XXX`, for example `8806`. Confirm `/api/system/version` responds on that exact port, then stop the kernel gracefully. This proves user-selected ports remain unrestricted.

- [ ] **Step 7: Prove the original SiYuan configuration was untouched**

Repeat the Step 3 `stat`, `shasum` and `lsof` checks. The SiYuan workspace configuration fingerprint must be unchanged. Any pre-existing listener on `6806` must remain outside QingYu's process tree; QingYu's fixed listener is `9806`.

- [ ] **Step 8: Produce the handoff report**

Report:

- exact changed identity values and both repository paths;
- test/lint/i18n/build/runtime evidence;
- removal of the obsolete generated `SiYuan-Kernel` file, if performed, and that it was recoverable by rebuilding;
- Android Gradle status and any missing generated `kernel.aar`/Web assets;
- untested Windows/Linux/Docker packaging and Android device behavior;
- unchanged official application updater behavior and the location of `TODO(QingYu)`;
- no commit, push, publish or deployment performed.
