# QingYu S3 Advanced Options Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add configurable per-request timeout, S3 addressing style, and optional TLS certificate verification bypass to QingYu's application-local S3 synchronization settings.

**Architecture:** Extend strict sync-config schema version 2, propagate the three transport fields through `SyncTarget` into one `S3SyncSettings` boundary, and have both S3 HTTP clients derive from the same builder policy. Keep remote file execution serial; addressing style alone changes the target fingerprint.

**Tech Stack:** Rust, serde, reqwest 0.12 with rustls, React, TypeScript, Vitest, Tauri v2.

## Global Constraints

- `requestTimeoutSeconds` is an integer from 5 through 600 and defaults to 60.
- `addressingStyle` is `auto`, `path`, or `virtual-hosted` and defaults to `auto`.
- `tlsVerification` is `verify` or `skip` and defaults to `verify`.
- Schema version 2 does not migrate or read version 1.
- TLS skip applies to connection test, catalog, and ordinary S3 synchronization.
- Do not add `concurrentRequests`; the synchronization engine remains serial.
- Preserve all unrelated working-tree changes and do not modify `apps/desktop/src-tauri/Cargo.toml` or `macos-icon.icns`.

---

### Task 1: Strict version-2 configuration model

**Files:**
- Modify: `apps/desktop/src-tauri/src/sync_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/sync_validation.rs`
- Modify: `apps/desktop/src-tauri/src/sync_config.rs`
- Modify: `packages/app/src/lib/sync-config.ts`
- Test: `apps/desktop/src-tauri/src/sync_config/model.rs`
- Test: `apps/desktop/src-tauri/src/sync_config.rs`
- Test: `packages/app/src/lib/sync-config.test.ts`

**Interfaces:**
- Produces Rust enums `S3AddressingStyle` and `S3TlsVerification`.
- Produces version-2 `S3Config` transport fields and matching TypeScript unions.
- Produces `SyncTarget::S3` carrying all three transport values.

- [ ] **Step 1: Add failing Rust model tests**

Assert that defaults serialize as version 2 with the three required fields, timeout values 4 and 601 produce an issue for `s3.requestTimeoutSeconds`, valid endpoints remain ready, and parsing a version-1 document returns the existing unsupported response.

```rust
assert_eq!(value["version"], 2);
assert_eq!(value["s3"]["requestTimeoutSeconds"], 60);
assert_eq!(value["s3"]["addressingStyle"], "auto");
assert_eq!(value["s3"]["tlsVerification"], "verify");
```

- [ ] **Step 2: Run the Rust tests and confirm RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config::model::tests
```

Expected: failures because version 2 and the new S3 fields do not exist.

- [ ] **Step 3: Implement the Rust model and validation**

Add these types and fields:

```rust
pub(crate) const SYNC_CONFIG_VERSION: u32 = 2;
pub(crate) const MIN_S3_REQUEST_TIMEOUT_SECONDS: u32 = 5;
pub(crate) const MAX_S3_REQUEST_TIMEOUT_SECONDS: u32 = 600;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum S3AddressingStyle { Auto, Path, VirtualHosted }

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum S3TlsVerification { Verify, Skip }

pub(crate) struct S3Config {
    // existing fields
    pub(crate) request_timeout_seconds: u32,
    pub(crate) addressing_style: S3AddressingStyle,
    pub(crate) tls_verification: S3TlsVerification,
}
```

Implement `Default` manually for `S3Config`. Add a range-specific `SyncValueIssue` mapping to the message `Enter a value from 5 through 600.` without clamping invalid persisted values.

Add patch variants named exactly:

```rust
S3RequestTimeoutSeconds(u32)
S3AddressingStyle(S3AddressingStyle)
S3TlsVerification(S3TlsVerification)
```

Carry the fields in both snapshot constructors into `SyncTarget::S3`.

- [ ] **Step 4: Add failing TypeScript patch tests**

Add version-2 fixtures and assert these patches change only their target field:

```ts
{ field: "s3.requestTimeoutSeconds", value: 299 }
{ field: "s3.addressingStyle", value: "virtual-hosted" }
{ field: "s3.tlsVerification", value: "skip" }
```

- [ ] **Step 5: Implement TypeScript version-2 types and patching**

Define:

```ts
export type S3AddressingStyle = "auto" | "path" | "virtual-hosted";
export type S3TlsVerification = "verify" | "skip";
```

Change `QingYuSyncConfig.version` to `2`, add the fields under `s3`, extend `SyncConfigPatch`, and handle all three branches explicitly rather than using the final secret-key fallback for unrelated fields.

- [ ] **Step 6: Run model and TypeScript tests and confirm GREEN**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config::
pnpm exec vitest run src/lib/sync-config.test.ts
```

- [ ] **Step 7: Commit the model boundary**

```bash
git add apps/desktop/src-tauri/src/sync_config/model.rs apps/desktop/src-tauri/src/sync_validation.rs apps/desktop/src-tauri/src/sync_config.rs packages/app/src/lib/sync-config.ts packages/app/src/lib/sync-config.test.ts
git commit -m "feat(sync): add S3 transport settings"
```

### Task 2: Addressing, timeout, and TLS transport policy

**Files:**
- Modify: `apps/desktop/src-tauri/src/s3_http.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`
- Test: `apps/desktop/src-tauri/src/s3_http.rs`
- Test: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`

**Interfaces:**
- Consumes: `S3AddressingStyle`, `S3TlsVerification`, and timeout from Task 1.
- Produces: `S3SyncSettings` as the only constructor boundary for S3 transport policy.

- [ ] **Step 1: Add failing addressing tests**

Cover all of these exact cases:

```text
auto + https://oss-cn-chengdu.aliyuncs.com + notes
  -> https://notes.oss-cn-chengdu.aliyuncs.com/
path + http://127.0.0.1:9000 + notes
  -> http://127.0.0.1:9000/notes
virtual-hosted + https://s3.example.test + notes
  -> https://notes.s3.example.test/
virtual-hosted + https://notes.s3.example.test + notes
  -> unchanged host
path + https://notes.s3.example.test + notes
  -> validation error
```

- [ ] **Step 2: Run S3 URL tests and confirm RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml s3_http::tests
```

- [ ] **Step 3: Implement explicit addressing**

Extend `S3Connection::new` with `addressing_style: S3AddressingStyle`, store it, and switch `s3_bucket_url` by style. Preserve the current heuristic only in `Auto`. Include the normalized style string in `S3Backend::target_fingerprint_source()`.

- [ ] **Step 4: Add failing transport-policy tests**

Test a pure helper that applies policy to a `reqwest::ClientBuilder` input and records this normalized policy:

```rust
struct S3TransportPolicy {
    request_timeout: Duration,
    accept_invalid_certs: bool,
}
```

Assert `60/verify`, `299/skip`, and boundary values `5` and `600`. Also retain the existing redirect rejection test for the connection-test client.

- [ ] **Step 5: Build both clients from one policy**

Extend `S3SyncSettings`:

```rust
pub(crate) request_timeout_seconds: u32,
pub(crate) addressing_style: S3AddressingStyle,
pub(crate) tls_verification: S3TlsVerification,
```

Create both clients with:

```rust
Client::builder()
    .timeout(Duration::from_secs(u64::from(settings.request_timeout_seconds)))
    .danger_accept_invalid_certs(matches!(settings.tls_verification, S3TlsVerification::Skip))
```

Apply `Policy::none()` only to the connection-test client as today.

- [ ] **Step 6: Run S3 backend tests and confirm GREEN**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml s3_http::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::s3_backend::tests
```

- [ ] **Step 7: Commit transport behavior**

```bash
git add apps/desktop/src-tauri/src/s3_http.rs apps/desktop/src-tauri/src/remote_sync/s3_backend.rs
git commit -m "feat(sync): configure S3 transport behavior"
```

### Task 3: Propagate one transport policy through every S3 entry point

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/catalog.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Test: `apps/desktop/src-tauri/src/remote_sync/service.rs`
- Test: `apps/desktop/src-tauri/src/remote_sync/catalog.rs`

**Interfaces:**
- Consumes: the expanded `SyncTarget::S3` and `S3SyncSettings`.
- Produces: identical transport behavior for notes, portable settings, catalog, and connection test.

- [ ] **Step 1: Add failing propagation tests**

Construct a version-2 snapshot with `299`, `virtual-hosted`, and `skip`. Assert the notes backend and settings backend receive the same options and that the catalog backend receives them unchanged.

- [ ] **Step 2: Run focused service/catalog tests and confirm RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::service::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::catalog::tests
```

- [ ] **Step 3: Thread the fields through all constructors**

Update every `SyncTarget::S3` match and every `S3SyncSettings` literal, including fixtures and live-test helpers. Do not substitute defaults at downstream call sites; all runtime values must originate from the saved snapshot.

- [ ] **Step 4: Run the complete Rust sync tests**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config::
```

- [ ] **Step 5: Commit end-to-end propagation**

```bash
git add apps/desktop/src-tauri/src/remote_sync/service.rs apps/desktop/src-tauri/src/remote_sync/catalog.rs apps/desktop/src-tauri/src/remote_sync/live_tests.rs
git commit -m "feat(sync): apply S3 options to every sync path"
```

### Task 4: Desktop and compact/mobile settings controls

**Files:**
- Modify: `packages/app/src/components/settings/SyncSettings.tsx`
- Modify: `packages/app/src/components/settings/SyncSettings.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSyncFormScreen.tsx`
- Modify: `packages/app/src/components/compact/CompactSyncFormScreen.test.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`
- Modify: `packages/shared/src/i18n/index.test.ts`

**Interfaces:**
- Consumes: version-2 TypeScript configuration and patch types.
- Produces: identical controls on desktop and compact/mobile settings surfaces.

- [ ] **Step 1: Add failing desktop and compact UI tests**

Assert labels, current values, and emitted patches for:

```ts
{ field: "s3.requestTimeoutSeconds", value: 299 }
{ field: "s3.addressingStyle", value: "path" }
{ field: "s3.tlsVerification", value: "skip" }
```

Assert the TLS option is rendered as `Skip verification (unsafe)` / `跳过校验（不安全）` and that WebDAV mode does not display these S3-only controls.

- [ ] **Step 2: Run UI tests and confirm RED**

```bash
pnpm exec vitest run src/components/settings/SyncSettings.test.tsx src/components/compact/CompactSyncFormScreen.test.tsx
```

- [ ] **Step 3: Add desktop controls**

Use the existing `SettingsNumberInput` and `SettingsSelect` primitives:

```tsx
<SettingsNumberInput min={5} max={600} unit={translate("settings.sync.seconds")} />
<SettingsSelect options={addressingOptions} />
<SettingsSelect options={tlsVerificationOptions} />
```

Keep all patches immediate through the existing `queuePatch` path.

- [ ] **Step 4: Add compact/mobile controls**

Use native number and select inputs with the existing `inputClass`; keep a minimum 44-pixel touch target and the same option values as desktop.

- [ ] **Step 5: Add localized copy**

Add keys for the three labels, short descriptions, seconds unit, three addressing options, and two TLS options. Localize Simplified Chinese and Traditional Chinese rather than falling back to English for the unsafe choice.

- [ ] **Step 6: Run UI and i18n tests and confirm GREEN**

```bash
pnpm exec vitest run src/components/settings/SyncSettings.test.tsx src/components/compact/CompactSyncFormScreen.test.tsx
pnpm --filter @markra/shared test
```

- [ ] **Step 7: Commit settings surfaces**

```bash
git add packages/app/src/components/settings/SyncSettings.tsx packages/app/src/components/settings/SyncSettings.test.tsx packages/app/src/components/compact/CompactSyncFormScreen.tsx packages/app/src/components/compact/CompactSyncFormScreen.test.tsx packages/app/src/App.test.tsx packages/shared/src/i18n
git commit -m "feat(sync): expose S3 advanced options"
```

### Task 5: Integrated verification

**Files:**
- Verify only; do not modify unrelated files to silence failures.

- [ ] **Step 1: Run repository frontend gates**

```bash
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: all commands exit 0.

- [ ] **Step 2: Run the Rust gate**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: new sync tests pass. If the two existing dependency-boundary tests still fail because the unrelated working-tree `macos-private-api` feature remains in `Cargo.toml`, report that fact and do not alter the user-owned change.

- [ ] **Step 3: Run live MinIO coverage when configured**

```bash
pnpm test:s3-sync:live
```

Expected: pass against the configured test service; otherwise report that the required local environment was unavailable without committing credentials.

- [ ] **Step 4: Check repository hygiene**

```bash
git diff --check
git status --short
```

Confirm no credentials, generated build output, `node_modules`, or Rust `target` content entered the change set.
