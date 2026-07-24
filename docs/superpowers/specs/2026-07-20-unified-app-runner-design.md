# Unified App Runner Design

## Goal

Provide one cross-platform repository command for launching development builds and producing release builds for QingYu desktop, Android, and iOS targets.

The public interface is:

```bash
pnpm app <dev|build> <desktop|android|ios> [tauri options]
```

Examples:

```bash
pnpm app dev desktop
pnpm app dev android --open
pnpm app dev ios --open
pnpm app build desktop --no-sign
pnpm app build android --apk --target aarch64 --ci
pnpm app build android --aab --split-per-abi
pnpm app build ios --target aarch64-sim --no-sign --ci
```

## Architecture

Add a Node.js dispatcher under `packages/scripts/src/` and expose it as the root `app` package script. Node.js is already required by the pnpm workspace and provides consistent process spawning on macOS, Windows, and Linux without maintaining separate shell and PowerShell implementations.

The dispatcher converts the stable repository interface into native Tauri CLI arguments:

| Repository command | Tauri command |
| --- | --- |
| `app dev desktop` | `tauri dev` |
| `app build desktop` | `tauri build` |
| `app dev android` | `tauri android dev` |
| `app build android` | `tauri android build` |
| `app dev ios` | `tauri ios dev` |
| `app build ios` | `tauri ios build` |

Arguments after the platform are passed to Tauri unchanged. The dispatcher invokes the desktop workspace package directly and propagates its exit code or terminating signal.

## Platform Behavior

Desktop builds target the current host operating system. The script does not promise desktop cross-compilation: Windows packages are built on Windows, Linux packages on Linux, and macOS packages on macOS.

Android commands are available on hosts supported by the installed Android SDK and Tauri toolchain. A release build uses Tauri's default release mode. Callers select APK or AAB output, ABI targets, Android Studio, and CI behavior with normal Tauri options.

iOS commands are accepted only on macOS. A release build follows Tauri's normal signing behavior. The dispatcher does not manage Apple identities, provisioning profiles, export credentials, or secrets. Local simulator or archive checks can use `--no-sign` and the appropriate target.

## Validation and Errors

The dispatcher rejects:

- missing action or platform arguments;
- actions other than `dev` and `build`;
- platforms other than `desktop`, `android`, and `ios`;
- iOS commands on non-macOS hosts.

Invalid input prints concise usage text and exits non-zero. Child-process start failures and Tauri failures also exit non-zero. Successful commands preserve Tauri's normal output and artifact locations.

The unfinished local `run-tauri.mjs` override must not inject a false `macOSPrivateApi` value. Desktop macOS commands rely on the checked-in `tauri.macos.conf.json` and target-specific Cargo feature configuration so Tauri sees a consistent platform configuration.

## Release Boundaries

The command builds release artifacts but does not:

- bump or synchronize application versions;
- delete caches or previous artifacts;
- create signing identities or persist signing secrets;
- notarize without the caller's configured Apple credentials;
- upload artifacts or create a GitHub release.

These responsibilities remain in the existing release workflow and release helper scripts.

## Verification

Unit tests cover argument mapping, passthrough options, invalid commands, iOS host restrictions, and child-process exit handling where practical.

Implementation verification uses:

```bash
pnpm --filter @markra/scripts test
pnpm --filter @markra/scripts build
pnpm app build desktop --no-sign
pnpm app build android --apk --target aarch64 --ci
pnpm app build ios --target aarch64-sim --no-sign --ci
```

The desktop release build must produce the current host's application bundle and installer. Android verification must produce a release APK. iOS verification must produce an unsigned release simulator bundle or archive. Store-ready signing is reported separately because it depends on user-owned credentials.
