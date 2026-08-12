# Remove Daily Note and CalDAV Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the complete Daily Note product surface and CalDAV service without changing existing documents, stored CalDAV files, CardDAV, WebDAV, or ordinary date features.

**Architecture:** Delete Daily Note as a vertical slice from UI consumers through API, model, configuration, CLI, MCP, localization, and documentation. Isolate the DAV registration boundary so CalDAV routes and implementation can be removed while the shared CardDAV/WebDAV infrastructure remains testable and unchanged.

**Tech Stack:** Go 1.26, Gin, Cobra, TypeScript, Electron, pnpm 11.12.0, Python 3, SiYuan `.sy` JSON guide sources.

## Global Constraints

- Do not rewrite, migrate, or delete existing user documents or historical `custom-dailynote-*` attributes.
- Do not delete existing files under `data/storage/caldav`; removal is code and service-surface only.
- Preserve CardDAV, WebDAV, generic note creation, templates, Attribute View dates, and non-calendar uses of `iconCalendar`.
- Preserve unrelated dirty-worktree changes, including current MCP, settings-shortcut, branding, packaging, README, and localization edits.
- Do not add compatibility route stubs, feature flags, or data migrations.
- Do not hand-edit generated files under `app/stage/build/**` or `app/src/types/dist/**`.
- Do not run `pnpm build`, `pnpm dev`, compile the kernel binary, or restart a running kernel.
- Use `gofmt` after Go edits, `cd app && pnpm run lint` for frontend verification, and `python scripts/check-lang-keys.py` after localization edits.
- Do not commit, push, publish, deploy, or otherwise perform external writes.

---

### Task 1: Add removal and preservation boundary tests

**Files:**
- Modify: `kernel/api/feature_removal_test.go`
- Modify: `kernel/mcp/feature_boundary_test.go`
- Modify: `kernel/cli/cmd/root_test.go`
- Create: `kernel/server/dav_routes_test.go`
- Modify: `app/src/util/nativeMenu.test.ts`
- Modify: `kernel/server/serve.go`

**Interfaces:**
- Consumes: `api.ServeAPI(*gin.Engine)`, `tools.LookupTool(string)`, Cobra `rootCmd`, Electron native-menu exports, and Gin DAV route registration.
- Produces: executable contracts that reject Daily Note and CalDAV while preserving generic document APIs, MCP tools, CardDAV, and WebDAV.

- [ ] **Step 1: Extend the API and MCP boundary tests**

Add the exact Daily Note routes to the existing removed-route set:

```go
for _, removed := range []string{
	"/api/filetree/createDailyNote",
	"/api/block/appendDailyNoteBlock",
	"/api/block/prependDailyNoteBlock",
} {
	if routes[removed] {
		t.Fatalf("removed daily note route remains registered: %s", removed)
	}
}
```

Add `"dailynote"` to the existing MCP `removed` list. Keep the current `setMCP` preservation assertion and generic MCP tool assertions unchanged.

- [ ] **Step 2: Add CLI and native-menu removal contracts**

Add a CLI test that scans `rootCmd.Commands()` and fails if `command.Name() == "dailynote"`. Update the native-menu test so it asserts `NATIVE_MENU_COMMANDS.has("dailyNote") == false`, `NATIVE_MENU_LABEL_KEYS.includes("dailyNote") == false`, and `findItem(template, "dailyNote") == undefined`. Remove `dailyNote` from the readonly-mode list because the item must no longer exist.

- [ ] **Step 3: Introduce a testable DAV registration seam without changing behavior**

Add this helper and replace the three direct calls in `Serve` with one call:

```go
func serveDAV(ginServer *gin.Engine) {
	serveWebDAV(ginServer)
	serveCalDAV(ginServer)
	serveCardDAV(ginServer)
}
```

This is a temporary behavior-preserving refactor that lets the next test inspect the registered route set.

- [ ] **Step 4: Add the failing DAV route boundary test**

Create `kernel/server/dav_routes_test.go` in package `server`. Register `serveDAV(gin.New())`, collect `engine.Routes()`, fail for `/caldav/*path` and `/.well-known/caldav`, and require `/webdav/*path`, `/carddav/*path`, and `/.well-known/carddav`.

- [ ] **Step 5: Run the red tests**

Run: `cd kernel && go test ./api ./mcp ./cli/cmd ./server -run 'TestRemovedProductRoutesAndPreservedBoundaries|TestMCPServerBoundary|TestDailyNoteCommandRemoved|TestDAVRouteBoundary' -count=1`

Expected: FAIL because the three Daily Note routes, MCP tool, CLI command, and CalDAV routes still exist.

Run: `cd app && pnpm exec tsx --test src/util/nativeMenu.test.ts`

Expected: FAIL because `dailyNote` still exists in the native menu.

---

### Task 2: Remove the Daily Note vertical slice

**Files:**
- Delete: `kernel/cli/cmd/dailynote.go`
- Delete: `kernel/mcp/tools/dailynote.go`
- Modify: `kernel/api/router.go`
- Modify: `kernel/api/filetree.go`
- Modify: `kernel/api/block_op.go`
- Modify: `kernel/api/notebook.go`
- Modify: `kernel/model/file.go`
- Modify: `kernel/model/import.go`
- Modify: `kernel/conf/box.go`
- Modify: `app/src/util/mount.ts`
- Modify: `app/src/menus/workspace.ts`
- Modify: `app/src/mobile/menu/index.ts`
- Modify: `app/src/boot/globalEvent/keydown.ts`
- Modify: `app/src/boot/globalEvent/command/global.ts`
- Modify: `app/src/boot/globalEvent/command/panel.ts`
- Modify: `app/src/boot/globalEvent/commonHotkey.ts`
- Modify: `app/src/boot/nativeMenu.ts`
- Modify: `app/electron/nativeMenu.js`
- Modify: `app/src/layout/dock/Files.ts`
- Modify: `app/src/mobile/dock/MobileFiles.ts`
- Modify: `app/src/menus/onGetnotebookconf.ts`
- Modify: `app/src/constants.ts`
- Modify: `app/src/types/config.d.ts`

**Interfaces:**
- Consumes: removal tests from Task 1 and ordinary document creation/configuration APIs.
- Produces: no Daily Note UI, keymap, API, model function, CLI command, MCP tool, or notebook configuration; existing documents remain normal documents.

- [ ] **Step 1: Remove frontend entry points and command dispatch**

Delete `fetchNewDailyNote` and `newDailyNote` from `mount.ts`, then remove only their imports and call sites. Remove desktop and mobile menu items, keyboard matching, the `dailyNote` global-command case, command-panel entries, and `dailyNote` from the non-macOS hotkey-correction list. Keep `mountHelp` and all unrelated mount utilities.

- [ ] **Step 2: Remove native-menu state and tests' implementation targets**

Remove `dailyNote` from Electron `NATIVE_MENU_COMMANDS`, `NATIVE_MENU_LABEL_KEYS`, `DEFAULT_LABELS`, and the File menu template. Remove it from the renderer allowlist and localized labels. Preserve the user's current settings-shortcut changes in the same files.

- [ ] **Step 3: Remove persisted frontend schema entries and stale-event consumers**

Remove `Constants.LOCAL_DAILYNOTEID`, `Constants.SIYUAN_KEYMAP.general.dailyNote`, and `Config.IKeymap.general.dailyNote`. Remove the `createdailynote` cases from desktop and mobile file trees because no producer remains. Do not remove `iconCalendar` from the shared icon set or Attribute View date rendering.

- [ ] **Step 4: Remove notebook Daily Note settings**

Remove the two Daily Note fields from `INotebookConf`, the Daily Note configuration markup, element bindings, and `/api/notebook/setNotebookConf` payload. Keep document-create and reference-create configuration behavior unchanged.

- [ ] **Step 5: Remove HTTP routes and handlers**

Delete the three registrations from `kernel/api/router.go`. Delete `createDailyNote` from `filetree.go` and `appendDailyNoteBlock`/`prependDailyNoteBlock` from `block_op.go`; then remove imports that become unused. Preserve adjacent generic document and block handlers.

- [ ] **Step 6: Remove model and configuration support**

Delete `DailyNoteAttrPrefix` and `CreateDailyNote` from `kernel/model/file.go` while retaining `NodeAttrTitleEmpty`, `DocHiddenAttr`, and generic template rendering. Remove `DailyNoteSavePath` and `DailyNoteTemplatePath` from `BoxConf` and `NewBoxConf`. Remove Daily Note normalization from `setNotebookConf` and imported-notebook copying from `model/import.go`. Do not scan or edit existing document attributes.

- [ ] **Step 7: Delete CLI and MCP implementations**

Delete `kernel/cli/cmd/dailynote.go` and `kernel/mcp/tools/dailynote.go`. Both register themselves through `init`, so no separate registry edit is required.

- [ ] **Step 8: Format and run the focused green tests**

Run `gofmt` on every changed Go file. Then run:

`cd kernel && go test ./api ./mcp ./mcp/tools ./cli/cmd ./model ./conf -run 'TestRemovedProductRoutesAndPreservedBoundaries|TestMCPServerBoundary|TestDailyNoteCommandRemoved|TestNonExistent' -count=1`

Expected: PASS with Daily Note absent and existing preservation assertions intact.

Run: `cd app && pnpm exec tsx --test src/util/nativeMenu.test.ts`

Expected: PASS with no native Daily Note item.

---

### Task 3: Remove CalDAV while preserving CardDAV and WebDAV

**Files:**
- Delete: `kernel/model/caldav.go`
- Modify: `kernel/server/serve.go`
- Modify: `kernel/model/session.go`
- Modify: `kernel/model/dav.go`
- Test: `kernel/server/dav_routes_test.go`

**Interfaces:**
- Consumes: `serveDAV(*gin.Engine)` boundary from Task 1 and existing CardDAV/WebDAV backends.
- Produces: no CalDAV route or backend, with CardDAV/WebDAV registration and shared DAV filesystem utilities preserved.

- [ ] **Step 1: Remove CalDAV route registration and middleware branches**

Remove the `caldav` import, `CalDavMethods`, `serveCalDAV`, and its call inside `serveDAV`. Remove the CalDAV-specific CORS method variable and `/caldav` branch. Keep the `MethodReport` constant because CardDAV uses it.

- [ ] **Step 2: Remove CalDAV authentication handling**

Remove only `strings.HasPrefix(c.Request.RequestURI, "/caldav")` from the DAV Basic Authentication condition in `model/session.go`. Preserve `/webdav` and `/carddav` behavior.

- [ ] **Step 3: Narrow shared DAV metadata helpers**

Remove the `caldav` import from `model/dav.go`, change `SaveMetaData` from the generic CalDAV/CardDAV union to:

```go
func SaveMetaData(metaData []*carddav.AddressBook, metaDataFilePath string) error
```

Update comments to describe DAV/CardDAV filesystem paths accurately. Keep `DavPath2DirectoryPath`, `PathJoinWithSlash`, `PathCleanWithSlash`, and `FileETag` because CardDAV consumes them.

- [ ] **Step 4: Delete the CalDAV backend**

Delete `kernel/model/caldav.go`. Do not remove `github.com/emersion/go-webdav` from `go.mod` or `go.sum`, and do not delete any workspace storage directory.

- [ ] **Step 5: Format and run the DAV boundary test**

Run: `gofmt -w kernel/server/serve.go kernel/server/dav_routes_test.go kernel/model/session.go kernel/model/dav.go`

Run: `cd kernel && go test ./server ./model -run 'TestDAVRouteBoundary|TestNonExistent' -count=1`

Expected: PASS; no CalDAV routes are registered, and CardDAV/WebDAV routes still exist and compile.

---

### Task 4: Remove localization and documentation, then verify the frozen tree

**Files:**
- Modify: `app/appearance/langs/*.json` (21 files)
- Modify: `docs/API.md`
- Modify: `docs/API.zh-CN.md`
- Modify: `docs/API.ja.md`
- Modify: `docs/WORKSPACE.md`
- Modify: `docs/WORKSPACE.zh-CN.md`
- Modify/Delete: Daily Note material under `app/guide/**`
- Modify: `docs/superpowers/specs/2026-08-10-feature-removal-design.md`
- Modify: `docs/superpowers/plans/2026-08-10-feature-removal.md`

**Interfaces:**
- Consumes: the code-free Daily Note/CalDAV tree from Tasks 2 and 3.
- Produces: shipped localization and documentation with no removed feature instructions or obsolete configuration examples.

- [ ] **Step 1: Remove exclusive localization keys**

Remove `dailyNote`, `fileTree11`, `fileTree14`, and `fileTree15` from every language JSON file. Preserve all unrelated pending MCP and branding translations. Parse every file as JSON after the mechanical removal.

- [ ] **Step 2: Remove API and workspace configuration documentation**

Remove `dailyNoteSavePath` and `dailyNoteTemplatePath` from notebook configuration examples in all three API documents and from the two workspace-field tables. Do not remove generic examples that merely use a date-formatted path unless they specifically claim to be Daily Note configuration.

- [ ] **Step 3: Remove shipped guide feature pages and navigation**

Delete the Simplified Chinese, Traditional Chinese, English, and Japanese Daily Note document files and remove block-reference navigation nodes that target their IDs. Remove the Daily Note sections from the four CLI/MCP guide documents by deleting the heading and all following sibling blocks up to the next heading of equal or higher level. Remove developer API paragraphs that instruct clients to create Daily Notes or rely on `custom-dailynote-*`. Parse every changed `.sy` file as JSON and verify all remaining block-reference targets for the deleted document IDs are absent.

- [ ] **Step 4: Reconcile prior feature-removal documentation**

Replace statements that Daily Notes remain with a reference to this follow-on removal. Do not rewrite or invalidate the other approved feature-removal tasks.

- [ ] **Step 5: Run localization and residual-reference checks**

Run: `python scripts/check-lang-keys.py`

Expected: PASS across all 21 language files.

Run targeted scans over `app/src`, `app/electron`, and `kernel` for `DailyNote`, `dailyNote`, `dailynote`, `createdailynote`, `CalDav`, `CalDAV`, and `/caldav`, excluding tests that explicitly assert absence. Expected: no product-code matches. Incidental `iconCalendar`, Attribute View date logic, CardDAV, and WebDAV matches are allowed.

- [ ] **Step 6: Run the final integration verification**

Run: `cd kernel && go test ./api ./mcp ./mcp/tools ./cli/cmd ./server ./model ./conf -count=1`

Run: `cd app && pnpm run lint`

Expected: both commands PASS. Do not run a frontend production build or compile the kernel binary.

- [ ] **Step 7: Review the final diff and user-data boundary**

Inspect `git diff --check`, the final diff for all task-owned files, and `git status --short`. Confirm no existing document outside shipped `app/guide` sources was touched, no CalDAV storage path was deleted, no generated file changed, and unrelated dirty-worktree changes remain present.
