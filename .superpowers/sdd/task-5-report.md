# Task 5 Report: Bundled Drake Resource Themes and Catalog v2

## Status

Implemented bundled `drake-light` and `drake-ayu` resource-theme directories with semantic QingYu/Milkdown styling, the four supplied JetBrains Mono Patch WOFF2 faces, exact Drake MIT licensing, and the verified upstream font OFL. Catalog initialization is now versioned: version 0 installs the original 18 CSS seeds plus both Drake packages, version 1 installs only missing Drake IDs, and version 2 performs read-only scan/diagnostic work.

## RED evidence

The Task 5 tests were added before package files or catalog-v2 implementation.

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::migration::tests
```

Exited 101 with the intended missing surface: unresolved `materialize_embedded_theme`, `DRAKE_THEME_PACKAGES`, and `should_migrate_legacy_preferences`, plus the old Boolean initialization signature rejecting numeric catalog versions.

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short drake
```

Exited 101 for the same absent Drake package/catalog-v2 APIs.

```bash
pnpm --filter @markra/app test -- src/styles.test.ts
```

Exited 1 with two expected fixture failures: the shared bundled-theme loop and the Drake semantic-resource test could not read package-directory `theme.css` files because neither Drake directory existed.

Self-review added a persistent occupied-target diagnostic regression before changing production code:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::migration::tests::occupied_drake_destination_is_diagnostic_and_does_not_block_other_seed -- --exact --nocapture
```

Exited 101 with one failed assertion (`left: 0`, `right: 1`) because version-2 initialization discarded the diagnostic after the migration run. The read-only v2 diagnostic pass then made the test green without restoring or rewriting any theme.

## GREEN evidence

Final focused commands were run sequentially:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::migration::tests -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml drake -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes:: -- --nocapture
pnpm --filter @markra/app exec vitest run src/styles.test.ts --reporter=verbose
rustfmt --edition 2021 --check apps/desktop/src-tauri/src/themes/catalog.rs apps/desktop/src-tauri/src/themes/migration.rs
git diff --check
```

Results:

- migration: 4 passed, 0 failed;
- Drake validation/migration selection: 4 passed, 0 failed;
- complete native theme regression: 114 passed, 0 failed;
- stylesheet fixtures: 41 passed, 0 failed;
- focused Rust formatting and whitespace checks: exit 0.

## Review correction TDD wave

The review regressions were added together before changing production code:

- a valid same-ID but byte-different directory replaces the freshly written staging path after writes and before validation;
- a user directory appears at `drake-light` immediately before the atomic no-replace publication;
- the first generated owned staging name is already occupied;
- an injected error occurs immediately after staging directory creation, before the no-follow open and metadata checks.

RED was captured with:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml drake -- --nocapture
```

It exited 101 with the intended missing behavior surface: unresolved `EmbeddedSeedHookPoint` and `materialize_embedded_theme_with_hook`, plus no `seed_missing_drake_with_hook` method. The failures therefore came from the unimplemented retained-staging, retry, and publication-race contracts rather than a fixture typo.

GREEN was then verified sequentially:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::catalog::tests::drake -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::catalog::tests::failure_after_drake_staging_create_leaves_no_owned_residue -- --exact --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::catalog::tests::embedded_materialization_rejects_a_same_id_replacement_before_validation -- --exact --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes:: -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm --filter @markra/app exec vitest run src/styles.test.ts --reporter=verbose
pnpm --filter @markra/app test
pnpm test
```

Results:

- publication-race and staging-collision tests: 2 passed, 0 failed;
- create-before-open cleanup test: 1 passed, 0 failed;
- retained-root replacement test: 1 passed, 0 failed;
- complete native theme regression: 118 passed, 0 failed;
- complete native suite: 590 passed, 0 failed, 21 ignored;
- stylesheet fixture: 41 passed, 0 failed;
- complete `@markra/app` suite: 120 files and 1,988 tests passed;
- complete pnpm workspace suite: exit 0, including desktop 212 tests and web 45 tests.

The fix validates the materialized package through the originally retained `Dir`, checks its addressed identity and exact `include_bytes!` file graph before and after the production validator, and compares every validated relative path and byte sequence. An RAII cleanup guard is armed immediately after `create_dir`; owned-name `AlreadyExists` retries without touching the occupant. A target created between the final precheck and atomic rename is preserved, reported as an occupied diagnostic, and does not prevent the other Drake package from seeding.

## Source assets, licensing, and byte identity

Supplied source directory:

```text
/Users/ying/Downloads/DrakeTyporaTheme-2.9.6
```

The four files below were copied without conversion from its `drake/` directory into both packages. `cmp` succeeds for every source/destination pair, every destination begins with `wOF2`, and `file` identifies all four as WOFF2 TrueType version `2.15860`.

```text
JetBrainsMono-Bold.woff2
df3f86c04988d8f7fc516db3e95ec6b630cdc67bec91fe4297c6f8e132be1037

JetBrainsMono-BoldItalic.woff2
3aa30cac2529ca86f6b8ef479f143d924378682657510541d10d8e8b6d07120b

JetBrainsMono-Italic.woff2
9aef9fe9f1292b1cc4b1af075e4e9bc5f2adf23fef54908e58e2ebe338f33a65

JetBrainsMono-Regular.woff2
bceff0710e3a7fe5b3622265c48b6fbc055cf071df80ef5f36ffc69550296664
```

The supplied Drake `LICENSE` is copied byte-for-byte to each `THEME-LICENSE.txt`; all three files have SHA-256:

```text
e898849bad58ec301c6fe47e6a91b6d6ca87e47691c35b3746af3d10897a1452
```

The supplied Drake archive did not contain the font license. Its README identifies the bundled files as the author's JetBrains Mono Patch. The OFL was therefore verified against the patch repository's `patch4` branch and stored verbatim except for removing one trailing space so repository whitespace checks remain clean:

```text
https://raw.githubusercontent.com/liangjingkanji/JetBrainsMono-patch/patch4/OFL.txt
```

Both `FONT-LICENSE.txt` copies contain that normalized text and have SHA-256:

```text
60d55f23c6ce05a81099a762cb67ca2c9b6ea251c7912720998b4c89ebfd4faa
```

The OFL declares `Copyright 2020 The JetBrains Mono Project Authors` and the SIL Open Font License 1.1. Both manifests declare the theme and font license paths, and the production directory validator verifies their existence and UTF-8 content.

## Implementation details and requirement review

- Both packages have exactly eight files: `manifest.json`, `theme.css`, four WOFF2 faces, and two declared license texts.
- Manifests use IDs `drake-light` / `drake-ayu`, appearances `light` / `dark`, source version `2.9.6`, and Drake-derived preview palettes.
- CSS retains the upstream MIT notice and contains direct relative WOFF2 `@font-face` rules for regular 400, italic 400, bold 700, and bold-italic 700.
- JetBrains Mono is applied to the application root, visual Markdown body/headings, source editor custom property, inline code, fenced code, and code controls.
- Stable QingYu selectors cover centered H1, underlined H2 and heading hierarchy, links, blockquotes/callouts, pill highlights, inline/fenced code, syntax colors, striped tables, `kbd`, task checkboxes, Mermaid content, and current-color Lucide/editor icons.
- Both CSS files pass the production package CSS/resource validator. They contain no `@import`, network font URL, `@include-when-export`, Typora panel/menu selector, or CodeMirror selector.
- Embedded packages are modeled as explicit relative-path/`include_bytes!` arrays. Materialization uses a retained regular parent and staging `Dir`, immediately armed cleanup, bounded name-collision retries, no-follow directory/file opens, file sync, exact graph-and-byte checks on both sides of retained-capability production validation, and atomic catalog no-replace rename.
- Atomic publication races preserve the newly arrived user entry, return an occupied-target diagnostic, clean only the owned staging directory, and continue with the remaining embedded package.
- A user-owned theme with either Drake ID wins regardless of storage kind. A pre-existing destination is never overwritten and remains a diagnostic on the migration run and later v2 startup scans.
- Version 0 seeds the 18 legacy CSS files and both current resource packages. Version 1 calls only Drake seeding, so deleted legacy seeds stay deleted. Version 2 never writes seeds.
- The v1 branch advances only `themeCatalogVersion` to 2 and returns before reading or writing legacy appearance/light/dark selection keys. Selection is therefore preserved.
- Repeating version-2 initialization is content-idempotent. No sync, activation, runtime, mobile, or UI work was added.

## Files changed

- `apps/desktop/src-tauri/themes/third-party/drake-light/`: manifest, QingYu CSS, four supplied fonts, MIT license, and OFL.
- `apps/desktop/src-tauri/themes/third-party/drake-ayu/`: manifest, QingYu CSS, four supplied fonts, MIT license, and OFL.
- `apps/desktop/src-tauri/src/themes/catalog.rs`: embedded package tables, validated no-follow materialization, missing-only seeding, occupied-target diagnostics, and Drake production-fixture tests.
- `apps/desktop/src-tauri/src/themes/resources.rs`: retained-directory entry point for the existing production package validator.
- `apps/desktop/src-tauri/src/themes/migration.rs`: ordered v0/v1/v2 initialization, selection-preserving v1 path, diagnostic merging, and incremental migration tests.
- `packages/app/src/styles.test.ts`: mixed CSS/package-directory fixture lookup and semantic/self-contained Drake assertions.
- `.superpowers/sdd/task-5-report.md`: TDD, verification, license, and hash evidence.

`themes/mod.rs` required no Task 5 edit because Task 1-4 already registered and exposed the catalog, manifest, and production resource validator used here.

## Concerns

No functional concern remains within Task 5. Runtime stylesheet activation and asset-scope lifetime are intentionally deferred to Task 6 onward, so this task verifies stored/seeded package correctness rather than live WebView font loading.

---

# Task 5 implementation report

## Outcome

Implemented one primary-window-owned notebook switch/restore transaction across desktop and managed workspaces. The transaction flushes dirty documents, raises a module-visible sync barrier, drains already-started old-root sync work (including paired settings-apply callers), commits the new primary root, waits for that root to mount, records a successful canonical recent notebook, releases the barrier, and requests the visible launch sync. Failed pre-commit work leaves the old root authoritative; failed post-commit network work does not roll back the new local root.

Desktop restore now prepares exactly one validated child below a canonical parent and bootstraps it using a native-derived notebook identity before primary ownership is committed. Settings only emits `qingyu://notebook-switch-requested`; the main primary window validates/coalesces requests and owns persistence.

## RED evidence

Initial exact TypeScript gate:

```text
pnpm --filter @markra/app exec vitest run src/hooks/useNotebookSwitchCoordinator.test.tsx src/hooks/useAppSyncCoordinator.test.tsx src/App.test.tsx --environment jsdom --globals
```

Failed first at `result.current.beginNotebookSwitch is not a function`, proving the missing barrier/drain contract. Because that assertion exited while its deliberately deferred native run was still unresolved, 14 later AppSync tests timed out as a cascade; they were not independent product failures.

Initial native bootstrap gate:

```text
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config
```

Failed to compile because `SyncApplicationRequest` had no `bootstrap` field and required a standalone `notebook_name`.

Initial desktop-target gate:

```text
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml primary_workspace
```

Failed to compile because `prepare_desktop_notebook_target_at_path` did not exist (along with the then-unresolved bootstrap request shape).

During App integration, the new onboarding routing test found that passing `switchDesktopNotebook` directly as a React click handler supplied the synthetic click event as the optional path. The App callback was corrected to call `switchDesktopNotebook()` explicitly.

## Implemented behavior

- Added `beginNotebookSwitch` / `finishNotebookSwitch` to AppSync. Beginning a switch raises the shared barrier, invalidates the old generation so queued work cancels at `shouldStart`, and drains started runs plus all registered caller processing. It does not abort a native paired settings transaction.
- Added the central notebook switch coordinator with desktop/mobile switch and restore paths, mount acknowledgement, recent-notebook updates only after successful central commits, post-mount launch sync, primary-owner checks, and deterministic last-request coalescing.
- Removed picker/managed-root convenience mutations from `usePrimaryWorkspace`; it now exposes persistence commits used by the coordinator.
- Added validated notebook-switch events. Settings emits a request rather than mutating primary state.
- Added bootstrap sync request typing without `notebookName`; normal requests retain an exact immutable name. Desktop native validation derives the bootstrap identity from the canonical target basename and revalidates the result.
- Added the desktop native target-preparation command, including canonical parent validation, exact single-segment validation, symlink/non-directory rejection, secure create-or-reuse behavior, and canonical result checks.
- Bootstrap uses a complete configured snapshot even if global sync is disabled; ordinary application sync still requires the ready/enabled snapshot.
- Added recent notebook persistence (separate from recent external folders) with canonical successful roots only.

## Final verification

All final commands passed:

```text
pnpm --filter @markra/app exec vitest run src/hooks/useNotebookSwitchCoordinator.test.tsx src/hooks/useAppSyncCoordinator.test.tsx src/App.test.tsx --environment jsdom --globals
3 files, 290 tests passed

pnpm --filter @markra/app exec vitest run src/components/SettingsWindow.test.tsx --environment jsdom --globals
1 file, 2 tests passed

pnpm typecheck:test
9 of 10 workspace projects passed

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config
61 passed, 0 failed

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::service
14 passed, 0 failed

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml primary_workspace
20 passed, 0 failed

cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
passed

git diff --check
passed
```

## Fourth-round listener handoff reconciliation remediation

The final Task 5 handoff audit found one remaining eventless gap: after the old root's snapshot and while React replaced its listeners with the new root's listeners, the application-level registry could accept a pending apply without a frontend event. The cached switch identity therefore remained empty and the successful root change released its barrier without cancelling the native pending entry.

### Listener transition plus final authoritative snapshot

- A root-changed finish now waits for the current listener registration attempt to settle before taking one final authoritative `loadEditing` snapshot. Listener registration rejection is already converted into a settled error state, so the final snapshot still runs.
- The listener wait races registration settlement with that effect's cleanup signal. A finish that captured an old effect cannot wait forever merely because React disposed that effect during the root handoff.
- The final snapshot has priority at the same counter. A later listener-observed identity may win only when its counter is strictly greater, so stale cached state cannot replace the newer native registry identity.
- The selected non-completed identity is cancelled by exact `sessionId + revision + token` before the switch barrier is released. Only that exact identity is claimed and cleared locally; a newer native token installed during cancellation survives a stale cancellation mismatch.
- A completed or empty authoritative snapshot clears only cached pending state at or below its counter. There is no retry loop.
- The new listener may bootstrap from the old editing snapshot while the switch barrier is raised. Successful root change settlement therefore clears the old local editing and launch identities before release, allowing exactly one new-root app-launch after native cancellation settles.
- Failed same-root completion keeps the existing reclaim path: it performs no cancellation and publishes the pending apply after the barrier is released.

RED: a real default memory registry `requestApply` made directly during the A-to-B listener handoff produced a native pending entry but emitted no frontend event. `finishNotebookSwitch` made zero cancellation calls. A second test showed that this missing reconciliation could not exercise the exact-cancel mismatch guarantee.

GREEN: the eventless old identity is cancelled exactly, new-root app-launch remains blocked until cancellation settles, and a later new-root token publishes once despite repeated events. When a newer token is installed during exact cancellation, the stale second cancellation mismatches and the new native pending identity remains unchanged.

### Fourth-round verification

All commands exited successfully:

```text
pnpm --filter @markra/app exec vitest run src/hooks/useAppSyncCoordinator.test.tsx --environment jsdom --globals --silent=passed-only --reporter=dot
1 file, 24 tests passed

pnpm --filter @markra/app exec vitest run src/runtime/index.test.ts src/hooks/useNotebookSwitchCoordinator.test.tsx src/hooks/useAppSyncCoordinator.test.tsx src/App.test.tsx --environment jsdom --globals --silent=passed-only --reporter=dot
4 files, 307 tests passed

pnpm typecheck:test
9 of 10 workspace projects passed

pnpm test
all workspace projects passed
packages/app: 127 files, 1992 tests passed
apps/desktop: 18 files, 211 tests passed
apps/web: 9 files, 45 tests passed

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config::editing
5 passed, 0 failed

cargo check --tests --manifest-path apps/desktop/src-tauri/Cargo.toml
passed with no warnings

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
601 passed, 22 ignored, 0 failed under default parallel execution

pnpm build
passed across all workspace projects, including desktop vendor-chunk verification

git diff --check
passed
```

No Task 6 or Task 7 UI, external-folder removal, registry/UUID remote identity, MCP expansion, or unrelated behavior was included.

## Third-round native apply settlement remediation

The final Task 5 review found that hiding an old apply only in the frontend left the application-level native editing registry in `Pending`. A later settings session therefore received `sync-apply-pending` even after a successful notebook root change.

### Exact application-level cancellation contract

- Added `cancel_sync_config_apply` to the shared desktop and mobile command surfaces and to both TypeScript runtimes.
- Cancellation requires the exact `sessionId + revision + token` identity. A mismatch returns before the counter or entry changes.
- A matching `Pending` or `Claimed` entry becomes `Completed` with the deterministic `sync-apply-cancelled` error and notifies every watch waiter.
- Repeating cancellation for the same completed identity is an idempotent no-op. Once a new token replaces that completed entry, a stale cancellation mismatches and cannot touch the new token.
- The web/default memory runtime now models native session checks, pending-token rejection, exact duplicate identity, cancellation, and completed-entry replacement instead of overwriting any pending token.

Native RED: the test registry had no `cancel_apply` contract. Memory RED: a second token incorrectly resolved and replaced the first pending token. GREEN: a claimed old token is cancelled, its waiter receives the cancellation error, completed cancellation is idempotent, mismatch preserves the full snapshot and counter, and a new session can register its new token.

### Root switch settlement ordering

- `beginNotebookSwitch` retains the latest exact pending identity from both the native snapshot and barrier-time events.
- A failed same-root transaction does not cancel; it releases the barrier and reclaims/publishes the pending apply.
- A successful root change awaits exact native cancellation before releasing the barrier or launching new-root sync. The old identity is then claimed locally so it cannot revive.
- `useNotebookSwitchCoordinator` now awaits `finishNotebookSwitch`, so new-root launch work cannot overtake native apply settlement.
- A new session/token after A to B registers successfully and repeated exact events publish it once.

Frontend RED: A to B made zero cancellation calls, and new-root launch ran before an asynchronous finish settled. GREEN: exact cancellation is awaited once, failed A to A makes no cancellation call and settles normally, and the new token publishes once.

### Third-round verification

```text
pnpm --filter @markra/app exec vitest run src/runtime/index.test.ts src/hooks/useNotebookSwitchCoordinator.test.tsx src/hooks/useAppSyncCoordinator.test.tsx src/App.test.tsx --environment jsdom --globals --silent=passed-only --reporter=dot
4 files, 305 tests passed

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config::editing
5 passed, 0 failed

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests
23 passed, 0 failed

pnpm typecheck:test
passed

cargo check --tests --manifest-path apps/desktop/src-tauri/Cargo.toml
passed with no warnings

pnpm test
passed across all workspace projects
packages/app: 127 files, 1990 tests passed
apps/desktop: 18 files, 211 tests passed
apps/web: 9 files, 45 tests passed

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
601 passed, 22 ignored, 0 failed under default parallel execution

pnpm build
passed across all workspace projects; desktop imported 12 vendor chunks successfully
```

Focused tests cover picker cancellation, flush/drain aborts, active paired settings drain, queued stale-run cancellation, stale completion suppression, transaction order, failed post-commit sync retention, failed bootstrap preservation, exactly-once successful restore commit, recent notebook rules, main-window ownership, request validation/coalescing, Settings emission, and App onboarding routing.

## Scope notes and concerns

- `notebook_scope.rs` and `remote_sync/service.rs` required no production edit: their existing canonical-name derivation and paired sync contract are reused. Their focused tests were run to confirm compatibility.
- `SettingsWindow.tsx` and its focused test were necessarily changed in addition to the brief's file list so the settings surface obeys the emitter-only rule.
- No Task 6 notebook-management UI, Task 7 external-folder removal, registry/UUID work, or MCP expansion was included.
- No known remaining Task 5 blocker. Full repository `pnpm test` / `pnpm build` were outside the required focused gate; the exact App suite, workspace typecheck, and native focused suites are green.

## Independent-review remediation

The follow-up review identified four correctness gaps. This section supersedes the earlier scope notes where they conflict with the final implementation.

### Desktop restore authority is now an opaque one-use native lease

- Desktop preparation returns `{ lease, notesRoot }`; the frontend sends only the opaque lease during bootstrap and cannot independently choose the native bootstrap path or notebook name.
- Native preparation opens the canonical parent and selected child without following symlinks, retains both directory capabilities plus their filesystem identities, and stores them in a process-local capability map keyed by 192 bits of random entropy. The lease is removed before validation, so success and failure both consume it exactly once.
- Consumption revalidates the ambient parent, child type, child identity, retained handles, and exact canonical path before any sync scan or network operation.
- `RemoteSyncScope` now owns a retained source-directory capability. The sync engine scans, reads, writes, conflicts, and deletes relative to that retained capability rather than reopening the ambient path.
- Mobile bootstrap remains path-based only for the managed app-data workspace and still validates managed containment natively.

RED: `prepared_desktop_restore_target_rejects_replacement_before_any_sync_action` initially failed to compile because the lease preparation and consumption functions did not exist. GREEN: the test deterministically prepares the target, replaces it with a symlink to an outside same-basename directory, and proves consumption fails with zero sync actions.

### Settings apply is owned from reload through native publication

- A settings apply lifecycle is registered before its pending token is cleared and remains registered across reload and its paired native publication.
- Beginning a notebook switch disables new listener events and drains old-root settings lifecycles in addition to already-started shared runs.
- An already-owned settings apply may finish publication after generation invalidation; a failed same-root switch re-enables the existing listeners so subsequent applies are not permanently blocked.

RED: `owns a settings apply from reload through publication across a failed same-root switch` observed zero native publications when the switch began during deferred reload. GREEN: the first token publishes while switch drain waits, then a second token publishes after the failed same-root switch finishes.

### Running state is generation-owned

- Beginning a switch assigns the running counter to the new generation and resets it to zero.
- Completion from the invalidated generation cannot decrement the replacement generation.

RED: `does not carry an old running count into the replacement switch generation` completed a second deferred manual run with `running === true`. GREEN: after the generation reset, the second run ends with `running === false`.

### Recent notebook identity remains byte-exact

- Notebook recents now have a dedicated normalizer instead of reusing external-folder normalization.
- Nonempty names and paths preserve leading/trailing spaces, case, and Unicode bytes. Deduplication uses only exact path equality.
- External recent Markdown files and folders retain their existing trimming and fallback-name behavior.

RED: the exact normalizer and local-state round-trip tests showed both name and path were trimmed, trimmed paths were incorrectly deduplicated, and an empty notebook name was synthesized. GREEN: both test files pass with exact values preserved.

## Final remediation verification

All final commands passed:

```text
git diff --check
passed

pnpm typecheck:test
9 of 10 workspace projects passed

pnpm test
all workspace projects passed
packages/app: 127 files, 1984 tests passed
apps/desktop: 18 files, 211 tests passed

cargo check --tests --manifest-path apps/desktop/src-tauri/Cargo.toml
passed with no warnings

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
595 passed, 22 ignored, 0 failed

pnpm build
all workspace projects passed, including desktop vendor-chunk verification
```

No Task 6 or Task 7 UI, registry/UUID remote identity, MCP expansion, or unrelated product behavior was added. There are no known remaining Task 5 blockers.

## Second-round Important remediation

The second review closed the three remaining Task 5 race and authority gaps without adding Task 6 or Task 7 behavior.

### Prepared desktop leases have explicit cleanup on every exit

- Added a desktop-only native discard command and desktop runtime bridge. The mobile command surface remains unchanged.
- Discard is idempotent, consumption remains single-use, and both operations remove the retained capability from the same synchronized registry.
- Desktop restore discards in `finally` whether bootstrap consumes the lease, pre-bootstrap flushing fails, or the returned native target fails frontend consistency validation.
- Native tests cover registry baseline restoration, repeated discard, consume-then-discard, replay rejection, and concurrent consumers.

RED: `discards an unconsumed prepared restore lease when flushing fails` observed zero discard calls. A final static review added `discards a prepared restore lease when native target validation fails`, which also observed zero discard calls before the `finally` boundary was widened. GREEN: both paths discard exactly once; native replay and concurrency tests pass.

### Prepared sync reads ignore policy through retained authority

- `RemoteSyncScope` constructs note ignore rules through its retained `Dir`; it no longer reopens `.markraignore` through the ambient source path.
- The ordinary path-backed scope uses the same retained-directory parser and preserves global/workspace precedence.

RED: after retaining a source directory, renaming it, and installing an opposite `.markraignore` at the old ambient path, the prepared sync followed the replacement rules. GREEN: it uploads only the file allowed by the retained directory's rules; the ordinary ignore-rule regression test also passes.

### Barrier-time settings apply is deterministically recovered

- Apply events received during an active notebook-switch barrier are buffered by monotonic counter and exact identity.
- Failed same-root completion reclaims the latest native editing snapshot and awaits the recovered publication lifecycle.
- A successful root change marks the old snapshot handled before enabling the new root, so later same-root switches cannot revive it.
- Claimed apply identities suppress repeated publication; older events cannot replace a newer buffered apply.

RED: failed same-root, changed-root, and repeated-event tests initially observed respectively zero publication, old-root publication against the new root, and no recovered publication. A stronger changed-root replay check then exposed that a later same-root barrier could revive the old token. GREEN: the failed same-root token settles once, changed-root tokens never publish or revive, and only the newest repeated token publishes once.

### Second-round verification

The remediation commit is `fix: close task 5 switch race gaps`. Its final gates are recorded below; the commit hash is reported by the task handoff.

```text
pnpm --filter @markra/app exec vitest run src/hooks/useNotebookSwitchCoordinator.test.tsx src/hooks/useAppSyncCoordinator.test.tsx src/App.test.tsx --environment jsdom --globals --silent=passed-only --reporter=dot
passed

pnpm typecheck:test
passed

pnpm test
passed across all workspace projects

cargo check --tests --manifest-path apps/desktop/src-tauri/Cargo.toml
passed with no warnings

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
599 passed, 22 ignored, 0 failed under default parallel execution

pnpm build
passed across all workspace projects, including desktop vendor-chunk verification

cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
passed

git diff --check
passed
```
