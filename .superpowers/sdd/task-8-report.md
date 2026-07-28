# Task 8 Report: Tauri UI, Repository Bootstrap, and Relocation

## Outcome

Task 8 completes the application-level S3 bootstrap and controlled repository
relocation baseline. A first manual S3 sync from an unbound keyed notebook now
creates a remote Dejavu repository, persists the binding, and enqueues the
background job. Selecting the same remote repository for another local notes
root performs a serialized relocation: the old local repository cache is
invalidated transactionally, the binding is published to the new root, and an
interrupted process recovers according to the persisted binding.

The real Tauri application was exercised against an isolated public S3 test
repository. Restore and sync ran in the background while ordinary document
creation, editing, saving, and deletion remained available. The Task 8 QA
repository and every exact Task 8 prefix were removed and independently
verified after the run.

Conflict interaction is deliberately not changed by this task. Real UI testing
found that an unresolved conflict created after the main hook initially loaded
without a binding is visible in Settings but does not reach the main conflict
dialog. A temporary late-binding hook correction proved the diagnosis, but it
was reverted before commit because the approved SiYuan-style conflict design
will replace that interaction. No temporary hook code or test is part of the
Task 8 result.

## Product Behavior

- Manual S3 dispatch accepts a complete keyed local state without an existing
  repository binding, assigns a canonical repository UUID, runs portable
  settings synchronization, and hands the request to the installed Dejavu
  owner.
- The Dejavu owner creates remote metadata before binding, uses the notebook
  directory name as the remote display name, and deletes the newly created
  remote repository when binding fails.
- Automatic, startup, interval, and exit triggers still require an existing
  enabled exact binding. A disabled binding or a notes root owned by another
  repository remains rejected.
- Rebinding the same repository to a different local root waits for its active
  repository lane, validates the remote metadata and local key, journals the
  relocation, moves the old cache aside, publishes the new binding atomically,
  and removes the obsolete cache only after success.
- A failed binding write rolls the old cache and binding back. On restart, the
  relocation journal restores the old cache when the old root is authoritative
  or finishes deleting it when the new root is authoritative.
- History, temporary repository data, and the user's notes are preserved while
  only the repository cache is reset.

## Source-Boundary Corrections

- The application live restore test now reuses its local fake working-tree
  coordinator instead of importing the core no-op coordinator. This keeps the
  production path-guard source boundary exact without weakening the guard.
- Panic cleanup in the live S3 helper still executes cleanup and resumes the
  original panic, but no longer prints from a credential-bearing live test
  source. This restores the zero-print mobile/source contract.

## Real Tauri Evidence

The application was built with an isolated QA identifier and launched as a real
macOS Tauri app. Credentials were entered through the product settings surface
and remained masked.

- Connection test succeeded and the first manual sync created and bound a new
  remote repository.
- A background sync click returned in 599 ms. While the application still
  exposed the active-sync state, opening the new-document menu took 134 ms,
  creating a file took 94 ms, and editing plus saving took 138 ms.
- A document created during synchronization was uploaded successfully.
- A document deletion performed during synchronization completed, and a later
  restore into a third empty local root confirmed the deleted file stayed
  absent while the retained files downloaded correctly.
- Relaunch restored the selected local root and binding, then resumed automatic
  synchronization.
- Selecting the same remote repository for a second local root completed the
  controlled relocation. The before/after repository markers matched and the
  stale local cache was not reused.
- A deliberately invalid connection failed visibly; restoring the valid
  configuration made connection testing and synchronization retry succeed.
- Divergent edits to the same path produced exactly one unresolved conflict.
  Settings displayed the conflict and both local and remote versions remained
  available. No resolution action was selected because conflict behavior is
  owned by the follow-up SiYuan-style design.
- The application exited normally after testing.

## Cleanup and Security

- The generated Task 8 repository was deleted through the repository catalog,
  and a following catalog read returned `NotFound`.
- The exact Task 8 protected-settings prefix was cleaned, then the independent
  read-only isolated-prefix verifier returned empty.
- The final local live run independently verified its generated prefix empty;
  the public application parity run did the same for its generated prefix.
- The three isolated notes roots, isolated app-data directory, stable QA app
  bundles, and the temporary cleanup helper were moved to the macOS Trash. All
  original paths were verified absent. The user-supplied credential source file
  was not modified or deleted.
- Credentials were read only into process-local environment variables from the
  supplied temporary source. They were not printed, copied into source, added
  to a report, or committed. Endpoint evidence uses only `local` and `public`
  labels.

## Verification

Final verification after removing the temporary conflict-hook experiment:

- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`: 1112 passed,
  0 failed, 4 ignored.
- `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check`:
  passed.
- Final `@markra/app` suite: 147 files and 1861 tests passed.
- `pnpm test`: all workspace projects passed before the temporary two-test
  conflict experiment was removed; the affected application package was then
  rerun in full with the final 1861-test state above.
- `pnpm typecheck:test`: passed for all participating packages.
- `pnpm build`: passed; desktop verification imported 12 vendor chunks.
- `pnpm brand:verify`: passed.
- `pnpm test:dejavu-oracle:unit`: 5 passed.
- Pinned upstream Dejavu source was clean at the documented commit.
- `pnpm test:dejavu-oracle`: all 27 Go scenarios, all Go packages, and all 5
  Rust scenario tests passed.
- `pnpm test:dejavu-interop`: all 7 bidirectional Go/Rust scenarios passed.
- `local` real S3: 3 application tests, 1 independent prefix verifier, and 1
  Dejavu matrix passed.
- `public` parity: 3 application tests and 1 independent prefix verifier
  passed.
- `git diff --check`: passed.

## Scope

- `.serena/` and the new conflict-design plan are intentionally excluded.
- No conflict-resolution semantic change is included.
- No push or merge into `main` was performed by Task 8.
