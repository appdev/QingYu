# QingYu Recycle Bin Retention Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a preset, application-owned retention policy that safely cleans expired QingYu recycle-bin entries without manual day input.

**Architecture:** Extend the shared MCP configuration with the closed numeric set `0 | 7 | 30 | 90`. A new Rust recycle maintenance module performs root-confined, metadata-driven cleanup; the MCP runtime invokes it immediately when appropriate and once every 24 hours. The React settings surface conditionally renders a shared styled select when the QingYu recycle-bin deletion policy is selected.

**Tech Stack:** Rust, Tauri v2, Tokio, React 19, TypeScript, Vitest, Testing Library, pnpm.

## Global Constraints

- The control has exactly `Never automatically clean up`, `After 7 days`, `After 30 days`, and `After 90 days`.
- The default is 30 days; `0` means automatic cleanup is disabled.
- Unsupported values normalize to 30 days.
- Cleanup never accepts an MCP client path and only deletes valid direct UUID entries under the application-owned recycle root.
- The setting is global across MCP clients and only appears when `Deletion policy` is `QingYu recycle bin`.
- Existing uncommitted S3 work in the primary checkout must remain untouched.

---

### Task 1: Configuration contract

**Files:**
- Create: `packages/app/src/lib/mcp.test.ts`
- Modify: `packages/app/src/lib/mcp.ts`
- Modify: `apps/desktop/src-tauri/src/mcp/config.rs`
- Test: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: TypeScript `McpRecycleBinRetentionDays = 0 | 7 | 30 | 90`.
- Produces: `McpConfig.recycleBinRetentionDays` in TypeScript and `McpConfig.recycle_bin_retention_days: u16` in Rust.
- Produces: normalized values that downstream cleanup can trust.

- [ ] **Step 1: Write failing TypeScript configuration tests**

Create tests that assert the default is 30 and normalization accepts only `0`, `7`, `30`, and `90`:

```ts
expect(defaultMcpConfig().recycleBinRetentionDays).toBe(30);
for (const value of [0, 7, 30, 90] as const) {
  expect(normalizeMcpConfig({ recycleBinRetentionDays: value }).recycleBinRetentionDays).toBe(value);
}
expect(normalizeMcpConfig({ recycleBinRetentionDays: 180 }).recycleBinRetentionDays).toBe(30);
```

- [ ] **Step 2: Run TypeScript tests and confirm RED**

Run: `pnpm --filter @markra/app exec vitest run src/lib/mcp.test.ts`

Expected: FAIL because `recycleBinRetentionDays` does not exist.

- [ ] **Step 3: Write failing Rust configuration tests**

Extend `config_defaults_are_disabled_and_use_the_approved_policy` and `config_limits_are_clamped_before_revisioning` to require a default of 30 and normalization of unsupported values back to 30 while preserving `0`, `7`, and `90`.

- [ ] **Step 4: Run Rust tests and confirm RED**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::config_ --quiet`

Expected: compilation failure because `recycle_bin_retention_days` does not exist.

- [ ] **Step 5: Implement the minimal cross-language configuration**

Add the field to both MCP config structures, use 30 in both defaults, and normalize through an allow-list:

```ts
export type McpRecycleBinRetentionDays = 0 | 7 | 30 | 90;

function recycleBinRetentionDaysOr(value: unknown): McpRecycleBinRetentionDays {
  return value === 0 || value === 7 || value === 30 || value === 90 ? value : 30;
}
```

```rust
const DEFAULT_RECYCLE_BIN_RETENTION_DAYS: u16 = 30;

fn normalize_recycle_bin_retention_days(value: u16) -> u16 {
    match value {
        0 | 7 | 30 | 90 => value,
        _ => DEFAULT_RECYCLE_BIN_RETENTION_DAYS,
    }
}
```

- [ ] **Step 6: Run configuration tests and confirm GREEN**

Run both commands from Steps 2 and 4. Expected: PASS.

- [ ] **Step 7: Commit the configuration contract**

```bash
git add packages/app/src/lib/mcp.ts packages/app/src/lib/mcp.test.ts apps/desktop/src-tauri/src/mcp/config.rs apps/desktop/src-tauri/src/mcp/tests.rs
git commit -m "feat: add recycle bin retention policy"
```

### Task 2: Safe recycle cleanup engine

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp/recycle.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`

**Interfaces:**
- Consumes: normalized retention days from `McpConfig`.
- Produces: `clean_expired_entries(root: &Path, retention_days: u16, now_ms: u64) -> RecycleCleanupReport`.
- Produces: counts for removed, skipped, and failed entries without returning client-visible filesystem paths.

- [ ] **Step 1: Write failing cleanup unit tests in the new module**

Test these independent behaviors with temporary directories:

```rust
assert_eq!(clean_expired_entries(root, 0, now).removed, 0);
assert_eq!(clean_expired_entries(root, 7, now).removed, 1); // exactly at cutoff
assert!(recent_entry.exists());
assert!(malformed_entry.exists());
```

On Unix, create a symlink named with a UUID and assert its target remains untouched.

- [ ] **Step 2: Run cleanup tests and confirm RED**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::recycle::tests --quiet`

Expected: compilation failure because the cleanup function is not implemented.

- [ ] **Step 3: Implement the root-confined cleanup pass**

The implementation must:

```rust
if retention_days == 0 { return RecycleCleanupReport::default(); }
let cutoff = now_ms.saturating_sub(u64::from(retention_days) * 86_400_000);
```

For each direct child, require a UUID name, reject symlinks and non-directories using `symlink_metadata`, parse only `metadata.json`, compare `deleted_at <= cutoff`, and use `remove_dir_all` only on that validated child path. Read and deletion failures increment report counters and do not abort the pass.

- [ ] **Step 4: Run cleanup tests and confirm GREEN**

Run the command from Step 2. Expected: PASS, including the symlink boundary test on Unix.

- [ ] **Step 5: Commit the cleanup engine**

```bash
git add apps/desktop/src-tauri/src/mcp/recycle.rs apps/desktop/src-tauri/src/mcp/mod.rs
git commit -m "feat: clean expired recycle bin entries"
```

### Task 3: MCP lifecycle integration

**Files:**
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Test: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Consumes: `McpConfig.enabled` and `McpConfig.recycle_bin_retention_days`.
- Consumes: `clean_expired_entries` from Task 2.
- Produces: an immediate cleanup decision and a 24-hour background maintenance loop.

- [ ] **Step 1: Write failing lifecycle policy tests**

Add tests for a small pure helper that returns no retention when MCP is disabled or retention is zero, and returns the selected days when enabled:

```rust
assert_eq!(recycle_retention_for_cleanup(&disabled), None);
assert_eq!(recycle_retention_for_cleanup(&never), None);
assert_eq!(recycle_retention_for_cleanup(&enabled), Some(7));
```

- [ ] **Step 2: Run lifecycle tests and confirm RED**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::recycle_cleanup --quiet`

Expected: FAIL because the lifecycle helper does not exist.

- [ ] **Step 3: Implement immediate and daily cleanup**

Store the recycle root in `McpState`. Spawn one application-lifetime Tokio interval with a 24-hour period and `MissedTickBehavior::Skip`; its first tick runs immediately. Before every pass, read the latest MCP config and skip when the helper returns `None`. Execute filesystem cleanup through `spawn_blocking` and log only aggregate failures.

After a revision-checked settings update, run an immediate pass when MCP is enabled and either the enabled state or retention value changed. Cleanup failure remains best effort and does not fail the settings update.

- [ ] **Step 4: Run lifecycle and full MCP tests and confirm GREEN**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::recycle_cleanup --quiet
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests --quiet
```

Expected: PASS.

- [ ] **Step 5: Commit lifecycle integration**

```bash
git add apps/desktop/src-tauri/src/mcp/mod.rs apps/desktop/src-tauri/src/mcp/tests.rs
git commit -m "feat: schedule recycle bin cleanup"
```

### Task 4: Preset settings control and localized copy

**Files:**
- Modify: `packages/app/src/components/settings/McpSettings.tsx`
- Modify: `packages/app/src/components/settings/McpSettings.test.tsx`
- Modify: `packages/app/src/components/settings/SettingsControls.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`
- Modify: `packages/shared/src/i18n/index.test.ts`

**Interfaces:**
- Consumes: `McpConfig.recycleBinRetentionDays` from Task 1.
- Produces: a conditional `SettingsSelect` that writes one of `0 | 7 | 30 | 90` through `updateSettings`.

- [ ] **Step 1: Write failing React tests**

Test that the control is absent under System Trash, appears after selecting QingYu recycle bin, has exactly four options, defaults to 30 days, and writes `0` when Never is selected:

```ts
expect(screen.queryByRole("combobox", { name: "Recycle bin cleanup" })).not.toBeInTheDocument();
fireEvent.change(screen.getByRole("combobox", { name: "Deletion policy" }), {
  target: { value: "qing-yu-recycle-bin" }
});
const cleanup = await screen.findByRole("combobox", { name: "Recycle bin cleanup" });
expect(cleanup).toHaveValue("30");
fireEvent.change(cleanup, { target: { value: "0" } });
expect(mcp.updateSettings).toHaveBeenCalledWith(expect.objectContaining({
  config: expect.objectContaining({ recycleBinRetentionDays: 0 })
}));
```

- [ ] **Step 2: Run React tests and confirm RED**

Run: `pnpm --filter @markra/app exec vitest run src/components/settings/McpSettings.test.tsx`

Expected: FAIL because the cleanup combobox is absent.

- [ ] **Step 3: Implement shared-select styling support and MCP row**

Allow `SettingsSelect` to accept an optional `className` and `disabled` without changing its existing visual contract. Render the new row immediately below deletion policy only for `qing-yu-recycle-bin`, pass `min-h-11 min-w-11 w-full` in compact mode, and convert the selected string to the closed numeric union before updating the configuration.

Add locale keys for the row and four options in English, Simplified Chinese, and Traditional Chinese. Add the keys to `I18nKey` and the i18n coverage test.

- [ ] **Step 4: Run UI and i18n tests and confirm GREEN**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/McpSettings.test.tsx src/lib/mcp.test.ts
pnpm --filter @markra/shared test
```

Expected: PASS.

- [ ] **Step 5: Commit the settings control**

```bash
git add packages/app/src/components/settings/McpSettings.tsx packages/app/src/components/settings/McpSettings.test.tsx packages/app/src/components/settings/SettingsControls.tsx packages/shared/src/i18n/locales/types.ts packages/shared/src/i18n/locales/en.ts packages/shared/src/i18n/locales/zh-CN.ts packages/shared/src/i18n/locales/zh-TW.ts packages/shared/src/i18n/index.test.ts
git commit -m "feat: configure recycle bin cleanup"
```

### Task 5: Documentation and verification

**Files:**
- Modify: `docs/qingyu-mcp.md`

**Interfaces:**
- Consumes: the completed runtime and UI behavior.
- Produces: user-facing documentation that distinguishes QingYu recycle cleanup from System Trash.

- [ ] **Step 1: Document the retention policy**

Add a concise section stating that the setting is shown for QingYu recycle-bin deletion, defaults to 30 days, supports Never/7/30/90, and never controls System Trash.

- [ ] **Step 2: Run focused verification**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests --quiet
pnpm --filter @markra/app test
pnpm --filter @markra/shared test
pnpm typecheck:test
pnpm build
git diff --check
```

Expected: all commands pass with no warnings or whitespace errors attributable to this change.

- [ ] **Step 3: Commit documentation**

```bash
git add docs/qingyu-mcp.md docs/superpowers/plans/2026-07-23-qingyu-recycle-bin-retention.md
git commit -m "docs: explain recycle bin cleanup"
```

- [ ] **Step 4: Review final branch scope**

Run `git status --short`, `git diff main...HEAD --stat`, and `git log --oneline main..HEAD`. Confirm the branch contains only the design, plan, recycle retention implementation, tests, localization, and documentation.
