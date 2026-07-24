# QingYu S3 Advanced Options Design

## Status

Approved for implementation on 2026-07-23. The user accepted the recommended
request-timeout and addressing controls and explicitly required an option to skip TLS
certificate verification for self-hosted S3 services without a trusted certificate.

QingYu is unreleased for this configuration boundary. Per the existing product decision,
this change does not migrate or read the superseded sync configuration schema.

## Goal

Add the S3 compatibility controls that users commonly need when connecting QingYu to
cloud and self-hosted S3-compatible services, without exposing a concurrency control that
the current serialized synchronization engine cannot honor safely.

## Configuration Schema

The S3 object in application-local `sync-config.json` gains three required fields:

```json
{
  "s3": {
    "endpointUrl": "https://s3.example.test",
    "region": "us-east-1",
    "bucket": "notes",
    "accessKeyId": "...",
    "secretAccessKey": "...",
    "requestTimeoutSeconds": 60,
    "addressingStyle": "auto",
    "tlsVerification": "verify"
  }
}
```

- `requestTimeoutSeconds`: integer from 5 through 600; default `60`.
- `addressingStyle`: `auto`, `path`, or `virtual-hosted`; default `auto`.
- `tlsVerification`: `verify` or `skip`; default `verify`.

The sync configuration schema version becomes version 2. Version 1 is unsupported rather
than migrated or silently defaulted.

These settings remain application-local beside the other S3 credentials. They are not
written to, exported into, or synchronized with the notebook directory.

## Addressing Semantics

`auto` preserves the current provider-aware behavior:

- already bucket-qualified endpoints remain unchanged;
- known virtual-hosted providers such as Aliyun OSS use `<bucket>.<endpoint>`;
- other S3-compatible endpoints such as ordinary MinIO deployments use
  `<endpoint>/<bucket>`.

`path` always builds `<endpoint>/<bucket>/<object-key>`. A bucket-qualified endpoint is
rejected in this mode instead of producing a double bucket scope.

`virtual-hosted` builds `<bucket>.<endpoint>/<object-key>`, while accepting an endpoint
that is already qualified with the configured bucket.

The selected addressing style participates in the local target fingerprint because it can
change the actual remote target URL. Timeout and TLS verification do not participate in
that fingerprint because they change transport policy rather than remote data identity.

## Timeout Semantics

The configured timeout applies independently to each S3 HTTP request, including connection
testing, notebook catalog requests, listing pages, HEAD requests, downloads, uploads, and
deletes. It is not a total deadline for the complete synchronization run.

A multi-file synchronization may therefore run longer than the configured timeout as long
as each individual request completes within the limit.

## TLS Verification Semantics

`verify` uses the normal platform and bundled trust roots through the existing reqwest and
rustls client.

`skip` configures both the normal S3 client and the bounded connection-test client to accept
invalid server certificates. It applies to connection testing, notebook discovery, and
actual synchronization. It has no additional effect for an `http://` endpoint.

Skipping verification is never selected automatically. The setting label must include a
concise unsafe marker so the risk remains visible without adding another large settings
callout. Credentials, request signatures, and note content can be intercepted when this
mode is used on an untrusted network.

## Settings Experience

Desktop and compact/mobile S3 settings show the same controls after the existing endpoint,
region, bucket, and credential fields:

- Request timeout: numeric seconds field, `5–600`.
- Addressing style: Automatic, Path-style, or Virtual-hosted-style.
- TLS certificate verification: Verify certificates (recommended) or Skip verification
  (unsafe).

Each field retains the existing save-on-change behavior. A changed transport option only
takes effect after the settings session ends or the user explicitly starts a connection
test or synchronization with the saved revision.

## Concurrency Boundary

No `concurrentRequests` setting is added in this change.

The current engine executes remote file actions serially and checkpoints the manifest after
each successful action. Bounded parallel transfer requires a separate design for action
ordering, cancellation, conflict publication, summary aggregation, and serialized manifest
commits. Exposing an input before that work would create a setting that does not reflect
runtime behavior.

## Failure and Safety Behavior

- Values outside the timeout range make the configuration incomplete and identify the
  exact field.
- Unknown enum values keep the strict configuration malformed/unsupported behavior.
- Invalid addressing combinations fail before a signed network request is sent.
- Connection-test and synchronization errors remain credential-safe.
- Redirects remain disabled for the connection-test client.
- TLS verification defaults to enabled after every reset.

## Verification

Implementation coverage must include:

- version-2 defaults, serialization, strict parsing, patching, and timeout validation;
- TypeScript model and patch behavior;
- desktop and compact/mobile settings controls and localized labels;
- automatic, path-style, virtual-hosted, and already-qualified URL construction;
- target-fingerprint separation by addressing style;
- timeout propagation to normal and connection-test clients;
- TLS verification enabled by default and disabled only for the explicit `skip` value;
- S3 connection test, catalog, and ordinary synchronization construction using the same
  transport options;
- existing MinIO live synchronization when the configured test server is available.
