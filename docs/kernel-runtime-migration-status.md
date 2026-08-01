# Kernel Runtime Migration Status

> Status snapshot: 2026-08-01 (Asia/Shanghai)
>
> Verified AppConfig implementation SHA: `856c3c104bf11e2b0b975757124cd63f0f8d05b2`.
> This candidate includes the latest `main` integration at `e6266836`, the
> deterministic desktop MCP lock-order test repair, the unified managed-workspace
> restore fix, the Docker tracked-input refresh, and the compact mobile acceptance
> fixture update. Independent implementation reviews are complete. The automated
> gates described below passed on this SHA; outstanding live-environment rows
> remain explicitly pending.
>
> This is the progress source of truth for the active Desktop, Server Web/Docker,
> and Mobile Kernel migration. Historical test reports remain evidence for their
> recorded candidate only.

## Approved Architecture Baseline

- The React/Web application is the shared product UI.
- The Kernel owns documents, settings, resources, history, sync, persistence,
  lifecycle, and writer authority.
- Desktop selects a local workspace and supervises one authenticated loopback
  child Kernel. Normal operation has no legacy native-writer fallback.
- One Server Web/Docker deployment represents one user. It always owns the fixed
  `/data` hierarchy and never exposes workspace selection or switching.
- Server Web uses the browser-facing HTTP/WebSocket Kernel contract. Direct HTTP
  and externally terminated HTTPS are both supported with scheme-specific cookie
  profiles and exact Host/Origin/CSRF validation.
- Mobile embeds an in-process Kernel and presents the shared application through
  its native WebView. Platform adapters own only operating-system integration and
  lifecycle differences.
- Desktop, Server Web/Docker, Android, and iOS use one Kernel AppConfig service
  for normal settings and durable cold-start UI state. No official Kernel-backed
  client uses browser storage, Tauri Plugin Store, or process memory as its
  authoritative layout store.
- Durable configuration is grouped under each platform's ConfigRoot:
  `settings.json` contains AppConfig, `sync-config.json` retains its typed secret
  boundary, desktop MCP uses `mcp.json`, and desktop alone keeps
  `primary-workspace.json` as pre-Kernel workspace-selection metadata.
  Operational manifests, journals, checkpoints, locks, and recovery state remain
  under InstanceDataRoot.
- A ready workspace with no valid remembered file or recoverable draft renders
  the shared Workspace Home. Missing remembered paths are pruned; a dirty draft
  whose source disappeared restores in the editor instead of Home.
- CLI remains a Kernel process and automation entrypoint; it does not own a
  second implementation of product business logic.
- S3/WebDAV synchronization remains Kernel-owned. Sync baselines are isolated by
  the non-secret remote target identity so endpoint, bucket, and repository-root
  switching cannot reuse another target's deletion baseline.
- Shared Kernel/DTO/Cargo/runtime entry boundaries are integrated serially. Safe,
  non-overlapping implementation and review work may run in isolated worktrees.

## Phase Ledger

| Phase | State | Current evidence | Remaining gate |
| --- | --- | --- | --- |
| P1 — Kernel foundation and service boundaries | Complete | Kernel HTTP/WS contract, document/settings/resource/history/sync services, runtime ownership, and generated contract checks are in `main`. | Revalidated as part of the final combined suite. |
| P2 — Desktop production Kernel cutover | Code complete | `db20eb2a` completed the production cutover; later commits added atomic workspace switching, startup readiness, child supervision, MCP-to-Kernel routing, writer fencing, and recovery handling. | Repeat the real macOS GUI/Desktop regression on the final combined SHA. |
| P3 — Server Web and runtime-only Docker | Code complete; final-candidate acceptance pending | Fixed `/data`, one-user initialization, browser KernelClient, HTTP/HTTPS cookie profiles, WS/WSS, runtime-only packaging, restart persistence, and Linux container security were implemented. Earlier Linux candidates passed the core runtime/browser matrix. | Rebuild and repeat macOS/Linux runtime acceptance on the final combined SHA. Two live S3 endpoints run only when credentials can be injected without disclosure. |
| P4 — Mobile in-process Kernel | AppConfig integration and native builds complete; full native acceptance pending | The verified candidate integrates in-process Kernel ownership, shared runtime composition, lifecycle settlement, portable settings, mobile image import, a fixed managed workspace, and Kernel AppConfig ownership. Android aarch64 APK and iOS arm64 Simulator builds passed. A clean earlier-candidate iPhone 16 Pro / iOS 18.6 Simulator install opened Workspace Home, and a terminate/relaunch returned to Home. Mobile does not support or package MCP. | Complete the remaining final-SHA Android emulator and iOS/real-device AppConfig/Home lifecycle matrix. |
| Resource batch durability | Complete | `e1b409da` added durable journal/receipt replay and crash recovery; `a7d3a7bc` integrated the rollout, with later image/import fixes through downstream baseline `31ee52ce`. | Revalidate as part of the final combined suite; do not reuse the earlier candidate as evidence for later AppConfig changes. |
| Kernel AppConfig and Workspace Home | Code and automated acceptance complete | Verified SHA `856c3c10` moves configuration to ConfigRoot, adds the aggregate AppConfig service/API/client, unifies official client bootstrap and writes, adds deterministic restoration/Home, and removes obsolete native/local writers. Independent implementation reviews approved the result. | Complete the remaining real Desktop, Docker, Android, and iOS lifecycle rows before a release claim. |
| Final combined verification | Passed on verified implementation SHA | Kernel tests passed with 1014 passed / 3 ignored; Desktop Rust with 1252 passed / 6 ignored; formatting, OpenAPI/generated-client coverage, repository tests (3490), type checking, production builds, Android aarch64 APK build, and iOS arm64 Simulator build all passed. Docker contract and its 11 mutation tests also passed. | Rerun only gates affected by later code changes; documentation-only follow-ups do not invalidate this code evidence. |
| Final live acceptance | Partially complete; release acceptance still pending | An earlier candidate's clean iPhone 16 Pro / iOS 18.6 Simulator install and terminate/relaunch both rendered Workspace Home. The verification host has no Docker command, no configured Android AVD or physical mobile device, and no live S3 credentials. | Complete final-SHA real macOS GUI, Docker/browser/volume, Android emulator/device, iOS lifecycle, and credential-gated live sync rows. |

## Current Release Blockers

1. Repeat macOS real-GUI acceptance, including workspace-partitioned restoration,
   missing-file Home, dirty-draft recovery, and the absence of local-state or
   Plugin Store writes, using an artifact from that exact SHA.
2. Run Linux runtime-only Docker HTTP/HTTPS/WS/WSS and persistent-volume
   acceptance on that exact SHA. The current documentation host has no Docker
   command, so refresh, second-browser, restart/replace, and volume inspection
   remain unclaimed.
3. Run the remaining Android and iOS native AppConfig/Home lifecycle acceptance.
   The iOS 18.6 Simulator fresh-launch and terminate/relaunch Home rows passed;
   Android installation is pending because no AVD or physical device is configured.
   MCP is outside mobile scope and must not be added to this matrix.
4. Run the two-endpoint live S3 matrix only when credentials are available through
   non-echoing process input; never record them in repository files or evidence.
5. Resolve the mobile compatibility decision if hard seven-format parity remains
   required: static AVIF support conflicts with the current older iOS/Android
   minimums. Real-device-only acceptance remains pending until devices exist.

## Verification Policy

- Candidate branches run focused tests, formatting, diff checks, type checks, and
  the smallest compile needed for their changed boundary.
- The complete repository suite runs once on the serially integrated combination.
- If a later fix changes a shared Kernel/DTO/runtime contract, rerun the affected
  complete gate. Small isolated corrections receive risk-matched focused reruns.
- A successful old-SHA browser, GUI, Docker, or S3 run is useful diagnostic
  evidence but is not final release acceptance.
- No push is part of this migration unless the user explicitly authorizes it.

## Documentation Semantics

- The server sync lifecycle plan records the completed implementation steps for
  that bounded Kernel task; its checkboxes do not describe the entire migration.
- Docker and mobile acceptance documents describe their own artifact/candidate.
  When their candidate differs from the current review candidate or final verified SHA,
  treat them as historical evidence.
- Update this page whenever a phase changes state, a release blocker is added or
  removed, or the final candidate SHA changes.
