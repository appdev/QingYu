# Task 4 Report: Mixed Theme Catalog Lifecycle

## Status

Implemented mixed root-level legacy CSS and resource-directory catalog discovery, prepared `.css` / `.theme` import and replacement, storage-kind-aware deletion, and `.theme`-only package export. Directly placed author directories remain read-only during refresh, while imported packages are installed from Task 3 validated staging directories under canonical catalog targets.

## RED evidence

After adding the mixed catalog and command-flow tests first, I ran:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::catalog::tests
```

Result: exit 101 at compile time. The failures were the intended missing Task 4 surface: `CatalogPublicationHookPoint`, `import_prepared`, `replace_prepared`, `replace_prepared_with_hook`, package `export`, `import_external_theme`, and `replace_external_theme`. The previous CSS-only catalog had no code path capable of satisfying the new resource-directory lifecycle assertions.

The first implementation run compiled and exercised all new tests, then exited 101 with 10 passed and 9 failed. The failures identified two integration-boundary defects rather than fixture errors:

- internal legacy staging used a `.tmp` suffix but was revalidated through the external dispatcher that intentionally accepts only `.css` / `.theme`;
- successful resource staging rename was followed by `PreparedThemeDirectory` drop cleanup, whose retained directory handle correctly removed the moved directory because catalog publication had not explicitly consumed ownership.

The fixes kept the validators unchanged: internal legacy staging remains an exact owned catalog entry but ends in `.tmp.css`, and successful resource publication consumes/disarms the prepared staging owner; failure paths keep Task 3 retained-handle cleanup armed.

## GREEN evidence

Fresh final verification after implementation, root-cause fixes, refactoring, and formatting:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::
rustfmt --edition 2021 --check apps/desktop/src-tauri/src/themes/catalog.rs apps/desktop/src-tauri/src/themes/mod.rs apps/desktop/src-tauri/src/themes/migration.rs
git diff --check
```

Results:

- complete native theme suite: exit 0; 104 passed, 0 failed;
- focused rustfmt: exit 0;
- diff whitespace check: exit 0.

The focused evidence gathered during implementation was also green:

- catalog lifecycle: 19 passed, 0 failed;
- command flow: 4 passed, 0 failed;
- migration: 2 passed, 0 failed;
- archive regression: 35 passed, 0 failed;
- resource validation regression: 25 passed, 0 failed.

## Files changed

- `apps/desktop/src-tauri/src/themes/catalog.rs`: mixed scanning, exact owned-entry filtering, prepared import staging, storage transitions, fingerprint gates, backup/rollback publication, deletion, package export, and focused lifecycle tests.
- `apps/desktop/src-tauri/src/themes/mod.rs`: `.css` / `.theme` dispatcher, reusable command-flow helpers, conflict result preservation, source reopen on replacement, `.theme` export routing, and focused command tests.
- `apps/desktop/src-tauri/src/themes/migration.rs`: migrated legacy custom CSS now enters the same prepared-import catalog path instead of a byte-only lifecycle method.
- `.superpowers/sdd/task-4-report.md`: RED/GREEN evidence, boundary review, and concerns.

## Requirement self-review

- Mixed scan validates root `.css` files through the Task 3 no-follow external reader and root directories through the Task 2 directory validator.
- Invalid files and invalid directories become independent diagnostics and do not hide unrelated valid themes.
- Duplicate IDs are grouped across both storage kinds; every colliding descriptor is invalidated.
- Only exact numeric `.qingyu-theme-<pid>-<counter>` staging/backup forms with recognized owned suffixes are skipped; arbitrary prefix matches remain visible diagnostics.
- Command conflicts return the candidate, the fresh existing descriptor, and the unchanged `sourcePath` for CSS-to-directory and directory-to-CSS cases. Dropping a conflicted prepared package removes its staging directory.
- New resource installation accepts only `PreparedThemeImport::ResourcePackage`, revalidates its fingerprint immediately before no-replace rename, re-scans after publication, and explicitly consumes staging ownership only after success.
- CSS-to-directory and directory-to-CSS replacement use the descriptor `storage_kind`, not source or installed extensions, with expected-fingerprint checks before moving existing storage.
- Replacement validates the ready candidate before touching the current version, moves the exact old entry to an owned backup, revalidates that backup, publishes with an atomic no-overwrite rename, re-scans the installed descriptor, and restores the old name on every injected pre-publication or post-publication validation failure.
- Successful replacement normally deletes the backup. As in Task 3 archive publication, a true cleanup failure is advisory after a verified new version is already installed; an exact owned backup may remain recoverable and is excluded from catalog scan.
- Delete revalidates the current fingerprint and removes files or directories according to serialized storage kind. Protected `light` and `dark` deletion remains unchanged.
- Protected defaults, legacy CSS, and resource directories all export through the deterministic Task 3 package writer and reimport as resource packages; non-`.theme` targets fail before writing.
- Replacement reopens and revalidates the conflict UI `sourcePath`; no previously read candidate bytes are trusted.
- Repeated refresh scans directly placed author directories without renaming, rewriting, seeding, or otherwise taking ownership of them.
- No activation, UI, mobile, Drake, sync, or catalog-v2 work was introduced.

## Concerns

- Catalog publication uses the shared atomic no-replace primitive: retained-directory `renameat(..., RENAME_NOREPLACE)` on Unix and retained-handle `SetFileInformationByHandle(FileRenameInfo)` with `ReplaceIfExists=false` on Windows. The Windows branch source-compiles for `x86_64-pc-windows-msvc`; runtime execution was not available on the macOS host.
- Post-success backup cleanup is intentionally advisory so the command never reports a false failure after a verified replacement is already active. Normal and injected test paths leave no owned residue.

## Review fix: identity-bound catalog operations

The first report overstated its rollback coverage: the original injected failure test stopped at `AfterBackup`, before the candidate had been published. It therefore proved restoration after backup isolation, but did not prove restoration after publication. The review fix adds distinct `AfterPublication` tests for both legacy files and resource directories; both now publish the candidate first, inject failure, and verify byte-for-byte/tree-for-tree restoration of the old entry with no owned residue.

Strict RED evidence for the review tests:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::catalog::tests
```

The first run exited 101 at compile time because the tests intentionally referenced the not-yet-implemented `delete_with_hook`, `DeleteHookPoint`, `import_prepared_with_hook`, `BeforePublication`, `AfterPublication`, `CatalogRenameHookPoint`, and `rename_catalog_noreplace_with_hook` surfaces. After implementation, the first GREEN attempt compiled and ran 26 tests, with 25 passing and one failing: prepared-directory substitution returned `InvalidManifest` before reaching the retained-directory identity guard. Restricting the post-hook ambient re-read to legacy files made resource publication rely on the prepared directory's retained handle and exact file identities, producing the required `UnsafePath` result.

Fresh final verification:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::catalog::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::archive::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::resources::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::
git diff --check
```

Results:

- catalog lifecycle: 26 passed, 0 failed;
- archive regression: 35 passed, 0 failed;
- resource validation regression: 25 passed, 0 failed;
- complete native theme suite: 111 passed, 0 failed;
- diff whitespace check: exit 0.

The review changes close four catalog race boundaries:

- Delete now capability-renames the exact named entry to an owned quarantine name, revalidates its fingerprint/content there, restores it on mismatch, and only then removes the quarantined entry. A newer file or directory tree substituted after initial validation is preserved at the canonical name.
- Prepared resource publication retains staging/catalog directory handles through no-replace rename, rechecks root and file identities immediately around publication, and keeps cleanup ownership armed until the published tree is independently revalidated.
- Catalog rename retains the opened catalog root, verifies ambient and reopened identities before mutation, performs the rename relative to that retained directory, rechecks the root afterward, and reverses the rename through the retained directory if the postcondition fails.
- Replacement now has independent failure points before publication, after backup, and after publication; the last path isolates the rejected candidate and restores the exact backup before returning the injected error.

The Windows rename branch no longer uses ambient `MoveFileExW` paths. The final no-clobber review below replaces the intermediate `cap_std::fs::Dir::rename` implementation with a retained-handle atomic primitive; the platform-independent root-substitution test continues to exercise the shared identity gate.

## Review fix: Windows atomic no-clobber rename

The intermediate Windows implementation still had an absence-check/rename race because `cap_std::fs::Dir::rename` can use replacement semantics. The final design follows Microsoft's [`FILE_RENAME_INFO`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info) contract: `ReplaceIfExists=false` makes an existing destination fail in the same operation that performs the rename. The source is opened with `DELETE` access and no symlink following, and the relative destination is resolved from the retained destination directory handle as described by [`FILE_RENAME_INFORMATION`](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information). No ambient absolute path is passed to Win32.

Strict RED evidence:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short atomic_noreplace::tests
```

The first run exited 101 at compile time with the intended unresolved `atomic_noreplace::rename_noreplace` import. After implementation, both contract tests pass: an existing target preserves both source and installed bytes, and empty, parent-relative, or multi-component names fail with `InvalidInput` before mutation.

The shared primitive validates that both names are exactly one native path component. Unix retains the existing `rustix::fs::renameat_with(..., RenameFlags::NOREPLACE)` behavior. Windows opens the source through the retained `cap_std::fs::Dir`, uses `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT`, constructs an aligned `FILE_RENAME_INFO` buffer, sets the retained destination directory as `RootDirectory`, sets `ReplaceIfExists=false`, and maps failure with `io::Error::last_os_error`; file and buffer handles are released by RAII. Unsupported platforms still fail closed.

Every relevant caller now goes through this one primitive:

- archive new-target export and prepared resource-directory publication;
- catalog installation, quarantine, rejected-candidate isolation, backup restoration, and rollback;
- the existing remote-sync publication/restore consumers, replacing their duplicate Windows helper without changing their flow.

Windows compile evidence:

```bash
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --message-format short
```

The full project check reaches the existing `ring 0.17.14` C build and then fails before compiling `markra` because the macOS cross environment lacks the Windows C header `assert.h`. To isolate this unchanged toolchain limitation from the reviewed code, a temporary minimal crate with only `cap-std 4.0.2`, `cap-fs-ext 4.0.2`, and `windows-sys 0.61.2` included the actual `src/atomic_noreplace.rs` and ran:

```bash
cargo check --offline --target x86_64-pc-windows-msvc --message-format short
```

That source-level Windows check exited 0; its only warnings were expected dead-code warnings because the probe did not invoke the helper. The temporary probe was removed afterward. Windows runtime behavior remains unexecuted on the macOS host.

Fresh final native verification:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short atomic_noreplace::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short remote_sync::engine::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::catalog::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::archive::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::resources::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --message-format short themes::
rustfmt --edition 2021 --config skip_children=true --check apps/desktop/src-tauri/src/atomic_noreplace.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync/engine.rs apps/desktop/src-tauri/src/themes/archive.rs apps/desktop/src-tauri/src/themes/catalog.rs
git diff --check
```

Results:

- shared no-replace contract: 2 passed, 0 failed;
- remote-sync engine consumers: 23 passed, 0 failed;
- catalog lifecycle: 26 passed, 0 failed;
- archive regression: 35 passed, 0 failed;
- resource validation regression: 25 passed, 0 failed;
- complete native theme suite: 111 passed, 0 failed;
- focused rustfmt and diff checks: exit 0.

Files touched in this review fix:

- `apps/desktop/src-tauri/src/atomic_noreplace.rs`: shared cross-platform primitive and contract tests;
- `apps/desktop/src-tauri/src/lib.rs`: module registration;
- `apps/desktop/src-tauri/src/themes/archive.rs` and `themes/catalog.rs`: mechanical routing of every no-replace operation;
- `apps/desktop/src-tauri/src/remote_sync/engine.rs` and `remote_sync.rs`: mechanical routing and module cleanup;
- `apps/desktop/src-tauri/src/remote_sync/windows_noreplace.rs`: removed after moving the same retained-handle implementation to the shared helper;
- `.superpowers/sdd/task-4-report.md`: corrected Windows claims and added RED/GREEN/compile evidence.
