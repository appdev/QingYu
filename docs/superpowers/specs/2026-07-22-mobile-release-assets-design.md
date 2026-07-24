# QingYu Mobile Release Assets Design

## Context

QingYu already has a manual GitHub Actions release workflow that builds desktop bundles and uploads them to a GitHub Release. The workflow currently assumes that updater, macOS signing, and notarization secrets exist. The `appdev/QingYu` repository has no published Releases and no Actions secrets, so that workflow cannot complete in its current form.

The repository already supports the public commands `pnpm app build android` and `pnpm app build ios`. Local release verification has produced an Android APK and an Apple Silicon iOS Simulator application. This change should connect those existing build surfaces to the release pipeline without representing unsigned development artifacts as store-ready packages.

## Goals

- Keep every GitHub workflow manual-only.
- Build desktop, Android, and iOS artifacts in one Release run.
- Upload every produced package to the GitHub Release created by that run.
- Generate a deterministic commit list from the previous published Release to the current Release target.
- Let the repository publish a Release before signing secrets are configured.
- Preserve the existing signed desktop updater path when all required secrets are configured later.
- Run and verify the first Release in `appdev/QingYu` as `v1.7.5` without moving an existing tag.

## Non-goals

- Publishing to Google Play, TestFlight, or the Apple App Store.
- Claiming that an unsigned Android APK is a production distribution package.
- Claiming that an iOS Simulator application can be installed on an iPhone or iPad.
- Creating or storing signing credentials in the repository.
- Automatically triggering a Release from pushes or tags.

## Considered Approaches

### 1. Require all production signing secrets

This would produce the most polished artifacts, but it would keep the current repository unable to publish anything. Android needs a persistent private keystore, while iOS device distribution needs Apple Developer certificates and provisioning. Those credentials do not exist in the repository today.

### 2. Publish only unsigned builds and remove the signed path

This is the smallest workflow, but it would discard the existing updater signing, macOS notarization, and Homebrew integration. It would make future production distribution harder.

### 3. Optional signing with explicit mobile artifact scope

This is the selected approach. Desktop signing and updater generation remain available when their complete secret sets exist, and otherwise desktop builds use Tauri's no-sign mode. Android and iOS publish clearly named development artifacts until their platform signing credentials are added in a later change.

## Workflow Design

The Release workflow remains `workflow_dispatch` only. It accepts a `tag_name` and `draft` input. All build jobs check out the workflow dispatch commit rather than requiring the tag to exist before the run. The workflow validates that the requested tag is exactly `v` plus the application version. The publishing job gives `softprops/action-gh-release` the dispatch commit as `target_commitish`, allowing GitHub to create the new tag only after every build succeeds.

The desktop matrix remains:

- macOS Apple Silicon
- macOS Intel
- Linux x64
- Linux ARM64
- Windows x64

A capability step checks only whether complete signing secret sets are present. It never prints secret values. If updater signing is available, the existing updater configuration, signatures, metadata, and `latest.json` are generated. If it is unavailable, updater-only steps are skipped and ordinary desktop bundles are still uploaded. macOS certificate import and notarization are similarly enabled only when the complete Apple desktop secret set is present; otherwise the macOS build receives `--no-sign`.

The mobile builds run as two independent jobs:

- Android runs on Ubuntu, installs Java and the Android Rust target, and executes `pnpm app build android --apk --target aarch64 --ci`. It publishes a normalized `QingYu_<version>_android_arm64_unsigned.apk`.
- iOS runs on macOS Apple Silicon, installs the simulator Rust target, and executes `pnpm app build ios --target aarch64-sim --no-sign --ci`. It zips the generated `.app` directory as `QingYu_<version>_ios_simulator_arm64_unsigned.app.zip`.

The publishing job downloads desktop and mobile workflow artifacts, excludes internal metadata files, and passes every remaining file to the GitHub Release action.

## Release Notes

A repository-owned Node script replaces the floating `pnpm dlx changelogen@latest` call. The script uses the authenticated GitHub Releases API to list published, non-draft Releases in newest-first order. It ignores the current tag and selects the newest Release whose tag exists locally and is an ancestor of the current release target. This makes the range follow published Releases rather than unrelated Git tags.

For a normal Release, the script writes every commit in `previous_release_tag..current_target` in chronological order with short SHA, subject, and author. For the repository's first Release, it writes every commit reachable from the current target. The notes also explain that the Android APK is unsigned and that the iOS archive targets Apple Silicon Simulator only.

API failures, an invalid release target, or a Release tag that already points to another commit fail the workflow instead of silently generating a misleading range.

## Versioning and First Run

The implementation increments all application version sources from `1.7.4` to `1.7.5`. The existing `v1.7.4` tag remains unchanged. After verification and push, the Release workflow is dispatched with `tag_name=v1.7.5` and `draft=false`. Because `appdev/QingYu` currently has no Releases, the first generated note contains the full commit history through `v1.7.5`; later releases use the previous published Release boundary.

## Testing

- Add focused Node tests for previous-Release selection, ancestry filtering, first-Release fallback, commit formatting, and mobile disclosure text.
- Add workflow contract tests for manual-only triggering, mobile build commands, artifact normalization, dispatch-commit checkout, optional signing, all-artifact publication, and the release-notes script.
- Run `pnpm test:release`, `pnpm test`, `pnpm typecheck:test`, `pnpm build`, and `git diff --check`.
- Push `main`, manually dispatch the Release workflow, and verify all build jobs, the created `v1.7.5` Release, the commit notes, and downloadable desktop/mobile assets through the GitHub API.

## Security and Future Upgrade

No signing secret, generated keystore, certificate, provisioning profile, or local endpoint is committed. Artifact names and release notes state exactly what can be installed. A future signing change can add a persistent Android keystore and iOS distribution credentials using GitHub Secrets without changing the release range model or the public release workflow contract.
