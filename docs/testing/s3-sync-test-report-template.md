# S3 Sync Test Report

## Build

- Date/time (UTC):
- Commit:
- Branch:
- Platform:
- Desktop build or launch command:
- App version:

## MinIO Target

- Endpoint host only:
- Bucket:
- Prefix root:
- Automated run ID or desktop run suffix:
- Credentials recorded in this report: **No**

Do not add access keys, secret keys, authorization headers, signed URLs, or signed query parameters.

## Automated Live Integration

- Command: `pnpm test:s3-sync:live`
- Passed:
- Failed:
- Duration:
- Cleanup remaining objects:
- Failure run IDs, if any:

| Scenario group | Result | Notes |
| --- | --- | --- |
| Harness and cleanup | | |
| Create/read/update | | |
| Topology, empty, binary, pagination | | |
| Delete and changed survivors | | |
| Conflicts and stabilization | | |
| Manifest, target binding, ignored paths | | |
| Named catalog, A -> B -> A engine scopes, selected-A hydration, settings identity | | |
| Checkpoint, concurrency, atomicity | | |
| Invalid credentials and missing bucket | | |

## Desktop Entry Points

| Trigger | Prefix | Local SHA-256 | Remote SHA-256 / ETag | Last sync changed | Disabled control | No-op stable | Cleanup count | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Settings Sync now | | | | | N/A | | | |
| File menu Sync now | | | | | N/A | | | |
| Sync shortcut | | | | | N/A | | | |
| Sync after save | | | | | | | | |
| Scheduled sync | | | | | | | | |

Evidence locations must not expose credential fields:

- Settings manual:
- Native menu/shortcut:
- Save enabled/disabled:
- Schedule enabled/disabled:

## Named Notebook And Restore Runtime Evidence

Automated live coverage in the previous section is provider/catalog/engine evidence only. Record the desktop application-service and UI results separately here.

| Check | Remote root / local fixture | Evidence | Result |
| --- | --- | --- | --- |
| A syncs only to `notes/A/` | | | |
| Switch A -> B -> A keeps provider settings and remote keys isolated | | | |
| Unchanged `app/settings.json` keeps its identity across switches | | | |
| Standalone Markdown file does not retarget synchronization | | | |
| Shallow catalog lists A and B without downloading either | | | |
| Selecting A creates or reuses only the local A child | | | |
| Same-name A hydrates remote content before local-only publication | | | |
| Exact remote-root cleanup relists zero objects | | | |

## Offline and Build Verification

| Command | Result | Tests / notes |
| --- | --- | --- |
| `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml` | | |
| `pnpm test` | | |
| `pnpm typecheck:test` | | |
| `pnpm build` | | |
| `pnpm tauri build --debug` | | |

## Defects and Investigation

- Defect:
- Reproduction:
- Root cause:
- Regression test:
- Fix commit:
- Re-verification:

## Final Gate

- [ ] Every automated live scenario passed against real MinIO.
- [ ] Settings manual, native menu/shortcut, save-after, and scheduled sync each produced verified MinIO mutations.
- [ ] A -> B -> A used isolated `notes/<directory-name>/` keys while retaining one application-level provider configuration.
- [ ] A clean-device restore downloaded only the selected notebook, and same-name hydration preceded local-only publication.
- [ ] Portable `app/settings.json` remained stable across notebook switches when preferences did not change.
- [ ] Disabled save and scheduled controls produced no MinIO mutation.
- [ ] Conflict stabilization and no-op runs were stable.
- [ ] Every isolated prefix was cleaned to zero objects.
- [ ] No credential material appears in source, logs, screenshots, or this report.
- [ ] Offline tests, type checking, builds, and desktop debug packaging passed.

Overall result:

Open blockers:
