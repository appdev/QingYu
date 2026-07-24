# QingYu Mobile Native Acceptance

This checklist is the executable release gate for the Compact Android and iOS applications. A successful frontend build or an artifact that merely exists is not native acceptance. Run every applicable row in the installed application and record the outcome in `docs/testing/qingyu-mobile-native-results.md`.

Never record a credential, secret, authorization header, signed URL, private endpoint, private bucket, signing identity, or developer-team identifier. Evidence paths in the results document must be repository-relative or use a neutral placeholder such as `[LOCAL_EVIDENCE]/android/W01.png`.

## Result Vocabulary

- `Pass`: the row was exercised on the stated surface and matched every expectation.
- `Fail`: the row was exercised and any expectation was not met. Capture sanitized evidence and file a defect.
- `Pending root-run`: the row has not yet been exercised by the native acceptance operator.
- `Not run — no device connected`: required physical-device coverage could not be executed.
- `N/A`: the row does not apply to that surface. Do not use `N/A` to hide an unsupported native behavior.

## Surfaces and Prerequisites

| Surface | Prerequisite | Launch/install command or action |
| --- | --- | --- |
| Android emulator | API level and emulator image recorded; clean install plus upgrade install available | Build with the command below, install the APK with the Android tooling, then launch QingYu |
| iOS Simulator | Simulator model and iOS version recorded; clean install plus upgrade install available | Build with the command below, boot a Simulator, install the `.app`, then launch QingYu |
| Android device | One physical device with gesture and hardware/system Back available | Install a device-target build without recording signing material |
| iOS device | One physical device with gallery and LAN permissions available | Install a device-target build without recording signing material |
| Desktop native | Supported desktop host with Pandoc available for the export row | `pnpm tauri dev` |
| Narrow browser | Phone-sized viewport; this is Web UI evidence only | `pnpm dev`, then use a phone-sized viewport |

Before each clean-launch run, clear only the test installation's local data. Do not add a QingYu reset command and do not delete any remote data outside the test's isolated prefix. For upgrade persistence, install the earlier test build, create content, and install the candidate build over it without clearing app data.

## Commands

Run from the repository root. Build and test output must be sanitized before it is attached as evidence.

```bash
git status --short
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
pnpm tauri android build --debug --apk --target x86_64 --ci
pnpm tauri ios build --debug --target aarch64-sim --ci --no-sign
```

Live S3-compatible coverage is environment-injected and must use a disposable isolated prefix:

```bash
pnpm test:s3-sync:live
```

Do not paste the environment values or the invoking shell history into evidence. WebDAV manual coverage must likewise use a disposable isolated path and locally injected configuration.

## Evidence and Failure Capture

For every failed row, record the case ID, surface, date/time, commit, device/OS, exact sanitized reproduction steps, expected result, observed result, and a relative evidence path. Capture the last visible app state and sanitized diagnostic/log excerpt. For storage or sync cases, also capture a redacted local/remote file listing and content hash when relevant. For lifecycle failures, capture whether the last save completed before the transition.

A screenshot, log, diagnostic, or video containing a credential, authorization header, signed query, credential-bearing URL, or private endpoint is invalid evidence and must be deleted rather than redacted in-place. A failed required row blocks release until a focused regression test and rerun pass.

## A. Workspace, Documents, and File Operations

| ID | Applies to | Procedure and expected UI/behavior | Failure evidence if unmet |
| --- | --- | --- | --- |
| W01 | Android/iOS native | Clear app data and launch. Create notebook A from the welcome surface, then create/switch to B and back to A. Each is exactly one validated child below app-data `workspaces/`; no filesystem folder picker or path field appears. | Sanitized launch log and `workspaces/` tree |
| W02 | Android/iOS native | Decline/skip sync setup. Create, edit, save, close, and reopen a note. Local-only editing works and no remote request or sync configuration is required. | Screen recording plus sanitized network/log excerpt |
| W03 | Android/iOS native | Open document A, fully close and relaunch. Document A restores in the editor with the saved content. | Before/after screenshots and content hash |
| W04 | Android/iOS native | Make the stored last-document path unavailable, then relaunch. The editor shows the welcome empty state and remains usable; it does not crash or show a stale document. | Sanitized state/log and welcome screenshot |
| W05 | Android/iOS native, desktop | Create a named file through the in-app name dialog. The file opens in the editor and current naming semantics are preserved. | Dialog/action video and file tree |
| W06 | Android/iOS native, desktop | Create a folder and create a file inside it. Both appear once in the correct tree location. | Tree before/after |
| W07 | Android/iOS native, desktop | Rename a file and a folder. Open paths and visible tree update without losing the active document. | Tree and active editor path |
| W08 | Android/iOS native, desktop | Move a file and a non-empty folder to valid destinations. Content and descendants are preserved and shown once. | Before/after tree and hashes |
| W09 | Android/iOS native, desktop | Delete a file and a folder through the existing confirmation flow. They disappear locally; unrelated content remains. | Confirmation and before/after tree |
| W10 | Android/iOS native, desktop | Search for exact, partial, nested, non-ASCII, and missing text. Results open the expected document and missing text yields an empty result state. | Search result screenshots |
| W11 | Android/iOS native, desktop | Edit and save multiple versions, then inspect history and restore a prior version using existing behavior. The restored bytes match the selected revision. | History list and content hashes |
| W12 | Android/iOS native, desktop | Submit empty, whitespace-only, reserved/invalid, traversal-like, and otherwise rejected names. The dialog remains open with a localized safe validation message and performs no mutation. | Dialog screenshot and unchanged tree |
| W13 | Android/iOS native, desktop | Create or rename to an existing name. The dialog remains open, shows the localized duplicate-name reason, and preserves both existing entries. | Dialog and unchanged tree |
| W14 | Android/iOS native, desktop | Force a write/create/rename/move failure. Show a safe operation failure, keep recoverable UI state, and do not leave a partial file or close the name dialog as success. | Sanitized failure, staging/final tree |

## B. Editor, Input, Autosave, and Compact Navigation

| ID | Applies to | Procedure and expected UI/behavior | Failure evidence if unmet |
| --- | --- | --- | --- |
| E01 | Android/iOS native, desktop | Type headings, paragraphs, lists, links, code, emphasis, and a table in WYSIWYG mode. Content and formatting remain correct after reopen. | Before/after Markdown and screenshots |
| E02 | Android/iOS device; emulator/simulator where IME is available | Compose Chinese text with a system IME, including candidate selection and editing inside existing text. Composition is not committed early, duplicated, or reordered. | Video showing composition and final bytes |
| E03 | Android/iOS device; simulator where supported | Use native long-press/drag selection, copy, cut, and paste between QingYu and another app. System handles/menus work and content is exact. | Video and copied text hash |
| E04 | Android/iOS native, desktop | Perform several edits, undo to the initial content, then redo. Selection and content remain coherent. | Screen recording and final bytes |
| E05 | Android/iOS native | Focus and blur the editor repeatedly, rotate/resize where supported, and use toolbar actions while the keyboard is visible. The keyboard does not cover the active caret or strand navigation. | Video including keyboard/caret |
| E06 | Android/iOS native | Make a local edit and wait at least 1.5 seconds without an explicit save. Close/reopen and require the edit to persist; no sync is implied by this autosave. | Timestamped video and reopened content |
| E07 | Android/iOS native | Edit without waiting 1.5 seconds, background or hide the page, then return/relaunch. The visibility/page-hide flush preserves local content. | Lifecycle timestamps and reopened bytes |
| N01 | Android/iOS native, narrow browser | From an edited document, open Files. Files occupies the full Compact viewport rather than a half drawer; return to the editor with selection/content preserved. | Phone-sized screenshot/video |
| N02 | Android/iOS native, narrow browser | From an edited document, open Settings and visit General, Storage, Sync, Appearance, and Editor. Settings is full-screen; returning preserves the editor and does not expose desktop categories. | Category screenshots and editor before/after |
| N03 | Android/iOS native | While a name dialog or other Compact overlay is open, navigate away/back. Pending failures remain visible as designed and no unexpected mutation occurs. | Overlay navigation video and tree |

## C. Image Import and Attachment/Link Boundaries

Use fresh uniquely named fixtures. After each successful import, record the resulting relative `assets/` path and hash. Cancellation or failure must not modify Markdown and must not leave a partial or staging file.

| ID | Applies to | Procedure and expected UI/behavior | Failure evidence if unmet |
| --- | --- | --- | --- |
| I01 | Android/iOS native | Open the system image picker and cancel. No error toast, Markdown change, asset, or partial file is created. | Before/after Markdown and assets tree |
| I02 | Android/iOS device | Deny gallery/file permission, then retry after changing permission. The denial is actionable and safe; the editor remains unchanged; a later authorized retry works. | Permission UI and unchanged tree |
| I03 | Android/iOS native | Import valid PNG, JPEG, GIF, WebP, BMP, AVIF, and UTF-8 SVG fixtures. Each is signature-validated, collision-safe, inserted only after save, and displayed after reopen. | Fixture/result hashes and screenshots |
| I04 | Android/iOS native | Select empty, PDF, text, malformed, and extension/signature-mismatch data. Unsupported bytes are rejected; authoritative signature determines a valid mismatched image's extension; no partial Markdown insertion occurs. | Sanitized error and assets tree |
| I05 | Android emulator/device | Select an image delivered as an Android `content://` URI. It is read directly through the scoped filesystem adapter and persists under the workspace. | Sanitized URI scheme only, result hash |
| I06 | iOS Simulator/device | Select an image delivered by the iOS picker URI. It persists under the workspace without converting the URI into a Rust local path. | Sanitized URI scheme only, result hash |
| I07 | Android/iOS native | Import two images that resolve to the same safe name. Both complete files persist with collision-safe names and both Markdown nodes are inserted once. | Assets tree and hashes |
| I08 | Android/iOS native | Force low-storage or asset write failure during one- and multi-image import. Show one safe error; insert no partial batch; leave no staged or truncated final file. | Free-space/state note, Markdown/tree diff |
| I09 | Android/iOS native | Relaunch after importing every supported format. All Markdown references resolve and display from the managed workspace. | Relaunch screenshots |
| I10 | Android/iOS native + configured sync | Sync a document and its imported images, then receive/open them on the paired test state. Images transfer through the normal workspace sync and display. | Sanitized remote key list and hashes |
| A01 | Android/iOS native | Activate a visible synced non-image attachment from Files and from a Markdown link. Exactly one unsupported message appears per action and no native attachment-open call runs. | UI video plus sanitized command trace |
| L01 | Android/iOS native, desktop, narrow browser | Open valid HTTP and HTTPS links. Native uses the official opener; browser development opens a new safe tab; the editor remains intact. | Destination host only and editor screenshot |
| L02 | Android/iOS native, desktop, narrow browser | Activate file, JavaScript, mail, telephone, custom-scheme, malformed, credential-bearing, and opener-failure cases. They are rejected or show one safe failure; the current editor/page remains active. | Sanitized error and unchanged page |

## D. System Back, Lifecycle, and Persistence

| ID | Applies to | Procedure and expected UI/behavior | Failure evidence if unmet |
| --- | --- | --- | --- |
| B01 | Android emulator/device | Press hardware/system or gesture Back from Files. It returns to the editor and preserves content instead of exiting. | Video and lifecycle log |
| B02 | Android emulator/device | Press Back from Settings home and every detail page, including sync status/form. Each request pops one Compact level; repeated rapid requests do not double-pop. | Video covering every page |
| B03 | Android emulator/device | Press Back with a name dialog/overlay open. The overlay closes or its guard consumes Back according to current behavior; no mutation or app exit occurs. | Video and unchanged tree |
| B04 | Android emulator/device | Press Back at the editor root with no overlay. The request is acknowledged unconsumed and the app exits once; relaunch restores saved state. | Lifecycle log and relaunch screenshot |
| B05 | iOS Simulator/device, narrow browser | Exercise every Compact page using visible navigation and system edge/navigation behavior available on the surface. Each action pops one level; editor root remains stable. | Video covering every page |
| P01 | Android/iOS native | Move foreground to background and return from editor, Files, Settings, and an open picker/dialog. Saved editor state survives; foreground refresh preserves the active document and triggers only existing allowed sync behavior. | Lifecycle log and page screenshots |
| P02 | Android/iOS native | Force close after a completed autosave, then relaunch. The selected notebook, last document, and saved content persist. | Timestamps and reopened bytes |
| P03 | Android/iOS device | Trigger low-memory process recreation while content is saved. Relaunch/restore does not duplicate a managed notebook, lose saved content, or select a stale notebook/path. | OS lifecycle evidence and workspace tree |
| P04 | Android/iOS native | Install the candidate build over an earlier build containing local notes and sync configuration. Workspace, documents, last-open state, and configuration persist without migration prompts. | Version/build IDs and before/after hashes |

## E. Sync Parity and Error Handling

Use one disposable remote target per provider and one isolated prefix/path per case group. Synchronization always uses the selected managed notebook and the one application-wide provider configuration. Switching A to B changes only the named child below remote `notes/`; it must not clone provider settings or synchronize every remote notebook. Do not modify the sync algorithm, manifest, conflict naming, checkpoint behavior, deletion propagation, or save-trigger semantics for mobile.

| ID | Applies to | Procedure and expected UI/behavior | Failure evidence if unmet |
| --- | --- | --- | --- |
| S01 | Android/iOS native, desktop | Configure S3-compatible sync with locally injected values and pass the read-only connection check. Configuration remains editable without reset. | Safe status and remote snapshot |
| S02 | Android/iOS native, desktop | Configure WebDAV and pass the read-only connection check. Configuration remains editable without reset. | Safe status and remote snapshot |
| S03 | Android/iOS native, desktop | Run manual Sync now after local and remote creates/updates. Exact bytes transfer both directions and a successful run refreshes the mobile file tree once. | Local/remote hashes and status counts |
| S04 | Android/iOS native, desktop | Enable sync-after-save, save a unique edit, and require transfer. Disable it, save another edit, and require no transfer until manual sync. | Timestamps, hashes, disabled control |
| S05 | Android/iOS native, desktop | Enter an invalid/malformed endpoint. Show a clear localized reason and at most one action opening Sync settings; local content remains usable. | Error panel and button count |
| S06 | Android/iOS native, desktop | Use invalid credentials. Show a safe authorization reason and at most one Sync-settings action; no credential is rendered or logged. | Sanitized UI/log and button count |
| S07 | Android/iOS native, desktop | Go offline during connection test and sync. Show an offline/network reason, keep local edits, and allow configuration/retry. | Network state and sanitized status |
| S08 | Android/iOS native, desktop | Use an unresolvable host. Show a DNS/host-resolution reason and at most one settings action. | Sanitized failure class only |
| S09 | Android/iOS native, desktop | Force a connection/request timeout. Show a timeout reason and at most one settings action; no indefinite busy state remains. | Timing and status screenshot |
| S10 | Android/iOS native, desktop | Use a TLS-invalid test target. Show a certificate/TLS reason and at most one settings action; do not offer an insecure bypass. | Sanitized TLS category |
| S11 | Android/iOS native, desktop | Load a provider/configuration version unsupported by this build. Show the explicit unsupported reason and one action opening Sync settings; no reset is required. | Safe error and form navigation |
| S12 | Android/iOS native, desktop | Change the same file on both sides and sync. Preserve both versions with the established conflict-copy naming and report one conflict. | Hashes, names, summary count |
| S13 | Android/iOS native, desktop | Interrupt after a successful action checkpoint, then retry. Completed actions are not replayed and the final manifest/checkpoint is consistent. | Sanitized action trace and hashes |
| S14 | Android/iOS native, desktop | Establish a sync baseline, delete locally, and sync. Remote deletion propagates; changed remote survivors remain protected according to desktop behavior. | Baseline and remote listing |
| S15 | Android/iOS native, desktop | Establish a baseline, delete remotely, and sync. Local deletion propagates; changed local survivors remain protected according to desktop behavior. | Baseline and local tree |
| S16 | Android/iOS native, desktop | Repeat a successful unchanged sync. It is stable, creates no repeated conflict, and leaves bytes/identities unchanged. | Before/after hashes and conflict count |
| S17 | Android/iOS native, desktop | For every failure above, count content actions. There is a clear reason and no more than one action; when present it opens the Sync form. No reset/cloud-delete action appears. | Composite screenshots and action count |
| S18 | Android/iOS native, desktop | Search UI, logs, diagnostics, screenshots, and captured evidence for credential, secret, authorization, signed-query, credential-bearing URL, and private-endpoint values. Require zero matches. | Sanitized scan command and zero-match result |

## F. Desktop Native Regression

| ID | Procedure and expected behavior | Failure evidence if unmet |
| --- | --- | --- |
| D01 | Launch with `pnpm tauri dev`; existing native application menus and shortcuts work. | Menu/shortcut video and sanitized log |
| D02 | Open the independent Settings window, close/reopen it, and verify multiwindow/editor state behavior. Compact mobile remains in-app only. | Window video |
| D03 | Use desktop standalone-file selection, notebook-directory switching, file association/open path, and standalone-file new-window behavior. Confirm no temporary external-folder session remains. | Selected paths replaced by neutral labels in report |
| D04 | Open a local non-image attachment and its containing folder. Desktop behavior remains available. | Sanitized command trace |
| D05 | Export through the existing export/Pandoc flows and verify output bytes. | Output hash and safe error if unavailable |
| D06 | Verify the updater surface and process/restart entry points remain present and do not appear in mobile. | Surface screenshots |
| D07 | Enumerate/select system fonts and verify existing rendering. Mobile exposes only its supported font controls. | Font UI screenshots |
| D08 | Move/resize windows, relaunch, and verify window-state restore. | Before/after screenshots |

## G. Narrow Browser Compact Regression

These are Web UI checks, never mobile-native evidence.

| ID | Procedure and expected behavior | Failure evidence if unmet |
| --- | --- | --- |
| WEB01 | At a phone-sized viewport, verify full-screen editor, Files, and Settings layout plus safe-area/44px controls. | Viewport dimensions and screenshots |
| WEB02 | Verify create/rename dialogs, Compact navigation, and editor-state preservation without `window.prompt`. | Video and browser console |
| WEB03 | Verify browser-supported HTTP/HTTPS opening and invalid-link rejection. Do not infer native opener behavior. | Browser tab/page evidence |
| WEB04 | Confirm the Web capability matrix does not expose native sync, image picker, native Back, desktop menu/window, or native-only settings surfaces. | DOM screenshot and console trace |

## Final Audit

Run after all acceptance evidence has been recorded:

```bash
for mobile_secret_value in "$MARKRA_TEST_S3_ENDPOINT" "$MARKRA_TEST_S3_ACCESS_KEY_ID" "$MARKRA_TEST_S3_SECRET_ACCESS_KEY"; do
  if test -n "$mobile_secret_value" && git grep -F -q -- "$mobile_secret_value"; then
    exit 1
  fi
done
rg -n "networkProxy|network_proxy|window\.prompt" packages apps --glob '!**/target/**' --glob '!**/node_modules/**'
rg -n "arboard|tauri_plugin_window_state|tauri_plugin_updater|tauri_plugin_process|fontdb|create_application_menu|open_settings_window|open_containing_folder|open_markdown_attachment" apps/desktop/src-tauri/src/mobile_runtime.rs
git diff --check
```

Expected: the environment-value loop succeeds, both searches have no matches in the prohibited scope, and `git diff --check` succeeds. Finally inspect `git status --short`; no generated output, local evidence, secrets, or unrelated files may be committed.
