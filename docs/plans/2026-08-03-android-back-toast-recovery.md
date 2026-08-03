# Android Back and Error Notice Recovery Implementation Plan

**Goal:** Prevent Android system Back from leaving a live QingYu process without an Activity/WebView, and keep runtime-error notices usable on narrow screens.

**Architecture:** Intercept Android Back in `MainActivity` before Activity destruction and deliver a fixed DOM event to the already-loaded renderer. The renderer must acquire a one-shot, origin-bound Rust authorization before running the existing compact-navigation/dirty-state guard and acknowledging the result. Uncoded exit requests become a graceful Kernel-draining process exit instead of an attempted emit to an already-destroyed window. Sonner keeps its desktop intrinsic width token, but no inline `width` may override its mobile media query.

## Constraints

- Start from `c2b22e1f76a564e5a62c6b42e9be9e39ea18f024`; do not merge, push, install, deploy, or touch the preserved endurance environment.
- Keep sync authority, renderer-origin validation, credential redaction, dirty-state guards, and Kernel drain behavior intact.
- Do not change Docker tracked inputs or unrelated behavior.
- Use focused RED/GREEN checks during iteration, then the repository-required full checks and an Android build.

## Task 1: RED regression coverage

- Add a frontend test proving system Back is delivered through the pre-destruction DOM bridge, must acquire native authority before navigation, coalesces duplicates, fails closed, and acknowledges the guarded result.
- Add root-editor regressions proving Back waits for the existing navigation save flush, coalesces repeated attempts, and stays in the app when persistence fails.
- Add Rust/source-boundary tests proving Back commands are origin-bound, Android installs the pre-destruction callback, and uncoded exits use graceful Kernel shutdown rather than emitting into a destroyed WebView.
- Add a notice regression assertion proving the toaster has no inline width capable of overriding Sonner's mobile geometry.

## Task 2: Minimal GREEN implementation

- Register an AndroidX Back callback in `MainActivity`, retain the created WebView, and dispatch the fixed Back DOM event without destroying or backgrounding the Activity.
- Add an origin-validated `begin_mobile_back` command, retain the existing one-shot pending state and completion command, and bind completion to the same renderer authority.
- Run the Compact editor's existing `saveState.flush("navigation")` guard before authorizing a root Back exit.
- Route uncoded exit requests through the existing Kernel stop/terminal-exit path.
- Remove only the toaster's explicit inline width while retaining its desktop `--width` token and fixed positioning.

## Task 3: Verification and handoff

- Run focused TypeScript and Rust regression tests, formatting, lint/type checks, complete tests/builds, and the Android Gradle build without installing it.
- Run narrow-viewport browser geometry QA for the runtime-error notice.
- Commit the isolated branch, request independent read-only review, record commit/tree, and send one final callback to the parent endurance task with evidence and three-platform rebuild requirements.
