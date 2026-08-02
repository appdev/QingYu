# Web HTTP Repository Key Export Design

## Problem

QingYu currently exports the global Dejavu repository key by obtaining the key from the Kernel-backed runtime and unconditionally calling `navigator.clipboard.writeText`. The deployed Web client is intentionally reachable on a LAN HTTP origin. That origin is not a secure context, so Chrome does not provide a usable asynchronous Clipboard API. The product confirmation succeeds, the Kernel returns the key, the clipboard write fails, and the UI collapses the failure to the generic operation error.

This blocks the supported recovery flow: the Server key cannot be transferred to macOS and Android, so the three clients cannot authenticate the same encrypted notes repository.

## Constraints

- Keep the explicit product confirmation before retrieving or releasing the key.
- Never render the key in the DOM, include it in feedback, log it, or attach it to an error.
- Do not broaden the Kernel key-export route or add a new unauthenticated/download endpoint.
- Preserve the current secure-context/native behavior: copy to the clipboard and fail closed if that copy is rejected, even if a native WebView reports `isSecureContext === false`.
- Support an HTTP Web deployment without requiring TLS or changing browser permissions.
- Keep the change in the shared settings surface so Web and native behavior remain covered by one contract.

## Considered Approaches

### 1. Explicit HTTP download fallback (selected)

When the desktop Web runtime reports `window.isSecureContext === false`, the settings UI labels the action **Download key**, uses a download-specific confirmation warning, and releases the exported key as a one-shot `text/plain` Blob named `qingyu-repository-key.txt`. A hidden anchor receives only the Blob URL, is clicked synchronously from the confirmed user action flow, and is removed immediately; the Blob URL is always revoked, including if anchor removal throws. The key is never element text or an attribute value.

When the context is secure, the existing **Copy key** flow remains. A missing or rejected Clipboard API is a terminal operation failure; the application does not silently create a plaintext file on native or HTTPS clients.

This is the smallest compatible repair because it changes only the presentation boundary that depends on browser security context. It reuses the existing authenticated, confirmed Kernel export call and does not alter the credential transport surface.

### 2. Add a cross-platform sensitive-export runtime capability

The shared UI could delegate delivery to a new runtime method, with Web downloading and native copying through platform code. This would make the platform boundary explicit, but it would require changes to every runtime composition and test double, plus a native clipboard capability that does not currently exist in the shared contract. That scope is unnecessary for this incident.

### 3. Add a Server attachment endpoint

The Kernel could return the key with `Content-Disposition: attachment`. This would work on HTTP, but it would add another secret-bearing HTTP route, OpenAPI/client surface, and browser navigation/download contract. It materially expands the credential attack surface and is rejected.

## Detailed Behavior

### Secure context

1. Render **Copy key**.
2. Ask the existing clipboard warning confirmation.
3. On cancellation, do not call the runtime and do not touch the clipboard.
4. On acceptance, request the key with `{ confirmed: true }`, then call `navigator.clipboard.writeText` exactly once.
5. Report copied only after the write resolves. Any runtime or clipboard error reports the existing generic failure without secret material.

### Non-secure desktop Web context

1. Render **Download key**.
2. Ask a warning that the downloaded plaintext file grants repository access and should be stored securely.
3. On cancellation, do not call the runtime and do not create a Blob or anchor.
4. On acceptance, request the key with `{ confirmed: true }`, create the one-shot download, and report downloaded only after the trigger completes.
5. Always remove the anchor and revoke the Blob URL, including when DOM insertion or click throws.

`isSecureContext` is treated as a download signal only when it is explicitly `false` in the desktop Web runtime (`nativeWindowChrome === false` and desktop form factor). Native desktop and mobile runtimes retain clipboard delivery even if their WebView reports an explicit `false`; an absent secure-context capability also remains on the fail-closed clipboard path. This makes the deployed HTTP boundary deterministic without silently changing native secret delivery.

## Security Properties

- No plaintext key is placed in visible or hidden DOM text, form values, logs, feedback, thrown messages, URLs, query strings, or filenames.
- The download filename is constant and non-secret.
- Confirmation occurs before the Kernel export call.
- A cancelled action has zero credential reads and zero clipboard/download side effects.
- Secure-context clipboard failure remains fail closed and never becomes an unexpected plaintext download.
- Non-secure download cleanup uses nested `finally` blocks so the Blob URL lifetime remains bounded even if anchor removal itself fails.

## Verification

- Component RED/GREEN tests cover secure copy, non-secure Web download, native WebView and unspecified-context clipboard preservation, both confirmation messages, confirmation-before-export ordering, cancellation, clipboard rejection, download cleanup failures, and secret absence from DOM/feedback/log calls.
- The existing SyncSettings suite remains green.
- Workspace `pnpm test`, `pnpm typecheck:test`, and `pnpm build` must pass.
- No Rust behavior changes, so no Rust gate is required for this TypeScript-only repair.
- An independent read-only reviewer must inspect the exact baseline-to-fix diff for correctness, browser compatibility, and secret handling before integration.
