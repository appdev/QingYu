# QingYu Engineering Guidelines

This document only defines engineering conventions for this repository.

## Package Management

- Use `pnpm` for all JavaScript and frontend dependency workflows.
- This repository is a pnpm workspace. The desktop app lives in `apps/desktop`; reusable TypeScript packages live in `packages/`.
- Common commands:
  - `pnpm install`
  - `pnpm dev`
  - `pnpm test`
  - `pnpm build`
  - `pnpm tauri ...`
- Keep `pnpm-lock.yaml`.
- Do not add `package-lock.json`, `yarn.lock`, `bun.lockb`, or lockfiles from other package managers.

## Tech Stack

- Desktop shell: Tauri v2.
- Frontend: React, TypeScript, Milkdown, and Tailwind CSS.
- Icons: prefer `lucide-react`.
- Styling should use Tailwind CSS as much as practical. Global CSS should be reserved for design tokens, base styles, Milkdown/Markdown generated content, and platform-level polish.

## Code Organization

- Keep changes small and focused.
- Avoid unrelated refactors.
- Apps belong in `apps/`; reusable packages belong in `packages/`.
- Current package boundaries:
  - `packages/shared`: cross-cutting types, i18n, small pure utilities, and runtime debug logging.
  - `packages/app`: the shared React application surface and product UI.
  - `packages/editor`: Milkdown editor adapters, shortcuts, input rules, and selection handling.
  - `packages/markdown`: Markdown parsing and Markdown asset/path helpers.
  - `packages/scripts`: shared repository and release scripts.
  - `packages/ui`: reusable UI primitives.
- Keep Tauri frontend bridge code in `apps/desktop/src/lib/tauri`; it is app shell integration, not a shared package.
- Keep desktop-only build tooling such as the debug-strip Vite plugin in `apps/desktop/scripts/`.
- Keep shared cross-cutting TypeScript in `packages/shared` and import it through `@markra/shared` public exports.
- Split reusable UI into components instead of concentrating layout and behavior in `App.tsx`.
- Keep business logic, platform integration, and editor adapter logic in clear modules.
- Prefer established libraries for mature domains such as editor behavior, Markdown parsing, and platform APIs.
- Do not use the TypeScript `void` keyword or operator. Use `unknown`, omit explicit callback return annotations when practical, or call promises directly with their own error handling.
- Do not revert user changes unless explicitly requested.

## Testing Boundaries

- Add or update focused tests when changing business logic, user-facing behavior, editor behavior, file reliability, sync flows, or other product functionality.
- Configuration files and other code that does not implement business logic or product functionality do not require unit tests.
- Text-only changes do not require unit tests, including copy edits, label wording, placeholder text, static help text, and translation wording updates.
- Removed features do not need dedicated unit tests that only prove the feature no longer exists. Prefer deleting obsolete tests and keeping coverage focused on the remaining supported behavior.
- The following do not require unit tests by default: `package.json`, lockfiles, `tsconfig` files, Vite config, Tauri config, generated files, build scripts, static metadata, documentation, pure formatting changes, and pure styling changes.
- If a configuration change affects runtime behavior, verify it with the relevant command or integration build instead of forcing a unit test for the configuration file itself.
- Before reporting completion for code changes, run the smallest meaningful verification. Common checks are `pnpm test` and `pnpm build`; desktop packaging changes can use `pnpm tauri build --debug` when practical.

## Repository Hygiene

- Do not commit generated directories such as `node_modules/`, `dist/`, or `apps/*/src-tauri/target/`.
- Do not commit temporary caches, debug artifacts, or local environment files.
- Confirm that a new dependency is actually needed before adding it.
- Prefer reusing the current stack over introducing additional frameworks or tools.
- Update this document when repository-wide conventions change.

## Local Upstream Integration

- The canonical upstream repository is [markrahq/markra](https://github.com/markrahq/markra). Keep it configured as the `upstream` remote and treat it as read-only: fetch and merge from it, but do not push to it.
- Keep the private downstream repository configured as `origin`. Push local `main`, customization branches, and local merge commits only to `origin`, and only when the user explicitly requests a push.
- Check upstream changes with `git fetch upstream`; do not use an unreviewed direct pull to replace, rebase, reset, or discard local commits.
- Integrate new `upstream/main` commits in an isolated worktree and temporary local branch first. Preserve existing user changes and do not modify unrelated untracked files in the primary checkout.
- Preserve local product capabilities when resolving upstream changes, including the S3 note-folder `SyncProvider`, its shared remote-sync engine, triggers, conflict handling, and live MinIO coverage. Do not accept an upstream side merely because it lacks a local feature.
- Before updating local `main`, verify the integrated commit with `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`, `pnpm test`, `pnpm typecheck:test`, and `pnpm build`. Run `pnpm test:s3-sync:live` when the configured real MinIO test server is available.
- Update local `main` only after the integration checks pass. Then remove the temporary integration worktree and branch while leaving the local customization history intact.
- Never commit test credentials, access keys, secret keys, or local endpoint configuration.
