# QingYu S3 Zero-Length Upload Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Round-trip logical zero-byte files through S3-compatible gateways that reject physically empty `PUT` bodies.

**Architecture:** Encode a logical empty file as a signed one-byte sentinel object with QingYu metadata, validate and decode it during download, and leave every ordinary S3 object path unchanged.

**Tech Stack:** Rust, reqwest 0.12, AWS SigV4, Tauri v2, existing live S3 integration harness.

## Global Constraints

- Preserve all current S3 advanced-option changes in the dirty worktree.
- Do not write endpoint credentials into source, application configuration, logs, or commits.
- Do not change frontend configuration, sync-engine planning, or non-empty payload behavior.
- Live objects must remain under the isolated `markra-sync-tests/...` prefix and be cleaned after each run.

## Completed Tasks

- [x] Reproduce the 411 response and prove the raw request contains `Content-Length: 0`.
- [x] Test and reject HTTP/1-only transport as insufficient.
- [x] Test and reject an HTTP/1.1 chunked empty request as insufficient.
- [x] Add a failing raw-request regression test for the one-byte logical-empty representation.
- [x] Add `S3Payload::LogicalEmpty`, its sentinel, signed metadata, and physical content length.
- [x] Encode empty uploads and validate/decode marker-plus-sentinel downloads.
- [x] Add focused signing, upload-wire, and download-round-trip tests.
- [x] Pass the live topology scenario against the supplied S3 endpoint, including cleanup.

## Verification Results

- [x] All 32 focused S3 signer/backend unit tests pass.
- [x] The full non-ignored Rust suite ran: 805 passed and 2 unrelated builder-boundary tests failed because the existing `macos-private-api` worktree change has not updated its assertions; 22 live tests remain ignored by default.
- [x] The two changed Rust sources pass `rustfmt --check`, and their diff contains no HTTP/1-only or chunked-transfer experiment.
- [x] No supplied endpoint credential or temporary diagnostic test remains in repository content.
