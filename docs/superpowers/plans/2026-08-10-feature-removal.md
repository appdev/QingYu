# Feature Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove AI/Agent/semantic search, flashcards, cloud accounts and official cloud services, inbox, and client-side marketplace install reporting while preserving the authenticated MCP Server, third-party sync, local snapshots, plugins, marketplace download statistics, local authentication, and PDF reading.

**Architecture:** Remove each feature as a vertical slice from UI consumers through API, model, configuration, storage, and dependencies. Preserve shared infrastructure by adding boundary tests before deletion, then clean generated types and i18n only after all consumers are gone. Removed APIs are not registered and no compatibility shims or data migrations are added because the product has not shipped.

**Tech Stack:** Go 1.26, Gin, TypeScript, pnpm 11.12.0, Python 3, SQLite, Electron/Web frontend.

## Global Constraints

- Preserve `/mcp`, its local authentication, administrator-role check, read-only check, generic note tools, and plugin-registered tools.
- Preserve S3, WebDAV, and local-filesystem sync without cloud-account or subscription checks.
- Preserve local repository snapshots and remove only official remote snapshot operations.
- Preserve marketplace GitHub repository metadata, download-count reads, display, and sorting; remove only the client install-count report and `systemID` transmission.
- Preserve local access authentication, API Token, roles, TLS, plugins, PDF reading, full-text search, and all unrelated note features.
- Do not add compatibility migrations or disabled endpoint stubs.
- Do not hand-edit generated files under `app/stage/build/**` or `app/src/types/dist/**`.
- Do not run `pnpm build`, `pnpm dev`, compile the kernel binary, or restart a running kernel.
- Do not commit, push, publish, or deploy; the user has not authorized Git writes beyond workspace file edits.
- Use `gofmt` after Go edits, `cd app && pnpm run lint` for frontend verification, and `python scripts/check-lang-keys.py` after i18n edits.

---

### Task 1: Add preservation and removal boundary tests

**Files:**
- Create: `kernel/api/feature_removal_test.go`
- Create: `kernel/mcp/feature_boundary_test.go`
- Create: `kernel/bazaar/install_signature_test.go`
- Create: `kernel/conf/sync_test.go`

**Interfaces:**
- Consumes: `api.ServeAPI(*gin.Engine)`, `mcp.Serve(*gin.Engine)`, `mcp/tools.LookupTool(string)`, `bazaar.InstallPackage`, and sync provider constants.
- Produces: executable contracts that reject removed API families while protecting `/mcp`, marketplace statistics, and the three retained sync providers.

- [ ] **Step 1: Add a route-boundary test that initially fails**

Create `kernel/api/feature_removal_test.go` with a helper that registers `ServeAPI` on `gin.New()`, collects `engine.Routes()`, and asserts that no route starts with `/api/ai/`, `/api/riff/`, `/api/account/`, `/api/inbox/`, or the removed official-cloud route set. Assert that `/api/sync/setSyncProviderS3`, `/api/sync/setSyncProviderWebDAV`, `/api/sync/setSyncProviderLocal`, `/api/system/loginAuth`, and `/api/system/setAPIToken` remain.

```go
func TestRemovedProductRoutesAndPreservedBoundaries(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	ServeAPI(engine)
	routes := map[string]bool{}
	for _, route := range engine.Routes() {
		routes[route.Path] = true
		for _, prefix := range []string{"/api/ai/", "/api/riff/", "/api/account/", "/api/inbox/"} {
			if strings.HasPrefix(route.Path, prefix) {
				t.Fatalf("removed route remains registered: %s", route.Path)
			}
		}
	}
	for _, removed := range []string{
		"/api/cloud/getCloudSpace", "/api/cloud/setCloudReminder",
		"/api/setting/getCloudUser", "/api/setting/logoutCloudUser", "/api/setting/login2faCloudUser",
		"/api/asset/uploadCloud", "/api/asset/uploadCloudByAssetsPaths",
		"/api/repo/purgeCloudRepo", "/api/repo/getCloudRepoTagSnapshots", "/api/repo/getCloudRepoSnapshots",
		"/api/repo/removeCloudRepoTagSnapshot", "/api/repo/uploadCloudSnapshot", "/api/repo/downloadCloudSnapshot",
	} {
		if routes[removed] {
			t.Fatalf("removed official-cloud route remains registered: %s", removed)
		}
	}
	for _, preserved := range []string{
		"/api/sync/setSyncProviderS3", "/api/sync/setSyncProviderWebDAV", "/api/sync/setSyncProviderLocal",
		"/api/system/loginAuth", "/api/system/setAPIToken",
	} {
		if !routes[preserved] {
			t.Fatalf("preserved route is missing: %s", preserved)
		}
	}
}
```

- [ ] **Step 2: Run the route-boundary test and verify the red state**

Run: `cd kernel && go test ./api -run TestRemovedProductRoutesAndPreservedBoundaries -count=1`

Expected: FAIL naming at least one currently registered AI, riff, account, inbox, or official-cloud route.

- [ ] **Step 3: Add MCP preservation/removal tests**

Create `kernel/mcp/feature_boundary_test.go` using package `mcp_test`. Register MCP routes on a fresh Gin engine with `mcp.Serve(engine)` and assert `POST /mcp` remains. Assert the exact generic tool names `notebook`, `document`, `block`, `database`, `search`, and `sync` remain. Assert the removed `inbox` tool and AI-backed `image` tool do not remain.

```go
func TestMCPServerBoundary(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	mcp.Serve(engine)
	foundPost := false
	for _, route := range engine.Routes() {
		foundPost = foundPost || route.Method == http.MethodPost && route.Path == "/mcp"
	}
	if !foundPost {
		t.Fatal("POST /mcp must remain registered")
	}
	for _, preserved := range []string{"notebook", "document", "block", "database", "search", "sync"} {
		if tools.LookupTool(preserved) == nil {
			t.Fatalf("preserved MCP tool is missing: %s", preserved)
		}
	}
	for _, removed := range []string{"inbox", "image"} {
		if tools.LookupTool(removed) != nil {
			t.Fatalf("removed MCP tool remains: %s", removed)
		}
	}
}
```

Run: `cd kernel && go test ./mcp -run TestMCPServerBoundary -count=1`

Expected: FAIL because the AI-backed image and inbox tools currently remain.

- [ ] **Step 4: Add compile-time contracts for marketplace reporting and sync providers**

Create `kernel/bazaar/install_signature_test.go` with a compile-time assignment requiring the post-removal five-argument install signature. Extend `kernel/conf/sync_test.go` to assert the only provider constants are S3 `0`, WebDAV `1`, and Local `2`, with `NewSync().Provider == ProviderLocal` so a fresh unconfigured application never attempts remote sync.

```go
var _ func(string, string, string, string, string) error = InstallPackage

func TestRetainedSyncProviders(t *testing.T) {
	if ProviderS3 != 0 || ProviderWebDAV != 1 || ProviderLocal != 2 {
		t.Fatalf("unexpected provider values: S3=%d WebDAV=%d Local=%d", ProviderS3, ProviderWebDAV, ProviderLocal)
	}
	if NewSync().Provider != ProviderLocal {
		t.Fatalf("new sync config must default to local provider")
	}
}
```

Run: `cd kernel && go test ./bazaar ./conf -run 'TestRetainedSyncProviders|TestNonExistent' -count=1`

Expected: build failure because `InstallPackage` still accepts `systemID`, and sync constants still include `ProviderSiYuan` with the old numbering.

- [ ] **Step 5: Keep these tests red and review their scope**

Do not weaken or skip the tests. Confirm the failing assertions correspond only to explicitly approved removals and that all preserved routes/tools are named from actual registrations rather than guessed names.

---

### Task 2: Remove AI, Agent, MCP client, and semantic search

**Files:**
- Delete: `app/src/ai/actions.ts`
- Delete: `app/src/ai/chat.ts`
- Delete: `app/src/layout/dock/agent/`
- Delete: `app/src/config/tabs/aiRuntime.ts`
- Delete: `app/src/config/tabs/aiTab.ts`
- Delete: `app/src/config/tabs/aiUi.ts`
- Delete: `app/src/assets/scss/business/_ai_agent.scss`
- Delete: `kernel/agent/`
- Delete: `kernel/mcp/client/`
- Delete: `kernel/api/ai.go`
- Delete: `kernel/api/agent.go`
- Delete: `kernel/api/agent_timeout_test.go`
- Delete: `kernel/conf/ai.go`
- Delete: `kernel/conf/ai_mcp_test.go`
- Delete: `kernel/conf/ai_multimodal_test.go`
- Delete: `kernel/conf/ai_provider_test.go`
- Delete: `kernel/conf/ai_timeout_test.go`
- Delete: `kernel/model/ai.go`
- Delete: `kernel/model/embedding.go`
- Delete: `kernel/model/rerank.go`
- Delete: `kernel/util/openai.go`
- Delete: `kernel/util/openai_extra_test.go`
- Delete: `kernel/util/openai_multimodal_test.go`
- Delete: `kernel/mcp/tools/image.go`
- Delete: `kernel/mcp/tools/image_test.go`
- Modify: `kernel/api/router.go`
- Modify: `kernel/api/search.go`
- Modify: `kernel/model/search.go`
- Modify: `kernel/model/assets.go`
- Modify: `kernel/model/assets_test.go`
- Modify: `kernel/model/conf.go`
- Modify: `kernel/model/conf_save_test.go`
- Modify: `kernel/model/session.go`
- Modify: `kernel/server/serve.go`
- Modify: `kernel/api/setting.go`
- Modify: `kernel/api/system.go`
- Modify: `kernel/sql/database.go`
- Modify: `kernel/sql/queue.go`
- Modify: `kernel/sql/block_query.go`
- Modify: `kernel/cli/cmd/root.go`
- Modify: `kernel/cli/cmd/serve.go`
- Modify: `kernel/mobile/kernel.go`
- Modify: `kernel/go.mod`
- Modify: `kernel/go.sum`
- Modify: `app/src/config/setting/tabs.ts`
- Modify: `app/src/layout/dock/index.ts`
- Modify: `app/src/layout/util.ts`
- Modify: `app/src/plugin/index.ts`
- Modify: `app/src/plugin/uninstall.ts`
- Modify: `app/src/search/menu.ts`
- Modify: `app/src/search/spread.ts`
- Modify: `app/src/search/util.ts`
- Modify: `app/src/mobile/menu/search.ts`
- Modify: `app/src/protyle/gutter/index.ts`
- Modify: `app/src/protyle/render/setLute.ts`
- Modify: `app/src/types/config.d.ts`
- Modify: `app/src/types/index.d.ts`
- Modify: `app/src/assets/scss/base.scss`

**Interfaces:**
- Consumes: Task 1 route and MCP boundary tests.
- Produces: no AI configuration or runtime, no semantic search method, no `block_embeddings` table, and an intact MCP Server with generic tools.

- [ ] **Step 1: Remove frontend AI consumers and semantic-search UI**

Delete the listed AI, Agent, settings, and SCSS files. Remove their imports, dock registration, settings-tab registration, menu actions, keyboard actions, plugin Agent action registration, and `Config.IAI`/Agent-related TypeScript declarations. Remove semantic search method `4` and all `config.ai.embedding` checks from desktop and mobile search UI while preserving ordinary full-text, document, reference, asset, and SQL search methods.

- [ ] **Step 2: Remove AI and Agent API registrations**

Delete every `/api/ai/*` registration from `kernel/api/router.go`, including MCP client OAuth callbacks, model tests, Embedding controls, and Agent session/skill routes. Delete `kernel/api/ai.go` and `kernel/api/agent.go`; remove the semantic search route and handler while leaving `/api/search/fullTextSearchBlock` and other nonsemantic search routes intact.

- [ ] **Step 3: Remove backend AI runtime and configuration**

Delete `kernel/agent/`, `kernel/mcp/client/`, `kernel/conf/ai.go`, `kernel/model/ai.go`, `kernel/model/embedding.go`, and `kernel/model/rerank.go`. Remove `Conf.AI`, `Conf.MCPOAuth`, AI initialization, API-key migration, MCP-client bootstrap/reconnect, AI request activity exceptions, CLI/server/mobile Embedding startup, configuration export/import secret scrubbing, and AI environment-variable handling. Keep `Conf.Secrets` because plugin-registered MCP tools may use it independently.

- [ ] **Step 4: Remove AI-backed image operations while retaining ordinary assets**

Delete `kernel/mcp/tools/image.go` and its tests. In `kernel/model/assets.go`, remove `AnalyzeDocumentImage*`, `GenerateDocumentImage*`, external AI execution, and generated-image persistence helpers, but retain local asset listing, upload, OCR, unused/missing asset checks, and PDF annotation behavior. Update `kernel/model/assets_test.go` to remove tests solely for AI image analysis/generation and preserve local path-security tests.

- [ ] **Step 5: Remove Embedding storage and queue hooks**

Remove `block_embeddings` creation, migration, deletion, update, queue maintenance, and query helpers from `kernel/sql/database.go`, `kernel/sql/queue.go`, and `kernel/sql/block_query.go`. Remove Embedding dirty notifications from block indexing while preserving FTS5 indexing and encrypted-notebook database isolation.

- [ ] **Step 6: Remove now-unused Go dependencies**

Remove direct dependencies used only by the deleted runtime, including `github.com/sashabaranov/go-openai`, `github.com/pkoukk/tiktoken-go`, and `github.com/pkoukk/tiktoken-go-loader`. Run `cd kernel && go mod tidy` to update `go.mod` and `go.sum`; inspect the diff to ensure MCP Server dependencies remain.

- [ ] **Step 7: Format and verify the slice**

Run: `gofmt -w kernel/api/router.go kernel/api/search.go kernel/model/search.go kernel/model/assets.go kernel/model/conf.go kernel/model/session.go kernel/server/serve.go kernel/sql/database.go kernel/sql/queue.go kernel/sql/block_query.go`

Run: `cd kernel && go test ./api ./mcp ./mcp/tools ./model ./sql ./conf -count=1`

Expected: AI-related route assertions and MCP removal assertions pass; generic MCP, asset, search, SQL, and configuration tests pass.

Run: `cd app && pnpm run lint`

Expected: PASS with no missing AI/Agent imports or semantic-search type branches.

---

### Task 3: Remove flashcards and spaced repetition

**Files:**
- Delete: `app/src/card/`
- Delete: `app/src/config/tabs/flashcardRuntime.ts`
- Delete: `app/src/config/tabs/flashcardTab.ts`
- Delete: `kernel/api/riff.go`
- Delete: `kernel/conf/flashcard.go`
- Delete: `kernel/model/flashcard.go`
- Modify: `kernel/api/router.go`
- Modify: `kernel/api/filetree.go`
- Modify: `kernel/api/notebook.go`
- Modify: `kernel/api/setting.go`
- Modify: `kernel/api/system.go`
- Modify: `kernel/conf/sync.go`
- Modify: `kernel/model/block.go`
- Modify: `kernel/model/box.go`
- Modify: `kernel/model/conf.go`
- Modify: `kernel/model/export.go`
- Modify: `kernel/model/file.go`
- Modify: `kernel/model/import.go`
- Modify: `kernel/model/repository.go`
- Modify: `kernel/model/transaction.go`
- Modify: `kernel/mobile/kernel.go`
- Modify: `kernel/harmony/kernel.go`
- Modify: `kernel/cli/cmd/serve.go`
- Modify: `kernel/go.mod`
- Modify: `kernel/go.sum`
- Modify: `app/src/boot/globalEvent/command/panel.ts`
- Modify: `app/src/boot/globalEvent/keydown.ts`
- Modify: `app/src/business/openRecentDocs.ts`
- Modify: `app/src/config/setting/tabs.ts`
- Modify: `app/src/constants.ts`
- Modify: `app/src/menus/commonMenuItem.ts`
- Modify: `app/src/menus/navigation.ts`
- Modify: `app/src/menus/workspace.ts`
- Modify: `app/src/mobile/menu/index.ts`
- Modify: `app/src/mobile/menu/search.ts`
- Modify: `app/src/plugin/API.ts`
- Modify: `app/src/protyle/gutter/index.ts`
- Modify: `app/src/protyle/header/openTitleMenu.ts`
- Modify: `app/src/search/util.ts`
- Modify: `app/src/types/config.d.ts`
- Modify: `app/src/types/index.d.ts`
- Modify: `app/src/util/pathName.ts`

**Interfaces:**
- Consumes: Task 1 route-boundary test and the AI-free tree from Task 2.
- Produces: no riff API, storage, UI, config, import/sync hook, or block metadata; ordinary task-list items remain supported.

- [ ] **Step 1: Remove flashcard UI and command consumers**

Delete the card and flashcard settings modules. Remove flashcard commands, menus, due-count badges, keybindings, recent-document card tabs, title/gutter actions, mobile entries, plugin API event variants, and TypeScript interfaces. Keep Markdown task-list checkbox behavior and ordinary block reminders.

- [ ] **Step 2: Remove riff API and model state**

Delete all `/api/riff/*` routes and `kernel/api/riff.go`. Delete `kernel/model/flashcard.go` and `kernel/conf/flashcard.go`; remove `Conf.Flashcard`, flashcard default validation, `RiffCardID`, `RiffCard`, deck loading, transaction hooks, import/export hooks, sync reload flags, and mobile/harmony binding methods.

- [ ] **Step 3: Remove the riff dependency**

Remove `github.com/siyuan-note/riff` and its commented local replace from `kernel/go.mod`. Run `cd kernel && go mod tidy` and verify no `riff` module remains in `go.mod` or `go.sum`.

- [ ] **Step 4: Format and verify the slice**

Run: `rg -n '/api/riff|github.com/siyuan-note/riff|RiffCard|Flashcard|flashcard' app/src kernel --glob '!app/appearance/langs/**'`

Expected: no product-code matches; comments and changelogs outside the searched roots are not part of this check.

Run: `cd kernel && go test ./api ./model ./conf ./mobile ./harmony -count=1`

Run: `cd app && pnpm run lint`

Expected: PASS.

---

### Task 4: Remove cloud accounts and official cloud services while preserving third-party sync

**Files:**
- Delete: `app/src/config/tabs/accountUi.ts`
- Delete: `app/src/util/iOSPurchase.ts`
- Delete: `kernel/api/account.go`
- Delete: `kernel/model/cloud_service.go`
- Delete: `kernel/conf/account.go`
- Delete: `kernel/conf/user.go`
- Modify: `kernel/api/router.go`
- Modify: `kernel/api/asset.go`
- Modify: `kernel/api/repo.go`
- Modify: `kernel/api/setting.go`
- Modify: `kernel/api/sync.go`
- Modify: `kernel/conf/sync.go`
- Modify: `kernel/model/assets.go`
- Modify: `kernel/model/conf.go`
- Modify: `kernel/model/repository.go`
- Modify: `kernel/model/sync.go`
- Modify: `kernel/util/cloud.go`
- Modify: `app/src/index.ts`
- Modify: `app/src/window/index.ts`
- Modify: `app/src/mobile/index.ts`
- Modify: `app/src/config/setting/tabs.ts`
- Modify: `app/src/config/tabs/syncRuntime.ts`
- Modify: `app/src/config/tabs/syncTab.ts`
- Modify: `app/src/config/tabs/syncUi.ts`
- Modify: `app/src/layout/topBar.ts`
- Modify: `app/src/mobile/menu/index.ts`
- Modify: `app/src/types/config.d.ts`
- Modify: `app/src/types/index.d.ts`

**Interfaces:**
- Consumes: Task 1 sync-provider and route tests.
- Produces: provider enum `ProviderS3=0`, `ProviderWebDAV=1`, `ProviderLocal=2`; local default; no cloud account or official provider; local repository operations unchanged.

- [ ] **Step 1: Remove cloud-account UI and initialization**

Delete the account tab and iOS purchase helper. Remove `getCloudUser` calls from desktop, detached-window, and mobile startup. Remove account state, login/2FA/activation/trial/deactivation controls, subscription gates, cloud-space display, official provider option, official cloud directory controls, and official cloud status actions. Keep the sync status icon if it represents any retained provider rather than account state.

- [ ] **Step 2: Remove cloud-account and official-cloud routes**

Delete `/api/account/*`, `/api/cloud/*`, cloud-user setting routes, official cloud asset upload routes, and remote snapshot routes listed in Task 1. Delete handlers that become unused. Preserve local repo routes such as initialization, snapshot creation/tagging/listing/diff/checkout, local snapshot file restore/export, retention, and purge of the local repository where applicable.

- [ ] **Step 3: Simplify sync provider configuration**

In `kernel/conf/sync.go`, remove `ProviderSiYuan`, set `ProviderS3=0`, `ProviderWebDAV=1`, `ProviderLocal=2`, and default `NewSync().Provider` to `ProviderLocal`. Remove official-only fields and validation while keeping `S3`, `WebDAV`, `Local`, sync interval, mode, conflict handling, and generic remote-directory naming required by Dejavu.

- [ ] **Step 4: Remove account and entitlement checks from retained providers**

Delete `Conf.Account`, `Conf.User`, cloud-region/account initialization, subscriber checks, trial/activation logic, official endpoints, official cloud implementation selection, cloud capacity checks, and official WebSocket behavior. Ensure `SetSyncProviderS3`, `SetSyncProviderWebDAV`, `SetSyncProviderLocal`, manual sync, boot sync, and scheduled sync operate solely from local configuration and do not call `IsSubscriber()`.

- [ ] **Step 5: Preserve marketplace content endpoints in `kernel/util/cloud.go`**

Do not delete `BazaarOSSServer`, `BazaarStatServer`, or any endpoint still required for explicitly retained marketplace index/statistics/content reads. Remove only constants and helpers whose remaining callers were cloud account, official sync/storage, official backup, or cloud resource upload. Verify each retained constant with `rg` before removal.

- [ ] **Step 6: Add and run retained-provider behavior tests**

Extend sync tests to instantiate each retained provider configuration without `Conf.User` and verify provider selection succeeds. Where network transfer cannot run without credentials, test configuration and cloud-backend construction using invalid endpoints only far enough to confirm the error is endpoint-related, not subscription/account-related.

Run: `cd kernel && go test ./api ./conf ./model -run 'TestRemovedProductRoutesAndPreservedBoundaries|TestRetainedSyncProviders|Sync|Repo' -count=1`

Expected: route and enum tests pass; retained sync tests have no account/subscription failures.

- [ ] **Step 7: Format and verify the slice**

Run: `rg -n '/api/account|/api/cloud|getCloudUser|CloudUser|ProviderSiYuan|IsSubscriber|startFreeTrial|activationcode|uploadCloudSnapshot|downloadCloudSnapshot' app/src kernel`

Expected: no active product-code matches. Marketplace statistics/content endpoints and generic third-party “cloud” library terminology may remain when not tied to the official provider.

Run: `cd app && pnpm run lint`

Expected: PASS.

---

### Task 5: Remove inbox and shorthand capture

**Files:**
- Delete: `app/src/layout/dock/Inbox.ts`
- Delete: `kernel/api/inbox.go`
- Delete: `kernel/cli/cmd/inbox.go`
- Delete: `kernel/mcp/tools/inbox.go`
- Modify: `kernel/api/router.go`
- Modify: `kernel/api/filetree.go`
- Modify: `kernel/api/setting.go`
- Modify: `kernel/cli/cmd/root.go`
- Modify: `kernel/conf/filetree.go`
- Modify: `kernel/job/cron.go`
- Modify: `kernel/model/conf.go`
- Modify: `kernel/model/shortcuts.go`
- Modify: `kernel/model/sync.go`
- Modify: `kernel/mcp/tools/skill.go`
- Modify: `kernel/util/skill_test.go`
- Modify: `app/src/assets/template/mobile/index.tpl`
- Modify: `app/src/config/tabs/fileTab.ts`
- Modify: `app/src/constants.ts`
- Modify: `app/src/layout/Model.ts`
- Modify: `app/src/layout/dock/index.ts`
- Modify: `app/src/mobile/util/initFramework.ts`
- Modify: `app/src/types/config.d.ts`
- Modify: `app/src/types/index.d.ts`

**Interfaces:**
- Consumes: Task 1 API and MCP boundary tests.
- Produces: no inbox route, Dock, CLI, shorthand configuration, scheduled job, or MCP tool; normal document creation remains. Daily Note is removed by the follow-on `2026-08-10-remove-daily-note-caldav.md` plan.

- [ ] **Step 1: Remove inbox UI and layout state**

Delete the Inbox Dock and remove its model registration, layout restoration branch, mobile template node, constants, interfaces, and file-tree settings for shorthand destination. Preserve Files, Outline, Backlink, Bookmark, Tag, Graph, and custom docks.

- [ ] **Step 2: Remove inbox API, CLI, model, and job consumers**

Delete `/api/inbox/*`, shorthand save-path and move-local-shorthand routes, the inbox CLI command, shorthand sync/shortcut operations, and scheduled shorthand jobs. Remove `ShorthandSaveBox` and `ShorthandSavePath` from configuration and default normalization.

- [ ] **Step 3: Remove MCP inbox tooling without weakening MCP Server**

Delete `kernel/mcp/tools/inbox.go` and remove any inbox references from the skill tool or its tests. Do not modify `/mcp`, tool registration infrastructure, plugin tool registration, or unrelated tools.

- [ ] **Step 4: Format and verify the slice**

Run: `rg -n '/api/inbox|Inbox|Shorthand|shorthand' app/src kernel`

Expected: no active product-code matches except unrelated English uses of “shorthand” that are manually verified.

Run: `cd kernel && go test ./api ./mcp ./mcp/tools ./conf ./model ./cli/cmd -count=1`

Run: `cd app && pnpm run lint`

Expected: PASS and `TestMCPServerBoundary` confirms `/mcp` remains while the inbox tool is absent.

---

### Task 6: Remove marketplace install reporting and Agent-only plugin extensions

**Files:**
- Modify: `kernel/bazaar/install.go`
- Modify: `kernel/model/bazaar.go`
- Modify: `kernel/api/bazaar.go`
- Modify: `app/src/plugin/index.ts`
- Modify: `app/src/plugin/uninstall.ts`
- Modify: `app/src/plugin/API.ts`
- Modify: `app/src/types/index.d.ts`
- Test: `kernel/bazaar/install_signature_test.go`
- Test: existing `kernel/bazaar/*_test.go`

**Interfaces:**
- Consumes: marketplace `repoURL`, `repoHash`, package type/name, and the Task 1 five-argument signature contract.
- Produces: `InstallPackage(repoURL, repoHash, installPath, pkgType, packageName string) error` with no device identifier or report call; marketplace download statistics remain readable and sortable.

- [ ] **Step 1: Remove the client install-count report**

Delete `incPackageDownloads`, its request to `/apis/siyuan/bazaar/addBazaarPackageDownloadCount`, the goroutine call after installation, and the `systemID` parameter from `bazaar.InstallPackage`. Propagate the five-argument signature through `kernel/model/bazaar.go` and any API callers.

- [ ] **Step 2: Preserve download statistics and sorting explicitly**

Do not remove `BazaarStatServer`, `getBazaarStats`, `bazaarStats.Downloads`, `Package.Downloads`, frontend `downloads`, download-count rendering, or ascending/descending download sorting. Add assertions to an existing bazaar test or a new focused test that stage statistics still populate `Package.Downloads`.

```go
func TestBuildBazaarPackageKeepsDownloadStatistics(t *testing.T) {
	repo := &StageRepo{URL: "owner/repo@hash", Package: &Package{Name: "repo"}}
	pkg := buildBazaarPackageWithMetadata(repo, map[string]*bazaarStats{"owner/repo": {Downloads: 42}}, "plugins", "desktop")
	if pkg.Downloads != 42 {
		t.Fatalf("downloads=%d, want 42", pkg.Downloads)
	}
}
```

- [ ] **Step 3: Remove Agent-only plugin APIs**

Remove the frontend plugin method that registers Agent actions, its `plugin__<name>__<action>` tracking, uninstall cleanup, frontend action registry interactions, and related TypeScript declarations. Preserve ordinary plugin lifecycle, commands, menus, docks, RPC, settings, custom block rendering, and plugin registration of MCP Server tools.

- [ ] **Step 4: Verify no install reporting remains**

Run: `rg -n 'addBazaarPackageDownloadCount|incPackageDownloads|systemID.*InstallPackage|InstallPackage\([^\n]*systemID' kernel app/src`

Expected: no matches.

Run: `cd kernel && go test ./bazaar ./model ./api -count=1`

Expected: signature and statistics-preservation tests pass.

Run: `cd app && pnpm run lint`

Expected: PASS; download rendering and sorting remain type-correct.

---

### Task 7: Clean i18n, dependencies, and dead code; run final integration verification

**Files:**
- Modify: `app/appearance/langs/*.json`
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `README.ja.md`
- Modify: `README.tr.md`
- Modify: `kernel/go.mod`
- Modify: `kernel/go.sum`
- Test: all boundary tests from Task 1

**Interfaces:**
- Consumes: completed feature slices from Tasks 2–6.
- Produces: a coherent product surface with no removed feature strings or dependencies and a frozen verified tree.

- [ ] **Step 1: Identify and remove only orphaned language keys**

Generate the candidate key set from deleted TypeScript/Go call sites before deletion or from Git diff, then use `rg` across `app/src` and `kernel` to prove each candidate has no remaining consumer. Remove confirmed orphan keys from all 21 language JSON files. Do not remove shared words such as ordinary “search”, “sync”, “plugin”, “account” used by local publish authentication, or “cloud” used by S3/WebDAV abstractions.

- [ ] **Step 2: Update feature claims in repository README files**

Remove claims for AI writing/chat, semantic search if documented, flashcard spaced repetition, official cloud account/sync/backup, and inbox. Retain API, plugins/marketplace, MCP Server, local snapshots, and S3/WebDAV/local sync claims. Keep Markdown paragraphs and list items on single lines per repository rules.

- [ ] **Step 3: Run dependency and forbidden-symbol scans**

Run:

```bash
rg -n '/api/ai|/api/riff|/api/account|/api/inbox|semanticSearchBlock|block_embeddings|ProviderSiYuan|addBazaarPackageDownloadCount|incPackageDownloads|github.com/siyuan-note/riff|github.com/sashabaranov/go-openai|tiktoken-go' app/src kernel
```

Expected: no matches.

Run:

```bash
rg -n '(/mcp|setSyncProviderS3|setSyncProviderWebDAV|setSyncProviderLocal|BazaarStatServer|downloads)' kernel app/src
```

Expected: matches proving MCP Server, third-party sync, and marketplace download statistics remain.

- [ ] **Step 4: Verify telemetry boundaries**

Run:

```bash
rg -n -i 'google analytics|sentry|posthog|segment\.io|addBazaarPackageDownloadCount|incPackageDownloads' app/src kernel --glob '!app/changelogs/**'
```

Expected: no active analytics/reporting implementation matches. PDF.js `reportTelemetry` event names may remain only under `app/src/asset/pdf/`; confirm `external_services.js` remains an empty sender and `app_options.js` keeps telemetry disabled.

- [ ] **Step 5: Format and validate language files**

Run: `python scripts/check-lang-keys.py`

Expected: all language files parse and have identical key sets.

- [ ] **Step 6: Run focused Go integration tests**

Run: `cd kernel && go test ./api ./bazaar ./conf ./mcp ./mcp/tools ./model ./sql ./cli/cmd -count=1`

Expected: PASS. This does not compile or restart the kernel binary.

- [ ] **Step 7: Run frontend lint**

Run: `cd app && pnpm run lint`

Expected: PASS. Do not run `pnpm build` or `pnpm dev`.

- [ ] **Step 8: Review the final diff and tree identity**

Run: `git status --short` and `git diff --stat` followed by `git diff --check`.

Confirm every changed path belongs to the approved feature-removal design, no generated bundles or changelogs changed, no secrets or debug code were introduced, and the only documentation artifacts outside product code are the approved design and implementation plan. Do not commit or stage files.
