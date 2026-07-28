# QingYu V2 Downstream Integration Design

## Goal

Update the customized QingYu downstream from `upstream/main` to `upstream/v2` while accepting the V2 CodeMirror editor architecture and compatible upstream improvements, preserving QingYu-only capabilities and branding, and preventing previously removed product capabilities from returning under new V2 files or APIs.

## Current State

- Local `main` is a rewritten downstream history whose tree contains deep QingYu customization.
- `upstream/v2` is based on `upstream/main`; the two branches do not share the downstream rewrite as a normal first-parent history.
- A previous isolated audit established that a bridge commit with the local tree and `upstream/main` as the second parent gives Git the correct semantic merge base.
- The refreshed upstream target is `801ce49815bf01657bdec886f306ad97a4892913` (`2.0.0-beta.5`).
- The V2 delta contains 64 commits, dominated by the Milkdown-to-CodeMirror migration and its follow-up fixes.

## Integration Strategy

Create a dedicated branch and worktree from the current local `main`. Add a bridge merge using the `ours` strategy against `upstream/main`, then merge `upstream/v2` normally. Resolve the result by product capability rather than by choosing one side for whole files.

This retains a real upstream ancestry edge for future V2 merges while keeping the local tree as the starting downstream state. The final integration commit records both the upstream V2 parent and the resolved QingYu product tree.

## Accepted V2 Capabilities

- Replace the Milkdown editor implementation with the V2 CodeMirror editor and `packages/editor-react` bridge.
- Accept V2 editor correctness fixes for IME, selections, code blocks, tables, Markdown syntax visibility, links, images, Mermaid, frontmatter, footnotes, block dragging, folding, and paste behavior.
- Accept non-deleted editor preferences such as Vim mode and typewriter mode.
- Accept Alt-only shortcut support.
- Accept collapsible file-list controls.
- Accept safe unused-image cleanup when it can be wired without weakening QingYu's local-path and sync safety rules.
- Accept the custom-theme enable/disable toggle while keeping QingYu's existing built-in and imported CSS theme pipeline.
- Accept the preview updater channel only if it composes with the downstream release workflow and does not restore removed proxy settings.

## Hard Removal Contract

The merge must not restore these capabilities, even if V2 implements them in new editor or runtime modules:

- AI chat, AI agent, inline AI commands, provider configuration, model catalogs, web-search tools, ACP integration, and AI screenshots or product copy.
- `packages/ai` and `packages/providers` workspace packages.
- CodeMirror AI preview events, decorations, held AI selections, UI, styles, tests, and controller hooks.
- Network/proxy settings, proxy request fields, proxy persistence, proxy diagnostics, native proxy application, and the `reqwest` SOCKS feature.
- Theme export UI, TypeScript runtime methods, Tauri commands, Rust archive/export code, export-only CSS assets, locale keys, and tests.
- Any compatibility reader or hidden fallback whose only purpose is to accept one of those removed contracts.

Historical changelog entries may remain. Current product documentation, settings schemas, runtime contracts, and shipped assets must describe only supported behavior.

## Downstream Capabilities That Must Survive

- QingYu/轻语 product name, identifiers currently used by the downstream, application icons, web/marketing assets, and local icon-generation choices.
- The S3 note-folder `SyncProvider`, shared remote-sync engine, conflict handling, sync triggers, live MinIO coverage, and current safe-error behavior.
- Existing Dejavu design and implementation-plan documents on local `main`; the separate in-progress Dejavu worktree is outside this integration and must not be modified or silently absorbed.
- Settings-owned cloud notebook restore and cross-window lifecycle behavior.
- Local document history and its existing non-synced storage boundary.
- QingYu MCP bridge and app-owned service integration.
- `apps/site`, downstream repository links, release scripts, and the `origin`/read-only-`upstream` policy.
- Existing update checks and web-image download behavior, without proxy configuration.
- User-facing icon and layout changes unless a V2 editor integration requires a narrowly scoped adaptation.

## Conflict Resolution Rules

1. Use the V2 implementation for editor architecture and V2-only editor modules.
2. Remove AI-specific portions from otherwise accepted CodeMirror files instead of retaining the whole AI preview seam.
3. Use the downstream implementation for S3 sync, Dejavu, cloud restore, MCP, branding, icons, release configuration, and repository policy.
4. For delete/modify conflicts, preserve the deletion when the path belongs solely to a hard-removed capability.
5. For mixed files such as `App.tsx`, settings contracts, runtime indexes, `lib.rs`, locale types, and lockfiles, reconstruct the supported union explicitly.
6. Regenerate dependency lockfiles after manifests are resolved; do not resolve lockfiles by choosing one side.
7. Preserve upstream changelog history but keep current-facing QingYu documentation and links downstream-specific.

## Verification

Use Node 24 for pnpm commands because the local Node 26 runtime exposes an incompatible experimental `localStorage` global to Vitest/jsdom. Use the installed stable Rust toolchain directory directly because the user's `~/.cargo/bin` rustup links are currently stale.

The integration is accepted only after:

- Removed-feature source and dependency scans return no current-runtime matches.
- Focused CodeMirror/editor tests pass.
- Focused S3, cloud restore, settings, history, and updater tests pass. Dejavu runtime tests remain outside this branch because its implementation has not landed on local `main`.
- `cargo fmt --check` passes.
- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml` passes.
- `pnpm test` passes, with the known Save As test rerun in isolation if the existing full-suite flake recurs.
- `pnpm typecheck:test` passes.
- `pnpm build` passes.
- `pnpm test:s3-sync:live` runs when every required `MARKRA_TEST_S3_*` variable is configured; otherwise the environment limitation is recorded.
- A final tree comparison confirms the QingYu icons, branding, site, sync engine, MCP, and other downstream-only paths remain present.

## Landing Policy

Do not push to `upstream` or `origin`. After all checks pass, fast-forward local `main` to the verified integration branch without touching unrelated changes in the primary checkout, then remove the temporary integration worktree and branch.
