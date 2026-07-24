# Task 6 Report: Notebook Selection Surfaces

## Outcome

Task 6 is complete. Desktop onboarding now offers one local-notebook choice, cloud restore, standalone-file editing, and defer without an external-folder action. Desktop Settings and compact Settings route notebook changes through the central switch coordinator. True mobile now opens a named notebook manager that enumerates exact direct managed children, creates or switches one local notebook, and restores exactly one selected remote notebook without a filesystem picker.

The post-implementation review remediation is also complete. The native load contract now reports `configured` independently from `readiness`: complete-but-disabled sync configurations can list and bootstrap a selected notebook, while default or partially configured disabled documents route to the appropriate settings surface. Catalogs are bound to their configuration revision, mobile cancellation results are retryable failures, modal focus remains contained even while every control is disabled, and native disabled-reason codes never reach visible copy.

The wider external-folder runtime/menu chain remains intact for Task 7. No Task 7 removal was included.

## Product and Runtime Wiring

- `WelcomeScreen` keeps the approved identity-rail/task-pane composition and the slogan `明窗净几，字字轻语。`; no fake step marker remains.
- The welcome surface no longer declares or receives the unused `onOpenExternalFolder` callback.
- The old `PrimaryWorkspaceController.chooseDesktopRoot/createMobileRoot` methods are absent. The remaining onboarding UI props call `switchDesktopNotebook()` and the mobile notebook manager respectively.
- Desktop cloud restore checks the application-level sync configuration first. Incomplete configuration opens Sync settings; complete enabled or disabled configuration lists names for the exact revision and restores one selected name through `restoreDesktopNotebook`.
- `SyncConfigDocument.configured` is computed by validating a cloned configuration as enabled without mutating persisted state. Existing disabled `readiness` and empty `issues` semantics remain unchanged, so consumers no longer infer completeness from presentation state.
- `SettingsWindow` emits `qingyu://notebook-switch-requested`; the primary-window notebook coordinator owns the switch. Compact Storage calls `openNotebookManager`, wired to desktop switch or true-mobile notebook management according to form factor.
- True mobile obtains its local catalog from the native shallow `workspaces/` enumeration, not recent history. The enumeration keeps exact Unicode and ordinary spaces, excludes invalid/protected/symlink/non-directory children, does not create a missing collection, and sorts deterministically.
- Mobile create preserves the entered logical notebook name byte-for-byte, including ordinary leading/trailing spaces, matching the native name contract.
- Successful switch/restore closes the surface. Cancellation/null results and operation errors keep it open for retry.
- True-mobile incomplete configuration closes the notebook manager and sends an explicit navigation request to the real Compact Sync stack instead of calling the desktop settings-window API.
- Compact navigation requests compare the complete target page identity. A rejected push during an active transition completes safely instead of sticking; non-onboarding requests are consumed when their exact target opens, while onboarding keeps the Sync page active until the user returns to the editor.
- Remote catalogs record their listing revision as soon as a request starts. A configured/applied revision change closes the surface, invalidates the request generation, rejects stale restore actions, and ignores late async results on both desktop and mobile.
- Both notebook modals contain Tab and Shift+Tab focus, including zero-enabled-control busy states where focus remains on the dialog container; they retain Escape dismissal and previous-focus restoration, and map known or unknown native disabled reasons to localized safe copy.

## Files Changed

### Native managed notebook catalog and runtime boundary

- `apps/desktop/src-tauri/src/managed_workspace.rs`
- `apps/desktop/src-tauri/src/desktop_runtime.rs`
- `apps/desktop/src-tauri/src/mobile_runtime.rs`
- `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- `apps/desktop/src/runtime/desktop.ts`
- `apps/desktop/src/runtime/mobile.ts`
- `apps/desktop/src/runtime/index.test.ts`
- `apps/desktop/src/runtime/mobile.test.ts`
- `apps/desktop/src/runtime/tauri/managed-workspace.ts`
- `apps/desktop/src/runtime/tauri/managed-workspace.test.ts`

### Application surfaces and orchestration

- `packages/app/src/App.tsx`
- `packages/app/src/App.test.tsx`
- `packages/app/src/runtime/index.ts`
- `packages/app/src/index.ts`
- `packages/app/src/components/notebooks/RemoteNotebookDialog.tsx`
- `packages/app/src/components/notebooks/RemoteNotebookDialog.test.tsx`
- `packages/app/src/components/notebooks/MobileNotebookDialog.tsx`
- `packages/app/src/components/notebooks/MobileNotebookDialog.test.tsx`
- `packages/app/src/components/onboarding/WelcomeScreen.tsx`
- `packages/app/src/components/onboarding/WelcomeScreen.test.tsx`
- `packages/app/src/components/settings/NotesWorkspaceSettings.tsx`
- `packages/app/src/components/settings/NotesWorkspaceSettings.test.tsx`
- `packages/app/src/components/SettingsWindow.test.tsx`
- `packages/app/src/components/compact/CompactSettingsDetail.tsx`
- `packages/app/src/components/compact/CompactSettingsDetail.test.tsx`
- `packages/app/src/components/compact/CompactAppShell.tsx`
- `packages/app/src/components/compact/CompactAppShell.test.tsx`
- `packages/app/src/components/compact/types.ts`
- `packages/app/src/components/notebooks/dialog-focus.ts`
- `packages/app/src/components/notebooks/remote-notebook-disabled-reason.ts`
- `packages/app/src/styles.css`

### Localized copy and contract

- `packages/shared/src/i18n/locales/en.ts`
- `packages/shared/src/i18n/locales/zh-CN.ts`
- `packages/shared/src/i18n/locales/zh-TW.ts`
- `packages/shared/src/i18n/locales/types.ts`
- `packages/shared/src/i18n/index.test.ts`

### Final review contract hardening

- `apps/desktop/src-tauri/src/sync_config/model.rs`
- `apps/desktop/src-tauri/src/sync_config/storage.rs`
- `apps/desktop/src-tauri/src/sync_config.rs`
- `apps/desktop/src-tauri/src/remote_sync/mcp_service.rs`
- `packages/app/src/lib/sync-config.ts`
- `packages/app/src/lib/sync-config.test.ts`
- `packages/app/src/hooks/useSyncConfig.ts`
- `packages/app/src/hooks/useAppSyncCoordinator.ts`
- `packages/app/src/hooks/useCompactSyncSettings.ts`

## Hallmark Review

The implementation preserves the locked root `DESIGN.md`: system UI typography, neutral paper, ink-black interaction hierarchy, semantic app tokens, compact 4/8px rhythm, restrained 6–8px surfaces, Lucide icons, and motion-cut editorial behavior.

Pre-emit critique:

- Welcome: `P5 H5 E4 S5 R5 V4`
- Remote notebook dialog: `P5 H5 E5 S5 R5 V4`
- Mobile notebook sheet: `P5 H5 E5 S5 R5 V4`

The 58-gate sweep passes under the locked application-design contract. Page-only nav/footer/hero/enrichment/diversification gates are non-applicable rather than synthesized into the product UI. Gate 1 follows the locked `DESIGN.md` system-font requirement, which overrides catalog-display novelty. Concrete applicable checks passed:

- no gradients, nested marketing cards, fake chrome, emojis, mixed icon libraries, invented metrics, section kickers, italic headings, raw new colors, or celebratory success UI;
- no `transition-all`, scale-hover, overshoot easing, animated focus ring, layout-property animation, or unguarded spatial motion;
- semantic colors and existing font tokens only; error text uses the theme-aware `--danger`, and secondary copy on new surfaces uses AA-capable `--text-primary` rather than low-contrast `--text-secondary`;
- visible focus rings, active and disabled states, error/loading/retry/empty/merge states, Escape dismissal, Tab/Shift+Tab containment, focus restoration, and reduced-motion fallbacks;
- silent success closes the dialog/sheet instead of adding a redundant success toast;
- action labels remain one line; display headings use `overflow-wrap: anywhere` and `min-width: 0`;
- mobile input keeps a fixed one-pixel border, outline-based focus, equal 44px control height, reserved helper slot, native disabled state, cursor, and opacity;
- rendered pages/surfaces have no horizontal scrolling.

### Real viewport evidence

Rendered through Vite in headless Chromium and visually inspected. Temporary screenshots were moved into the worktree for inspection, then removed; no QA artifact remains.

- Desktop welcome: `320`, `375`, `414`, `768`, and `1280x800`.
- True-mobile welcome: `320`, `375`, `414`, and `768`.
- Desktop remote dialog: `320`, `375`, `414`, `768`, and `1280x800`.
- Mobile notebook sheet: `320`, `375`, `414`, and `768`.

At every measured width, document/body/surface scroll width stayed within the viewport. All action labels had `white-space: nowrap` with no text overflow. Buttons, inputs, radio rows, and notebook rows measured at least `44px`; the mobile primary action measured `48px`. Desktop welcome core content fit the `1280x800` first screen.

## Verification

All final gates passed after the final three-item review remediation:

- Exact Task 6 plus Compact navigation suite: 8 files, 335 tests.
- Desktop runtime managed-workspace suite: 3 files, 35 tests.
- Shared i18n contract: 1 file, 13 tests.
- `pnpm test`: all workspace projects passed; app 129 files / 2020 tests (JSON reporter: 279/279 suites), desktop 19 / 212, web 9 / 45, plus shared/UI/editor/markdown/scripts/site suites.
- `pnpm typecheck:test`: all 9 participating workspace packages passed.
- `pnpm build`: all workspace builds passed; desktop vendor-chunk verification imported 12 vendor chunks.
- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`: 604 passed, 0 failed, 22 ignored.
- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml managed_workspace`: 10 passed.
- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_`: 23 passed.
- `cargo check --tests --manifest-path apps/desktop/src-tauri/Cargo.toml`: passed with no warnings.
- `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check`: passed.
- `git diff --check`: passed.

The first full `pnpm test` run exposed one stale i18n test contract: the new reviewed Traditional Chinese Compact notebook labels were incorrectly included in the set required to retain English fallback copy. The focused failure reproduced consistently. The test's explicit localized-Compact whitelist was extended by exactly the two new keys, its 13-test suite passed, and the complete workspace suite then passed.

Review remediation followed one RED/GREEN cluster at a time. The final review added three explicit clusters: the load contract first failed to serialize/preserve `configured` and App incorrectly listed default/partial disabled configurations; Compact requests then failed to consume exact non-onboarding targets or clear rejected transition pushes; finally, both busy dialogs allowed Tab to escape when no enabled control existed. Each focused failure was observed before its scoped correction and each suite passed afterward. The earlier reload/React-commit race test still confirms that a newly opened catalog is not closed when its own reload returns the authoritative revision before state commit.

## Concerns

- No known Task 6 implementation blocker remains.
- Task 7 still owns removal/rerouting of the wider external-folder native/menu chain; this task deliberately changed only the welcome surface.
