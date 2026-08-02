# Fatal notes sync repair plan

## Baseline and constraints

- Exact source commit: `3d023098eec6eb05e353bcb9c8b0e3c9762dc012`
- Exact source tree: `101bcc7af2bbbf551f74ec61e5d9cbb9af469a6f`
- Product version: `2.5.1`
- No live profile/S3 mutation, deployment, installation, app launch, merge, or push.

## Evidence-backed diagnosis

Kernel Documents and the DejaVu runner retain the same active workspace authority. `documents-v1`
contains history/recovery data only. The observed `scannedFiles=1` is the portable settings file;
the notes adapter currently reports zero scanned files regardless of the DejaVu index.

The fatal cross-host split is repository identity, not workspace path. The inspected macOS and
Android profiles bind different repository IDs, and their global-key fingerprints also differ. Both use the
same S3 target and configured remote root, but notes live below the legacy-compatible namespace
`qingyu/repositories/<repository-id>/repo`, while portable settings live below `<remote-root>/app`.
Kernel-backed runtimes cannot currently list or bind catalog repositories, so the UI cannot repair
that split. Fresh profiles silently generate a new repository UUID and key.

A second data-loss bug exists within one repository: file identity is derived from path plus mtime
truncated to one second. A different payload saved at the same path during that second is treated as
the previous immutable object and the run can succeed without uploading the edit.

## Repair design

1. Add a deterministic RED test for different contents at the same path with two millisecond mtimes
   in one second. Preserve the existing DejaVu ID format: when an immutable ID collision contains a
   different file descriptor, advance a bounded logical mtime by whole seconds and publish under the
   first free compatible ID without mutating the user's source-file metadata. Search the bounded
   collision chain and reuse the last semantically equal descriptor on later scans so an unchanged
   repository remains a no-op. Exhaustion or path/handle instability fails closed.
2. Preserve immutable S3 objects across independent hosts. Every no-overwrite upload uses an atomic
   conditional write. On `AlreadyExists`, compare chunks and File descriptors by decrypted semantic
   content rather than randomized ciphertext. If the remote historical File object conflicts, remap
   the local published index through the same bounded logical-time chain and never overwrite history.
3. Add Kernel S3 repository operations guarded by exact sync-config revision, retained instance and
   workspace authorities, and the runtime mutation coordinator. Catalog list returns only validated
   entries, enforces bounded pagination/item counts, and rejects malformed metadata. Binding first
   reads exact remote metadata, then transactionally rewrites only repository
   bindings while preserving device ID and global key. A pre-existing exact binding remains stable;
   stale roots can be explicitly relocated, and duplicate active roots remain impossible.
4. Before a successful S3 notes run, ensure the active local repository has catalog metadata. Existing
   metadata is validated and never overwritten. A new local repository publishes one metadata record
   with an atomic conditional write; remote conflicts are reread and then either accepted exactly or
   rejected fail closed.
5. Expose catalog/list, bind, key-state, key-import, and confirmed key-export through the Kernel
   contract/client/domain adapter. Enable DejaVu UI only for the three Kernel-backed hosts once these
   operations are wired. Key replacement preserves device ID, disables bindings, and requires a new
   explicit bind, matching the legacy safety model. A disabled but otherwise configured target may
   list and bind for recovery; the bind performs one in-memory one-shot sync without enabling the
   persisted scheduled configuration.
6. Add complete temporary executor fixtures. Seed a source through the Documents service, cover
   nested files, an empty file, rename, rapid content update, and delete; inspect the fake S3 object
   namespace and restore into fresh server, managed-mobile, and desktop workspace fixtures sharing
   the selected repository/key. Assert exact scanned-file accounting and stable no-op reruns. Add
   negative tests for wrong root, unsafe paths, stale revision, cancellation with zero remote I/O,
   malformed/oversized catalog data, and preservation of legacy profile fields.
7. Run focused RED/GREEN tests, Kernel tests, desktop Rust tests, full pnpm test, typecheck:test, and
   build. Then request an independent read-only review and address all valid findings before a single
   final callback to the parent task.
