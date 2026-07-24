# Sync Settings Groups and Defaults Design

## Status

Approved through the user's standing instruction to use the recommended option
without another confirmation step.

## Goal

Make the loaded synchronization settings easier to scan and reduce the number
of choices required when creating a configuration, without guessing any
service-specific endpoint, bucket, or credential.

## Presentation

Keep QingYu's existing flat settings-page visual language. Use the same small,
muted section headings and hairline row dividers as General settings; do not add
cards, accordions, new colours, or decorative containers.

Loaded synchronization settings are arranged in this order:

1. **Basic settings**: current notebook directory, cloud notebook selection,
   enabled state, provider, and remote root.
2. **Automatic sync**: sync after save and scheduled interval.
3. **S3 connection** or **WebDAV connection**: provider-specific endpoint and
   credentials. S3 region and bucket belong here.
4. **Advanced options**: S3 request timeout, addressing style, and TLS
   verification. WebDAV has no advanced rows today, so this section is omitted.
5. **Connection and status**: readiness, validation issues, failed writes,
   connection testing, manual synchronization, and the latest status.

Loading, absent, malformed, and unsupported states retain their current compact
single-section presentation.

## Defaults

A newly created or reset configuration remains disabled until the user enables
it, but starts with these useful values:

- provider: `s3`
- remote root: `qingyu`
- sync after saving: `true`
- automatic sync interval: `5` minutes
- S3 region: `us-east-1`
- S3 request timeout: `60` seconds
- S3 addressing style: `auto`
- S3 TLS verification: `verify`

The S3 endpoint, bucket, access key ID, and secret access key remain empty.
WebDAV URL and credentials remain empty. No default value may contain a real
account, host, bucket, or secret.

## Compatibility

Defaults apply only when QingYu creates or resets a configuration. Existing
saved configurations are not migrated or overwritten. The configuration
schema, patch API, validation rules, remote layout, and sync behavior remain
unchanged.

## Verification

- A component test asserts the five loaded-state headings and provider-specific
  grouping.
- A Rust unit test asserts every new default and the fields that must remain
  empty.
- Existing patching, optimistic updates, readiness, connection testing, and
  synchronization tests continue to pass.
- Run the focused React and Rust tests, full frontend tests, Rust tests,
  typecheck, and production build.
