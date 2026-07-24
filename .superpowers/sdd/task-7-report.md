# Task 7 Report: Every Folder Open Switches the Notebook

## Outcome

Task 7 is complete. QingYu no longer has an external-folder window or temporary folder-mount mode. Every directory entry point now routes to the central notebook switch coordinator, while standalone Markdown files continue to open as unsynchronized external documents without mounting their parent directory.

## Product and Runtime Wiring

- Desktop titlebar, native menu, `Shift+Cmd+O`, compact file-list action, recent-notebook selection, directory drop, queued CLI/second-instance paths, and OS directory-open events all request one notebook switch.
- The coordinator remains the only owner of successful switch ordering: flush the active document, enter the sync barrier, commit the new primary root, wait for it to mount, update notebook recents, release the barrier, and launch sync for the new scope.
- The file picker is now file-only. External file windows preserve relative link and asset resolution from the file path but do not create a notebook/file-tree root.
- The folder picker primitive remains available to notebook switch and restore flows.
- The legacy `recentMarkdownFolders` storage and file-tree recents writer were removed. The drawer reads `recentNotebooks`; only completed coordinator switches write that list, and recent-item clicks route back through the coordinator.
- Native cold start with any directory creates a restore-capable primary window and queues all supplied paths. A mixed folder/file request therefore switches the folder and still forwards the files for external opening.
- macOS `RunEvent::Opened` now uses that same restore-capable reveal route. Once the primary React tree has rendered, queued and live native directories enter the coordinator's one request pump; simultaneous requests still coalesce to the latest path, while a later drain received during an active transaction waits and runs next instead of being dropped. The pump rechecks pending work after its `finally` handoff, closing the settle-callback microtask window without introducing another scheduler.
- External editor and settings windows resolve a folder locally and call a durable desktop command. The command validates the exact directory, queues it in native state, and creates/focuses a restore-capable primary window when none exists, so menu, drop, and recent-directory requests do not depend on a live cross-window listener.
- Native directory delivery is race-safe across startup: Rust queues the directory before spawning the primary window, React installs its event listener before a follow-up durable queue drain, and live callbacks treat that queue as authoritative to avoid duplicate handling.
- An owner unmount closes both already-queued requests and transactions whose asynchronous root commit completes afterward; mount waiting fails closed instead of registering an unreachable waiter.
- macOS now exposes the same explicit “Open Markdown File” / “Switch Notebook Directory...” choice menu as other supported titlebar layouts; the unified label no longer performs a file-only action.
- Recent-notebook reads remain available in every editor window, while only the primary-window owner may save or remove entries. External single-file windows can select a recent notebook through the durable primary-window request but do not expose or execute recent deletion. Primary-window reads and mutations retain the per-runtime serialization and generation guard, and removal compares the exact path without trimming valid trailing whitespace.
- CLI and second-instance argument filtering uses trimmed text only to identify empty values and options; path resolution receives the original argument so valid Unix/macOS directory names with trailing spaces remain exact.
- The deleted surface includes `openMarkdownFolderInNewWindow`, `open_markdown_folder_in_new_window`, `editor_window_url_for_folder`, the combined `openMarkdownPath` picker, and the `external-folder` URL context.
- Native and shared folder actions use localized “Switch Notebook Directory” copy. Log-folder and reveal/containing-folder behavior is unchanged.
- MCP remains application-level and current-notebook-only; no MCP tools or authority behavior changed.

## Verification

- Final `App.test.tsx` run: 273 passed, 0 failed.
- Final notebook-switch coordinator run: 18 passed, 0 failed, including direct and native-event requests arriving during an active switch, the pump-finally microtask handoff, and deferred-commit owner unmount.
- Focused review-remediation runs cover titlebar, notebook coordinator, notebook switch requests, local state, native startup delivery, external-window recent ownership, and exact CLI path handling.
- Desktop file runtime run: 63 tests passed.
- Web file runtime: 21 tests passed.
- Full `pnpm test`: passed; the application package reported 129 files and 2,021 tests, and the desktop package reported 19 files and 211 tests.
- `pnpm typecheck:test`: all participating workspace packages passed.
- `pnpm build`: all workspace builds passed, including desktop vendor-chunk verification.
- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml menu`: 21 passed.
- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows`: 55 passed.
- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp`: 85 passed.
- CLI mixed folder/file cold-start regression: 1 passed.
- Full Rust suite: 607 passed, 22 ignored, 0 failed.
- `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check`: passed.
- Required stale-path audit returned no matches.
- `git diff --check`: passed.

The brief names `src/hooks/useNativeMarkdownDrop.test.tsx`, but this repository has no such file. Native drop behavior is covered by `useMarkdownDocument.test.tsx` and the App routing tests.

The first full Rust run exposed one obsolete source-inspection test that still required the deleted combined picker. The stale assertion was removed, its remaining path-classification test was renamed for runtime open targets, and the complete Rust suite then passed.

## Concerns

- No known Task 7 blocker remains.
- Live S3/WebDAV provider validation remains outside Task 7 and was not started.
