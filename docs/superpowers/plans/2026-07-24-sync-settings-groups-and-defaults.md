# Sync Settings Groups and Defaults Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group loaded synchronization settings into scan-friendly sections and give newly created configurations safe S3-oriented defaults.

**Architecture:** Keep the existing `SettingsSection` primitive and render several sibling sections from `SyncSettings`. Change only `SyncConfig::default()` for persisted defaults, so create and reset paths share one source of truth and existing files stay untouched.

**Tech Stack:** React, TypeScript, Tailwind CSS, Vitest, React Testing Library, Rust, serde, Tauri v2.

## Global Constraints

- Preserve the existing configuration version and patch API.
- Keep synchronization disabled by default.
- Never provide default endpoints, buckets, account identifiers, or secrets.
- Do not modify or stage the unrelated `macos-icon.icns`.
- Follow the locked QingYu `design.md`: system UI type, neutral palette, 4px/8px spacing, fine dividers, and no card stack.

---

### Task 1: Define the grouping contract

**Files:**
- Modify: `packages/app/src/components/settings/SyncSettings.test.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`

**Interfaces:**
- Consumes: `SettingsSection` headings and the existing `SettingsTranslate` API.
- Produces: the translation keys `settings.sync.section.basic`, `automatic`, `s3Connection`, `webdavConnection`, `advanced`, and `connectionStatus`.

- [ ] Add a React test that renders S3 and asserts the headings in DOM order: Basic settings, Automatic sync, S3 connection, Advanced options, Connection and status.
- [ ] Add a React test that renders WebDAV and asserts WebDAV connection is present while S3 connection and Advanced options are absent.
- [ ] Run `pnpm --filter @markra/app exec vitest run src/components/settings/SyncSettings.test.tsx` and verify RED because the headings do not exist.
- [ ] Add the six typed English and Simplified Chinese translation messages.

### Task 2: Render grouped loaded settings

**Files:**
- Modify: `packages/app/src/components/settings/SyncSettings.tsx`

**Interfaces:**
- Consumes: the six translation keys from Task 1.
- Produces: sibling `SettingsSection` groups without changing any control callback or patch payload.

- [ ] Split the loaded-state JSX into Basic, Automatic sync, provider connection, optional S3 Advanced, and Connection and status sections.
- [ ] Keep loading, absent, malformed, and unsupported branches unchanged.
- [ ] Run the focused React test and verify GREEN.

### Task 3: Define and implement safe creation defaults

**Files:**
- Modify: `apps/desktop/src-tauri/src/sync_config/model.rs`

**Interfaces:**
- Consumes: the existing `Default for SyncConfig` and `Default for S3Config` implementations.
- Produces: disabled S3 configuration with `qingyu`, save sync enabled, five-minute interval, `us-east-1`, 60-second timeout, automatic addressing, and TLS verification.

- [ ] Extend `default_shape_is_flat_disabled_and_versioned` to assert provider S3, save sync true, interval 5, region `us-east-1`, and empty endpoint/bucket/credentials.
- [ ] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml default_shape_is_flat_disabled_and_versioned -- --nocapture` and verify RED against the old WebDAV/disabled-automation defaults.
- [ ] Update `SyncConfig::default()` and `S3Config::default()` with the specified safe defaults.
- [ ] Run the focused Rust test and verify GREEN.

### Task 4: Repository verification

**Files:**
- Verify all modified files.

**Interfaces:**
- Consumes: Tasks 1 through 3.
- Produces: a release-ready working tree with no generated artifacts staged.

- [ ] Run `pnpm --filter @markra/app exec vitest run src/components/settings/SyncSettings.test.tsx`.
- [ ] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`.
- [ ] Run `pnpm test`.
- [ ] Run `pnpm typecheck:test`.
- [ ] Run `pnpm build`.
- [ ] Run `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check` and `git diff --check`.
- [ ] Confirm `macos-icon.icns` remains untracked and unchanged.

## Self-review

The plan covers every grouping and default from the design, introduces no new
schema or component abstraction, preserves all existing loaded configurations,
and keeps user-specific S3 values empty. There are no placeholders or deferred
requirements.
