# QingYu S3 Zero-Length Upload Compatibility Design

## Problem

The live S3 endpoint accepts ordinary non-empty `PUT` requests but rejects a zero-byte object upload with HTTP 411 and `MissingContentLength`. QingYu sends `Content-Length: 0` correctly. The endpoint also rejects an HTTP/1.1 chunked zero-byte upload, so neither protocol negotiation nor a missing client header is the root cause. This is a provider/gateway incompatibility with physically empty request bodies.

## Decision

Represent a QingYu logical empty file as a one-byte S3 object containing `0x00` and the metadata header `x-amz-meta-qingyu-logical-empty: 1`. Include the metadata header in SigV4 `SignedHeaders`. On download, decode the object to an empty byte vector only when both the marker and exact one-byte sentinel are present.

Ordinary zero-byte S3 objects without the marker continue to download as empty files. Ordinary one-byte files, including a single `0x00` byte, remain unchanged because they do not carry the marker. A marked object with unexpected bytes is rejected as an integrity failure instead of being silently truncated.

## Code Boundary

- `apps/desktop/src-tauri/src/s3_http.rs` owns the sentinel, metadata name, payload hash, content length, and SigV4 signed-header representation.
- `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs` selects the logical-empty representation on upload and validates/decodes it on download.
- The sync engine, frontend configuration, diagnostics, addressing modes, timeouts, TLS policy, and normal non-empty uploads remain unchanged.

## Rejected Approaches

- `Content-Length: 0`: already emitted on the wire; the endpoint still returns 411.
- Force HTTP/1.1: the endpoint still returns 411 with the explicit zero length.
- HTTP/1.1 chunked empty body: the request contains valid chunk framing, but the endpoint still returns 411 `MissingContentLength`.
- Retry every 411: it would repeat requests without changing the incompatible physical body representation.

## Verification

1. A raw TCP fixture requires `Content-Length: 1`, the signed logical-empty marker, and no chunked transfer for an empty-file upload.
2. A download fixture requires marker-plus-sentinel decoding back to zero bytes.
3. SigV4 tests require the marker in both the request headers and `SignedHeaders`.
4. The live topology scenario must round-trip empty, Unicode/reserved-path, binary, and ordinary files, then perform a no-op sync and clean its isolated prefix.

## Safety

Live tests use an isolated `markra-sync-tests/...` prefix and clean it after completion. Credentials stay in process environment variables and are never written to source, application configuration, or Git history.
