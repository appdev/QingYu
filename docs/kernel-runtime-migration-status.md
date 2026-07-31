# Kernel Runtime Migration Status

> Status snapshot: 2026-08-01 (Asia/Shanghai)
>
> Implementation snapshot commit: `5c3af461b1298882bc0d2a44a347acaf4a4e52c2`
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
| P4 — Mobile in-process Kernel | Implementation substantially complete | In-process mobile Kernel ownership, shared runtime composition, lifecycle settlement, portable settings, and mobile image-import integration are in `main`. | Finish the current durable resource-batch gate, then run final iOS/Android builds and the available native acceptance matrix. |
| Resource batch durability | In progress; release blocker | Batch API, bounded HTTP body, authentication/CSRF/Origin checks, seven-format validation, SVG hardening, publication fencing baseline, and OpenAPI/client integration are in `main`. An isolated worktree is adding durable journal/receipt replay and crash recovery. | Review and commit the durable transaction; prove restart recovery, idempotency, sync admission, capacity bounds, and fail-closed behavior before integration. |
| Final combined verification | Pending | Earlier combined candidates passed Kernel/Desktop Rust and workspace pnpm gates, but those results predate the durable transaction. | After serial integration, run one complete Kernel/Desktop Rust, pnpm test/typecheck/build, OpenAPI/generated-contract, iOS, and Android gate on one frozen SHA. |
| Final live acceptance | Pending | Earlier macOS/Linux and HTTP/HTTPS/WSS evidence is baseline-only. | Use new worktrees and artifacts built from the frozen final SHA. Do not reuse an older candidate as release evidence. |

## Current Release Blockers

1. Complete and independently review the durable resource-batch transaction.
2. Serially integrate it onto the latest `main` without losing intervening user
   changes or retained stashes.
3. Run the one final combined automated regression on the resulting frozen SHA.
4. Repeat macOS real-GUI and Linux runtime-only Docker HTTP/HTTPS/WS/WSS
   acceptance using artifacts from that exact SHA.
5. Run the two-endpoint live S3 matrix only when credentials are available through
   non-echoing process input; never record them in repository files or evidence.
6. Resolve the mobile compatibility decision if hard seven-format parity remains
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
  When their candidate differs from this page's snapshot or final frozen SHA,
  treat them as historical evidence.
- Update this page whenever a phase changes state, a release blocker is added or
  removed, or the final candidate SHA changes.
