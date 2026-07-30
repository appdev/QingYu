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

## Shared scenario oracle

Task 8 copies these pinned Dejavu fixtures byte-for-byte and runs all
`7 + 5 + 4 + 11 = 27` scenarios through both the Go and Rust implementations:

| Dejavu source path | Scenarios | SHA-256 |
| --- | ---: | --- |
| `test/sync/testdata/cases/basic/config.json` | 7 | `1b4c0ef8c3c39e0b971260f30ff9bad4120f56d23d00cab42d78be97a1693268` |
| `test/sync/testdata/cases/edge/config.json` | 5 | `eef8aa5389688989a39da511c30bbad87501ea04dee2fcf8b67ae288f5df1875` |
| `test/sync/testdata/cases/known-conflicts/config.json` | 4 | `40941ce32657ff4fe08379a61e8e4e3ff2bf2ed7f489f46654b758cfb51b8596` |
| `test/sync/testdata/cases/sync-download/config.json` | 11 | `6134a21d9deee1381498beb899a8ed2667c6c98f4b08e91f217d3cfff89fec24` |

Run the pinned cross-language oracle with:

```bash
DEJAVU_SOURCE_DIR=/Volumes/extendData/Data/IdeaProjects/upstream/dejavu pnpm test:dejavu-oracle
```

The pinned fixtures have no standalone `rename` operation or destination
field. Their rename scenarios are encoded explicitly as `remove` followed by
`write`; the Rust runner executes those two operations without inventing a
fixture transformation.

### Approved QingYu filesystem deviations

QingYu keeps ordinary local files during `sync_download` conflicts and does
not implement Dejavu's `.sy` structured-content merge. Summary counts still
match every pinned `want` object exactly. Only the following filesystem
assertions differ; the Rust runner keys each entry by fixture path and verified
SHA-256, exact case, step/final location, operation, client, path, upstream
state, and QingYu state. It fails on an unknown, duplicate, or unused entry.

| Fixture | Case | Location | Client/path | Upstream state | QingYu state |
| --- | --- | --- | --- | --- | --- |
| `known-conflicts/config.json` | `sync download structured content merge candidate reports conflict` | final | `b` / `doc.txt` | bytes `first\n\nsecond\n` | bytes `first\nsecond changed\n` |
| `sync-download/config.json` | `sync download remote update conflicts with independent local edit` | step 8 `assert` | `b` / `local.txt` | bytes `local base\n` | bytes `local changed\n` |
| `sync-download/config.json` | `sync download remote delete conflicts with independent local edit` | step 8 `assert` | `b` / `local.txt` | bytes `local base\n` | bytes `local changed\n` |
| `sync-download/config.json` | `sync download remote update conflicts with local update` | step 7 `assert` | `b` / `doc.txt` | bytes `from a\n` | bytes `from b\n` |
| `sync-download/config.json` | `sync download remote delete conflicts with local update` | step 7 `assert_missing` | `b` / `doc.txt` | missing | bytes `from b\n` |
| `sync-download/config.json` | `sync download remote update restores over local delete` | step 7 `assert` | `b` / `doc.txt` | bytes `from a\n` | missing |
| `sync-download/config.json` | `sync download remote create conflicts with local create at same path` | step 7 `assert` | `b` / `new.txt` | bytes `from a\n` | bytes `from b\n` |

## License attribution

- QingYu and Dejavu-derived code are AGPL-3.0-only. Dejavu `LICENSE` is the
  source for this attribution.
- Translated AES/KDF source from `siyuan-note/encryption` is Mulan PSL v2. Its
  `LICENSE` is the source for this attribution.
- Translated Rabin source from `restic/chunker` is BSD-2-Clause. Its `LICENSE`
  is the source for this attribution.
