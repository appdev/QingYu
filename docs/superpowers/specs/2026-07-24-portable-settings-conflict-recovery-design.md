# Portable Settings Conflict Recovery Design

## Status

Approved by the user on 2026-07-24 after the installed QingYu 1.7.8 runtime
was shown to remain blocked by a stale `portable-settings-pending.json` journal.

## Problem

When portable application settings change while a remote settings transaction is
in the `Reconcile` phase, QingYu correctly refuses to overwrite the newer local
settings. However, it retains the active journal and deliberately returns
`settings-reconcile-failed` on every later attempt. The retained remote settings
are safe, but the active transaction permanently blocks notes and settings sync.

## Goals

- Preserve the newer local portable settings.
- Preserve the reconciled remote settings as a non-blocking conflict copy.
- Replace the stale active transaction with a fresh transaction based on the
  current local settings.
- Allow the next manual, interval, save, or launch sync to complete normally.
- Recover existing journals written by QingYu 1.7.8 without a migration step.

## Non-goals

- Do not merge two conflicting MCP/settings documents field by field.
- Do not change the remote S3 key layout or manifest format.
- Do not weaken validation, path, symlink, or atomic-write protections.
- Do not delete conflict evidence after recovery.

## Design

The existing settings engine already quarantines remote `settings.json`
conflicts under the settings scope's private `conflicts/` directory. Expose one
focused helper that publishes valid pending remote settings through that same
safe, no-overwrite path.

During `prepare_portable_settings_sync`, if a `Reconcile` journal's expected
portable revision no longer matches the current store and the journal does not
represent an already-applied commit:

1. Read the validated staged bytes from the journal.
2. Preserve those bytes as a timestamped remote conflict copy.
3. Fall through to the existing fresh-journal path instead of returning
   `settings-reconcile-failed`.
4. Stage the current local portable settings and continue the same sync run.

The remote settings remain recoverable, while the ordinary manifest comparison
treats the current local settings as the new local change and can upload them if
the remote object has not changed again.

## Verification

- Change the existing regression test so its second sync must succeed.
- Assert the current local portable value is retained.
- Assert the prior remote value exists in the private conflict directory.
- Assert the active pending journal is cleared after the successful retry.
- Run the focused test, the complete Rust suite, frontend tests, typecheck, and
  production build.
