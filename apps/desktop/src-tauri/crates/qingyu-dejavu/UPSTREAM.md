# Upstream provenance

The `qingyu-dejavu` crate is a Rust translation boundary for selected sources
and behavior from the following pinned upstream repositories:

- Dejavu: <https://github.com/siyuan-note/dejavu> at
  `8462fe30163c6e6e95ae2da832cfe76058e0e830`.
- SiYuan: <https://github.com/siyuan-note/siyuan> at
  `eef10568384e2e7cf547adb029ae46a72e43c287`.

SiYuan is a pinned behavioral reference for later app integration. No SiYuan
implementation file is translated in this core milestone.

## Source mapping

- Task 2 translates or references Dejavu `entity/chunk.go`, `entity/file.go`,
  `entity/index.go`, `entity/stat.go`, `diff.go`, `util/hash.go`, and their
  pinned tests.
- Task 3 translates or references Dejavu `store.go`, `store_test.go`,
  `entity/index.go`, `util/disk.go`, and `util/disk_mobile.go`. It also
  translates the AES/KDF implementation from
  <https://github.com/siyuan-note/encryption> at
  `v0.0.0-20260715062728-9cb8e9548044`, specifically `aes.go`, `kdf.go`, and
  their tests.
- Task 4 translates or references Dejavu `repo.go`, `repo_test.go`, and the
  Rabin implementation from <https://github.com/restic/chunker> at `v0.5.0`,
  specifically `chunker.go`, `options.go`, `polynomials.go`, and tests.
- Task 5 translates or references Dejavu `ref.go`, `ref_test.go`, `repo.go`,
  `repo_test.go`, `store.go`, `store_test.go`, and the history portions of
  `sync.go` and `sync_test.go`.
- Task 6 translates or references Dejavu `cloud/cloud.go`, `cloud/local.go`,
  `sync_lock.go`, and `sync_test.go`, plus coordination semantics defined by
  the QingYu plan. There is no Go structural copy for `working_tree.rs`.
- Task 7 translates or references Dejavu `sync.go`, `sync_manual.go`,
  `sync_test.go`, `repo.go`, `diff.go`, `ref.go`, and `backup.go` where called
  by the state machine.
- Task 8 translates or references Dejavu `test/sync/sync_scenario_test.go` and
  the four exact `test/sync/testdata/cases/{basic,edge,known-conflicts,sync-download}/config.json`
  fixtures.
- Task 9 adds no upstream translation; it is verification only.

## License attribution

- QingYu and Dejavu-derived code are AGPL-3.0-only. Dejavu `LICENSE` is the
  source for this attribution.
- Translated AES/KDF source from `siyuan-note/encryption` is Mulan PSL v2. Its
  `LICENSE` is the source for this attribution.
- Translated Rabin source from `restic/chunker` is BSD-2-Clause. Its `LICENSE`
  is the source for this attribution.
