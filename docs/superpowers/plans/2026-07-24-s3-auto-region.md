# S3 Automatic Region Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep the saved S3 region blank by default and resolve blank input to `auto` at the S3 signing boundary.

**Architecture:** Preserve the version-2 string field and patch API. Make region optional in the configuration model, then normalize only the effective runtime region inside `S3Connection`, which is shared by connection tests, catalog reads, synchronization, and MCP-triggered synchronization.

**Tech Stack:** Rust, serde, reqwest, AWS Signature Version 4, React, TypeScript, Vitest, React Testing Library, Tauri v2.

## Global Constraints

- Work directly on the current `main` branch as explicitly approved by the user.
- Use one agent only; do not dispatch subagents.
- Persist an empty string when the S3 region field is blank.
- Resolve empty or whitespace-only region input to the exact lowercase string `auto` only at `S3Connection` construction.
- Preserve every non-empty explicit region after trimming.
- Do not migrate or rewrite existing configurations and do not change schema version 2.
- Do not add dependencies or modify unrelated files.

---

### Task 1: Configuration default and readiness

**Files:**
- Modify and test: `apps/desktop/src-tauri/src/sync_config/model.rs`

**Interfaces:**
- Consumes: `S3Config::default`, `SyncConfig::normalize`, `SyncConfig::issues`, and `SyncConfig::readiness`.
- Produces: a serialized empty region default and ready S3 configurations whose only omitted former required field is region.

- [ ] **Step 1: Write failing model tests**

Change the default-shape assertion and add an observable readiness test:

```rust
assert_eq!(value["s3"]["region"], "");

#[test]
fn empty_s3_region_uses_runtime_default_and_remains_ready() {
    let mut config = complete_s3_config();
    config.s3.region = "  ".into();

    config.normalize();

    assert_eq!(config.s3.region, "");
    assert!(config
        .issues()
        .iter()
        .all(|issue| issue.field != "s3.region"));
    assert_eq!(config.readiness(), SyncConfigReadiness::Ready);
}
```

- [ ] **Step 2: Run the model tests and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config::model::tests -- --nocapture
```

Expected: the default assertion sees `us-east-1`, and the readiness test sees a required `s3.region` issue.

- [ ] **Step 3: Implement the minimal configuration change**

Set the S3 default to an empty string:

```rust
region: String::new(),
```

Remove only `s3.region` from the required-field loop while preserving bucket and access-key validation:

```rust
for (field, value) in [
    ("s3.bucket", self.s3.bucket.as_str()),
    ("s3.accessKeyId", self.s3.access_key_id.as_str()),
] {
```

- [ ] **Step 4: Run the model tests and verify GREEN**

Run the same focused command and require every model test to pass.

### Task 2: Runtime signing fallback

**Files:**
- Modify and test: `apps/desktop/src-tauri/src/s3_http.rs`

**Interfaces:**
- Consumes: the saved `region: &str` passed to `S3Connection::new_with_addressing_style`.
- Produces: `S3Connection.region` containing either `auto` or a trimmed explicit region, used by existing credential-scope and signing-key code.

- [ ] **Step 1: Write the failing S3 connection test**

Add a real signing-boundary test using a fixed timestamp:

```rust
#[test]
fn blank_region_resolves_to_auto_for_request_signing() {
    let connection = S3Connection::new(
        "https://s3.example.test",
        "  ",
        "notes",
        "key",
        "secret",
    )
    .expect("blank region should use auto");
    let url = s3_bucket_url(&connection).unwrap();
    let headers = signed_s3_headers(
        &Method::GET,
        &url,
        S3Payload::Empty,
        None,
        &connection,
        OffsetDateTime::from_unix_timestamp(1_784_181_600).unwrap(),
    )
    .unwrap();

    assert_eq!(connection.region, "auto");
    assert!(headers
        .get(reqwest::header::AUTHORIZATION)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("/auto/s3/aws4_request"));
}
```

- [ ] **Step 2: Run the S3 HTTP tests and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml s3_http::tests -- --nocapture
```

Expected: construction fails with `S3 region is required`.

- [ ] **Step 3: Implement the effective-region normalization**

Replace required-region validation with a local fallback at connection construction:

```rust
let region = region.trim();
let region = if region.is_empty() { "auto" } else { region }.to_string();
```

Leave `required_trimmed` in place for the access key and retain the existing signing implementation.

- [ ] **Step 4: Run the S3 HTTP tests and verify GREEN**

Run the same focused command and require all explicit-region and blank-region tests to pass.

### Task 3: Settings guidance on both form surfaces

**Files:**
- Modify and test: `packages/app/src/components/settings/SyncSettings.tsx`
- Modify and test: `packages/app/src/components/settings/SyncSettings.test.tsx`
- Modify and test: `packages/app/src/components/compact/CompactSyncFormScreen.tsx`
- Modify and test: `packages/app/src/components/compact/CompactSyncFormScreen.test.tsx`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`

**Interfaces:**
- Consumes: existing `SettingsTextInput.placeholder`, native input `placeholder`, and current translation keys.
- Produces: visible `auto` placeholder guidance without writing `auto` into form state.

- [ ] **Step 1: Write failing UI tests**

Render each form with provider `s3` and an empty saved region, then assert the real input remains empty and exposes the hint:

```ts
const region = screen.getByRole("textbox", { name: "S3 region" });
expect(region).toHaveValue("");
expect(region).toHaveAttribute("placeholder", "auto");
```

- [ ] **Step 2: Run the focused UI tests and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/SyncSettings.test.tsx src/components/compact/CompactSyncFormScreen.test.tsx
```

Expected: both region inputs lack the `auto` placeholder.

- [ ] **Step 3: Add placeholder and accurate copy**

Pass `placeholder="auto"` to both region inputs. Update the existing English and Simplified Chinese messages to:

```text
Complete the application S3 endpoint, bucket, credentials, and remote root first.
Region used to sign S3 requests. Leave blank to use auto.
请先补全应用同步配置中的 S3 端点、存储桶、密钥和远端根目录。
用于签署 S3 请求的区域；留空时使用 auto。
```

- [ ] **Step 4: Run focused UI and shared i18n tests and verify GREEN**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/SyncSettings.test.tsx src/components/compact/CompactSyncFormScreen.test.tsx
pnpm --filter @markra/shared test
```

### Task 4: Integrated verification and delivery

**Files:**
- Verify all modified files; do not edit unrelated failures.

**Interfaces:**
- Consumes: Tasks 1 through 3.
- Produces: a verified `main` commit implementing the approved behavior.

- [ ] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`.
- [ ] Run `pnpm test`.
- [ ] Run `pnpm typecheck:test`.
- [ ] Run `pnpm build`.
- [ ] Run `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check`.
- [ ] Run `git diff --check` and inspect `git diff --stat`.
- [ ] If every required `MARKRA_TEST_S3_*` variable is configured, run `pnpm test:s3-sync:live`; otherwise record that the environment-gated live test was unavailable.
- [ ] Review the final diff for scope and commit the implementation with message `fix(sync): use auto for blank S3 region`.

## Self-review

The plan covers the persisted default, readiness semantics, the shared runtime
signing boundary, both settings forms, stale incomplete-configuration copy,
and all repository verification gates. The effective region has one source of
truth, schema version 2 remains unchanged, and no migration or compatibility
branch is introduced.
