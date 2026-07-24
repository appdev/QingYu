# S3 Automatic Region Design

## Status

Approved for direct implementation on `main` on 2026-07-24. The user selected
the empty-region design, waived further approval checkpoints, and requested
single-agent execution.

## Goal

Remove QingYu's fixed `us-east-1` S3 region default. A blank region remains
blank in `sync-config.json` and is interpreted as `auto` only when QingYu
constructs the S3 signing connection.

## Configuration and Validation

- `S3Config::default().region` is an empty string.
- An empty or whitespace-only region is valid and does not make an otherwise
  complete S3 configuration incomplete.
- Normalization continues to trim a non-empty region before it is saved.
- The configuration schema remains version 2; no migration is added.
- Existing explicit region values remain unchanged and continue to be used
  exactly as configured after trimming.

## Runtime Semantics

`S3Connection::new_with_addressing_style` resolves the signing region at its
existing construction boundary:

- empty or whitespace-only input becomes `auto`;
- non-empty input becomes its trimmed value.

The resolved value is stored on `S3Connection` and therefore participates in
the existing AWS Signature Version 4 credential scope and signing-key
derivation. No downstream sync, catalog, connection-test, or MCP call site
substitutes its own default.

## Settings Experience

The desktop region description states that leaving the field blank uses
`auto`. Both desktop and compact inputs display `auto` as placeholder text
while preserving an actual empty value. Incomplete-S3 guidance no longer lists
region among the fields that must be completed.

## Compatibility Boundary

There is no migration or heuristic for existing values. A correct explicit
region remains correct; an incorrect explicit region continues to fail at the
S3 service. Only blank input receives the `auto` runtime fallback.

## Verification

- Rust model tests prove the serialized default region is empty and an empty
  region can be ready when all required S3 fields are present.
- Rust S3 HTTP tests prove blank input constructs successfully, resolves to
  `auto`, and signs with an `/auto/s3/aws4_request` credential scope.
- Existing explicit-region signing tests continue to pass.
- Focused Rust tests run before and after implementation, followed by the
  repository's Rust, frontend test, typecheck, build, formatting, and diff
  checks.
