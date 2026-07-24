# QingYu Mobile Native Acceptance Results

## Run Metadata

- Date: 2026-07-20 (Asia/Shanghai)
- Candidate code commit: `a1fdd6f9643820be3127c39c3a243eef3633db50`
- Branch: `codex/qingyu-mobile-platform`
- Credentials, secrets, authorization headers, private endpoints, private buckets, signing identities, or developer-team identifiers recorded here: **No**
- Evidence root: `[LOCAL_EVIDENCE]/qingyu-mobile/2026-07-20/` (not committed)

## Surfaces and Artifacts

| Surface | Device / OS | Candidate artifact or launch | Overall state |
| --- | --- | --- | --- |
| Android emulator | Android 15 / API 35, arm64 virtual device | `apps/desktop/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk` | Core UI/editor smoke passed; extended matrix remains pending |
| iOS Simulator | iPhone 16 Pro, iOS 18.6 | `apps/desktop/src-tauri/gen/apple/build/arm64-sim/QingYu.app` | Core UI/editor smoke passed; extended matrix remains pending |
| Android device | No device connected | Device-target artifact not produced in this subtask | Not run — no device connected |
| iOS device | No device connected | Device-target artifact not produced in this subtask | Not run — no device connected |
| Desktop native | Pending root-run | `pnpm tauri dev` | Pending root-run |
| Narrow browser | Chromium, 390 x 844 CSS pixels | `pnpm dev` at a phone-sized viewport | WEB01-WEB02 passed; WEB03-WEB04 remain pending |

Artifact existence/build success is recorded separately below and does not count as launch or UI acceptance.

## Automated Gates

| Gate | Result | Evidence path / notes |
| --- | --- | --- |
| Clean tracked tree before Task 10 docs | Pass | `[LOCAL_EVIDENCE]/automated/00-git-status-before.txt`; no tracked or untracked changes were reported |
| `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml` | Pass | Final post-fix run: 453 passed, 21 environment-gated live tests ignored, 0 failed; `[LOCAL_EVIDENCE]/automated/01-cargo-test.txt` |
| `pnpm test` | Pass | Final fresh full run: 178 files and 2,342 tests passed. The first run had one non-reproducing welcome-text timeout; the exact test, the full 1,913-test App package, and the repeated full workspace run all passed without source changes. `[LOCAL_EVIDENCE]/automated/02-pnpm-test.txt` |
| `pnpm typecheck:test` | Pass | All 8 participating workspace projects passed; `[LOCAL_EVIDENCE]/automated/03-typecheck-test.txt` |
| `pnpm build` | Pass | All workspace builds, Web/Desktop Vite builds, and 12 desktop vendor chunks passed; `[LOCAL_EVIDENCE]/automated/04-pnpm-build.txt` |
| Android debug APK builds | Pass | x86_64 build passed during the initial gate; the final post-fix aarch64 build also passed and produced the universal debug APK; `[LOCAL_EVIDENCE]/automated/05-android-build.txt` |
| iOS aarch64 Simulator debug build | Pass | Fresh unsigned Simulator bundle, approximately 101 MiB. The first packaging attempt found an old ignored destination bundle; it was moved recoverably outside the repository and the unchanged command then passed. `[LOCAL_EVIDENCE]/automated/06-ios-build.txt` |
| Live MinIO suite | Pass | 21 passed, 0 failed in 29.42 seconds. Covered upload, download, update, deletion, conflict copies, checkpoints/retry, pagination, invalid credentials, and temporary-file cleanup. Values were environment-injected and are not recorded. |
| Managed-workspace alias regression | Pass | Focused Rust test reproduced the Android app-data alias mismatch before the fix and passed after returning the canonical workspace path; 4 managed-workspace tests passed. |
| Unsupported-feature scan | Pass | Both required prohibited-scope searches returned zero matches; `[LOCAL_EVIDENCE]/automated/07-unsupported-scan.txt` |
| Environment-value secret scan | Pass | Environment-value scan completed without finding an injected value in tracked files; generic credential/private-key patterns found zero matches in the two new documents; `[LOCAL_EVIDENCE]/automated/08-secret-scan.txt` |
| `git diff --check` | Pass | No whitespace errors; generated build paths contain zero tracked files; `[LOCAL_EVIDENCE]/automated/09-diff-check.txt` |

## Case Results

`N/A` means the acceptance document does not assign the case to that surface. Emulator/Simulator UI rows remain `Pending root-run` until the produced artifacts are actually installed and exercised. Physical-device-only rows are explicitly not run.

### Workspace, Documents, and File Operations

| Case | Android emulator | iOS Simulator | Android device | iOS device | Desktop native | Narrow browser | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| W01 | Pass | Pass | Not run — no device connected | Not run — no device connected | N/A | N/A | Clean launch showed one managed workspace and no folder picker/switcher/reset; `[LOCAL_EVIDENCE]/android/W01.png`, `[LOCAL_EVIDENCE]/ios/W01.png` |
| W02 | Pass | Pass | Not run — no device connected | Not run — no device connected | N/A | N/A | Named notes were created, edited, autosaved, and reopened in local-only mode |
| W03 | Pass | Pass | Not run — no device connected | Not run — no device connected | N/A | N/A | Cold relaunch restored the saved last document and exact visible content on both simulators |
| W04 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| W05 | Pass | Pass | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | In-app name dialog created and opened `FixedPersist.md` on Android and `Mobile Acceptance.md` on iOS |
| W06 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| W07 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| W08 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| W09 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| W10 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| W11 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| W12 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| W13 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| W14 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |

### Editor, Compact Navigation, Images, Attachments, and Links

| Case | Android emulator | iOS Simulator | Android device | iOS device | Desktop native | Narrow browser | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E01 | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| E02 Chinese IME | Pending root-run if available | Pending root-run if available | Not run — no device connected | Not run — no device connected | N/A | N/A | Chinese text paste/save/reopen passed on iOS Simulator; candidate-selection composition remains pending |
| E03 selection/copy/paste | Pending root-run if supported | Pending root-run if supported | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| E04 undo/redo | Pass | Pass | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Undo cleared the tested content and redo restored it; persisted bytes followed the final state |
| E05 keyboard | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| E06 1.5-second autosave | Pass | Pass | Not run — no device connected | Not run — no device connected | N/A | N/A | Filesystem bytes were inspected after the debounce and matched the visible editor content |
| E07 lifecycle flush | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| N01 full-screen Files | Pass | Pass | Not run — no device connected | Not run — no device connected | N/A | Pass | Files covered the full Compact viewport; returning preserved editor content. Browser region measured 390 x 844 at x=0/y=0. |
| N02 full-screen Settings | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | Pass | Full-screen Settings and Sync detail were visually verified natively; the complete native category sweep remains pending. Browser visited General, Storage, Sync, Appearance, and Editor in a 390 x 844 full-screen region and returned with editor content intact. |
| N03 overlay navigation | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| I01 picker cancel | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| I02 permission denial | N/A | N/A | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending physical devices |
| I03 all image formats | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| I04 unsupported/mismatch | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| I05 Android `content://` | Pending root-run | N/A | Not run — no device connected | N/A | N/A | N/A | Pending |
| I06 iOS picker URI | N/A | Pending root-run | N/A | Not run — no device connected | N/A | N/A | Pending |
| I07 collision | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| I08 low storage/write failure | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| I09 restart display | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| I10 sync display | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending root-run + MinIO/WebDAV |
| A01 unsupported attachment | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| L01 HTTP/HTTPS | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | Pending root-run | Pending |
| L02 invalid/opener failure | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | Pending root-run | Pending |

### Back, Lifecycle, and Persistence

| Case | Android emulator | iOS Simulator | Android device | iOS device | Desktop native | Narrow browser | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| B01 Files Back | Pending root-run | N/A | Not run — no device connected | N/A | N/A | N/A | Pending |
| B02 Settings/detail Back | Pending root-run | N/A | Not run — no device connected | N/A | N/A | N/A | Back from the Sync detail returned through Compact navigation without exiting; every detail page and rapid-repeat behavior remain pending |
| B03 overlay Back | Pending root-run | N/A | Not run — no device connected | N/A | N/A | N/A | Pending |
| B04 editor-root exit | Pending root-run | N/A | Not run — no device connected | N/A | N/A | N/A | Pending |
| B05 iOS/Web navigation | N/A | Pending root-run | N/A | Not run — no device connected | N/A | Pending root-run | Pending |
| P01 foreground/background | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending |
| P02 force close | Pass | Pass | Not run — no device connected | Not run — no device connected | N/A | N/A | After completed autosave, Android force-stop and iOS terminate/relaunch restored the fixed workspace, last document, and saved content |
| P03 low-memory recreation | N/A | N/A | Not run — no device connected | Not run — no device connected | N/A | N/A | Pending physical devices |
| P04 upgrade persistence | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | N/A | N/A | Android replacement install preserved existing workspace files; a full earlier-build run including last-open state and sync configuration remains pending |

### Sync and Security

The environment-injected live S3-compatible engine suite passed 21/21 cases. It directly covered transfer, conflict, checkpoint, deletion, pagination, invalid-credential, and temporary-cleanup behavior without recording any connection values. The table remains `Pending root-run` where the case also requires installed-app UI operation. WebDAV UI/manual coverage was not run.

| Case | Android emulator | iOS Simulator | Android device | iOS device | Desktop native | Narrow browser | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| S01 S3 configure/success | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending live/UI |
| S02 WebDAV configure/success | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending UI |
| S03 manual bidirectional sync | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Live engine transfer passed; installed-app manual Sync-now UI remains pending |
| S04 sync-after-save on/off | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending live/UI |
| S05 invalid endpoint | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| S06 invalid credentials | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Live engine invalid-credential behavior passed; installed-app reason/action UI remains pending |
| S07 offline | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| S08 DNS | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| S09 timeout | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| S10 TLS | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| S11 unsupported config | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| S12 conflict copy | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Live engine conflict-copy case passed; installed-app UI remains pending |
| S13 checkpoint/retry | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Live engine checkpoint/retry cases passed; installed-app UI remains pending |
| S14 local deletion propagation | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Live engine local-deletion propagation passed; installed-app UI remains pending |
| S15 remote deletion propagation | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Live engine remote-deletion propagation passed; installed-app UI remains pending |
| S16 no-op stability | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Live engine unchanged-sync stability passed; installed-app UI remains pending |
| S17 reason + at most one action | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Pending |
| S18 secret/private endpoint absence | Pending root-run | Pending root-run | Not run — no device connected | Not run — no device connected | Pending root-run | N/A | Repository/evidence scan passed; a complete installed-app UI/log scan remains pending |

### Desktop Native Regression

| Case | Result | Evidence |
| --- | --- | --- |
| D01 menus and shortcuts | Pending root-run | Pending |
| D02 settings window and multiwindow | Pending root-run | Pending |
| D03 selection, associations, open paths, project directory | Pending root-run | Pending |
| D04 attachment and containing folder | Pending root-run | Pending |
| D05 export/Pandoc | Pending root-run | Pending |
| D06 updater/process surfaces | Pending root-run | Pending |
| D07 system fonts | Pending root-run | Pending |
| D08 window restore | Pending root-run | Pending |

### Narrow Browser Compact Regression

| Case | Result | Evidence |
| --- | --- | --- |
| WEB01 Compact layout | Pass | At 390 x 844, Files and Settings each measured x=0/y=0/width=390/height=844; controls and editor toolbar remained usable |
| WEB02 dialog/navigation/state | Pass | Edited text survived Files and all five Settings category round-trips; New file opened one in-app dialog and zero browser dialogs |
| WEB03 link behavior | Pending root-run | Pending Web UI evidence |
| WEB04 capability boundary | Pending root-run | Native-only operations were not exposed during the core smoke, but the full capability-matrix audit remains pending |

## Defects and Blockers

- Resolved during this run: Android returned the managed workspace through `/data/user/0/...` while newly created files were canonicalized through `/data/data/...`; the relative-path guard then cleared last-open state. Commit `a1fdd6f` now returns the canonical managed-workspace root. A focused alias regression test and Android new-file/autosave/force-stop/cold-relaunch flow both passed after the fix.
- Required Android physical-device rows: Not run — no device connected.
- Required iOS physical-device rows: Not run — no device connected.
- Android emulator and iOS Simulator core UI/editor smoke: Passed. Extended image, abnormal-input, lifecycle, link, and complete navigation rows remain pending.
- Narrow browser core Compact UI/editor regression: WEB01-WEB02 passed; WEB03-WEB04 remain pending.
- Live MinIO engine suite: Passed, 21/21. Installed-app sync UI and WebDAV manual coverage remain pending.
- Desktop native regression: Pending root-run.
- Overall native acceptance: **Incomplete** because physical-device-only and extended matrix rows were not run. The requested core UI/editor and live S3-compatible checks passed on the available emulator/simulator surfaces.
