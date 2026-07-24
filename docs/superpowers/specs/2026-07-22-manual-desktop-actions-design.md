# Manual Desktop GitHub Actions Design

## Goal

Move QingYu's GitHub automation to `appdev/QingYu` with no automatic triggers. The first build workflow packages desktop applications only and can be run successfully without release-signing secrets.

## Workflow boundaries

- `ci.yml` keeps its validation jobs but exposes only `workflow_dispatch`.
- `release.yml` keeps the future signed desktop-release path but exposes only `workflow_dispatch`; its updater endpoint points to `appdev/QingYu`.
- `desktop.yml` is the first operational build workflow. It exposes only `workflow_dispatch`, builds unsigned desktop bundles on macOS, Windows, and Linux, and uploads each platform's bundle directory as a workflow artifact.
- No workflow contains a `push`, `pull_request`, `schedule`, or tag trigger.
- Android and iOS commands, runners, and artifacts are absent from the desktop workflow.

## Desktop build flow

Each matrix job checks out the dispatched commit, installs the pinned pnpm and Node versions, installs stable Rust, restores the Cargo cache, installs Linux system libraries when needed, installs the frozen pnpm workspace, and runs:

```text
pnpm app build desktop --no-sign
```

The job then uploads the platform bundle directory with `actions/upload-artifact`. Unsigned artifacts are intentional for this first CI iteration; store distribution and code signing remain in the separate manual Release workflow.

## Verification

Before pushing, validate workflow structure and repository tests that cover build tooling. After pushing `main`, manually dispatch Desktop Build in `appdev/QingYu`, monitor every matrix job, inspect failed logs if necessary, and require a successful terminal result with uploaded artifacts before completion.

## Scope protection

Only GitHub Actions files, their focused validation coverage, and this design are part of the change. Existing local edits to `apps/desktop/src-tauri/Cargo.toml` and the untracked `macos-icon.icns` are preserved but excluded from commits.
