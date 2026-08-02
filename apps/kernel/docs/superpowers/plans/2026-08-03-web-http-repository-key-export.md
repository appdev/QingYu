# Web HTTP Repository Key Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make confirmed global repository key export work on the deployed LAN HTTP Web client without leaking key material or changing secure/native clipboard semantics.

**Architecture:** `SyncSettings` selects the delivery mechanism at the presentation boundary. Explicit non-secure contexts use a one-shot Blob download; secure or unspecified contexts retain the existing clipboard operation and fail closed when it is unavailable.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, browser Blob/Object URL/download APIs, pnpm workspace.

## Global Constraints

- Exact baseline is `a12640de0e41d6c5ab3ff27428821827c683a97e` / tree `a62f768a6528a83f691339b3cc6ecc5f98810424` / version `2.5.1`.
- Work only in `codex/fix-web-http-key-export-a12640de`; do not move canonical `main`, merge, push, deploy, launch installed applications, log in to the live product, or write live S3/profile state.
- Require explicit confirmation before `exportGlobalKey({ confirmed: true })`.
- Never render or log the key, include it in errors, or silently download after a secure-context clipboard failure.
- Use `pnpm` for all JavaScript commands and preserve `pnpm-lock.yaml`.

---

### Task 1: Reproduce the browser security-context boundary

**Files:**
- Modify: `packages/app/src/components/settings/SyncSettings.test.tsx`

**Interfaces:**
- Consumes: `SyncSettings`, `AppSyncConfigRuntime.exportGlobalKey`, `window.confirm`, `window.isSecureContext`, `navigator.clipboard`, `URL.createObjectURL`, and `URL.revokeObjectURL`.
- Produces: behavioral tests for secure copy, HTTP download, cancellation, cleanup, and secret non-disclosure.

- [ ] **Step 1: Add a secure-context clipboard regression test**

Configure a key-present S3 runtime, set `window.isSecureContext` to `true`, confirm the product warning, and make `navigator.clipboard.writeText` observable. Click **Copy key** and assert one confirmed runtime export, one clipboard write with the fixture key, copied feedback, and no key text in `document.body`.

```tsx
const exportedKey = "test-repository-key-material";
const exportGlobalKey = vi.fn(async () => exportedKey);
const writeText = vi.fn(async () => undefined);
vi.stubGlobal("isSecureContext", true);
Object.defineProperty(navigator, "clipboard", {
  configurable: true,
  value: { writeText }
});
const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);

configureKeyExportRuntime(exportGlobalKey);
renderS3Settings();
fireEvent.click(await screen.findByRole("button", { name: "Copy key" }));

await waitFor(() => expect(writeText).toHaveBeenCalledWith(exportedKey));
expect(confirm).toHaveBeenCalledWith(
  "Copy the repository key to the clipboard? Anyone with this key can read the encrypted repository."
);
expect(exportGlobalKey).toHaveBeenCalledWith({ confirmed: true });
expect(await screen.findByRole("status")).toHaveTextContent("Repository key copied.");
expect(document.body).not.toHaveTextContent(exportedKey);
```

- [ ] **Step 2: Add a non-secure HTTP download regression test**

Set `window.isSecureContext` to `false`, configure deterministic Object URL methods, and intercept `HTMLAnchorElement.prototype.click`. Click the wished-for **Download key** action and assert the download-specific confirmation, a `text/plain;charset=utf-8` Blob containing the exact fixture, constant filename, immediate anchor removal, URL revocation, downloaded feedback, and no key text in the DOM.

```tsx
vi.stubGlobal("isSecureContext", false);
const createObjectURL = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:key-export");
const revokeObjectURL = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
let clickedLink: HTMLAnchorElement | null = null;
vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function () {
  clickedLink = this;
});

configureKeyExportRuntime(exportGlobalKey);
renderS3Settings();
fireEvent.click(await screen.findByRole("button", { name: "Download key" }));

await waitFor(() => expect(createObjectURL).toHaveBeenCalledOnce());
const blob = createObjectURL.mock.calls[0]?.[0] as Blob;
expect(blob.type).toBe("text/plain;charset=utf-8");
await expect(blob.text()).resolves.toBe(exportedKey);
expect(clickedLink).toMatchObject({ download: "qingyu-repository-key.txt", href: "blob:key-export" });
expect(clickedLink?.isConnected).toBe(false);
expect(revokeObjectURL).toHaveBeenCalledWith("blob:key-export");
expect(document.body).not.toHaveTextContent(exportedKey);
```

- [ ] **Step 3: Add negative security tests**

Cover confirmation cancellation for both branches, secure clipboard rejection, and a throwing download click. Assert cancellation performs no runtime export or delivery, secure failure does not create an Object URL, download failure still revokes/removes, feedback stays generic, and neither errors nor `console` calls contain the fixture key.

```tsx
vi.spyOn(window, "confirm").mockReturnValue(false);
fireEvent.click(await screen.findByRole("button", { name: expectedAction }));
expect(exportGlobalKey).not.toHaveBeenCalled();
expect(deliverySpy).not.toHaveBeenCalled();

writeText.mockRejectedValueOnce(new DOMException("Clipboard denied", "NotAllowedError"));
fireEvent.click(screen.getByRole("button", { name: "Copy key" }));
expect(await screen.findByRole("status")).toHaveTextContent("The operation could not be started.");
expect(URL.createObjectURL).not.toHaveBeenCalled();
expect(document.body).not.toHaveTextContent(exportedKey);

click.mockImplementationOnce(() => {
  throw new Error("download blocked");
});
fireEvent.click(screen.getByRole("button", { name: "Download key" }));
await waitFor(() => expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:key-export"));
expect(document.body.querySelector('a[download="qingyu-repository-key.txt"]')).toBeNull();
expect(document.body).not.toHaveTextContent(exportedKey);
expect(consoleError.mock.calls.flat().join(" ")).not.toContain(exportedKey);
```

- [ ] **Step 4: Run RED and record the expected failures**

Run:

```bash
pnpm --dir packages/app exec vitest run src/components/settings/SyncSettings.test.tsx
```

Expected: the non-secure cases fail because the button still says **Copy key** and the implementation unconditionally accesses the Clipboard API; the secure compatibility case may pass and is retained as a regression boundary.

---

### Task 2: Implement the minimal confirmed export delivery

**Files:**
- Modify: `packages/app/src/components/settings/SyncSettings.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`

**Interfaces:**
- Consumes: the existing `AppSyncConfigRuntime.exportGlobalKey({ confirmed: true })` string result.
- Produces: context-specific action label, confirmation, delivery, and success feedback; an internal constant-filename one-shot download helper.

- [ ] **Step 1: Add download-specific localized strings**

Add translation keys for the action, confirmation, and success feedback. English must explicitly warn that the plaintext file grants repository access; Chinese must carry the same warning. Do not interpolate the key.

```ts
"settings.sync.key.download": "Download key",
"settings.sync.key.downloadConfirm":
  "Download the repository key as a plaintext file? Anyone with this file can read the encrypted repository. Store it securely.",
"settings.sync.key.downloaded": "Repository key downloaded."
```

- [ ] **Step 2: Add the one-shot sensitive-text download helper**

Create a `text/plain;charset=utf-8` Blob, assign its Object URL to a hidden `a` with `download="qingyu-repository-key.txt"`, `rel="noopener"`, and no text content, append/click it, then remove it and revoke the URL in `finally`.

```ts
function downloadRepositoryKey(key: string) {
  const blob = new Blob([key], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = "qingyu-repository-key.txt";
  link.rel = "noopener";
  link.style.display = "none";
  try {
    document.body.appendChild(link);
    link.click();
  } finally {
    link.remove();
    URL.revokeObjectURL(url);
  }
}
```

- [ ] **Step 3: Branch only on explicit non-secure context**

Derive the branch from `window.isSecureContext === false`. Select the matching label and confirmation before reading the key. After confirmed export, call the download helper for non-secure HTTP; otherwise require and await `navigator.clipboard.writeText`. Preserve the existing generic catch behavior.

```ts
const downloadsRepositoryKey = window.isSecureContext === false;
const actionKey = downloadsRepositoryKey
  ? "settings.sync.key.download"
  : "settings.sync.key.export";
const confirmationKey = downloadsRepositoryKey
  ? "settings.sync.key.downloadConfirm"
  : "settings.sync.key.exportConfirm";

if (!window.confirm(translate(confirmationKey))) return;
const key = await getAppRuntime().syncConfig.exportGlobalKey({ confirmed: true });
if (downloadsRepositoryKey) downloadRepositoryKey(key);
else await navigator.clipboard.writeText(key);
```

- [ ] **Step 4: Run GREEN**

Run:

```bash
pnpm --dir packages/app exec vitest run src/components/settings/SyncSettings.test.tsx
```

Expected: all SyncSettings tests pass, including the new security-context matrix.

- [ ] **Step 5: Run focused cross-package regression**

Run:

```bash
pnpm --dir packages/app exec tsc -p tsconfig.test.json --noEmit
pnpm --dir packages/app exec tsc -p tsconfig.build.json --noEmit
```

Expected: both commands exit 0.

---

### Task 3: Commit, review, and complete verification

**Files:**
- Inspect: exact `a12640de0e41d6c5ab3ff27428821827c683a97e..HEAD` diff
- Verify: all files changed by Tasks 1–2 plus this design and plan

**Interfaces:**
- Consumes: the completed TDD change.
- Produces: one isolated fix commit, full workspace evidence, and an independent review report.

- [ ] **Step 1: Inspect and commit only the scoped files**

Run `git diff --check`, inspect `git status --short` and `git diff`, then create one commit named `fix(sync): export repository key over HTTP` without staging unrelated files.

- [ ] **Step 2: Run the full JavaScript gate on the committed tree**

Run sequentially:

```bash
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: every command exits 0 with no failed workspace test.

- [ ] **Step 3: Request independent read-only review**

Give the reviewer the exact baseline and fix SHAs, the incident evidence, this design, and the security requirements. Require findings classified as Critical, Important, or Minor and require confirmation that the key never enters DOM/log/error content and secure/native behavior does not silently download.

- [ ] **Step 4: Address review findings with TDD**

For every Critical or Important finding, add or adjust a failing test first, run it to RED, apply the smallest correction, and rerun the focused suite. Keep review-driven commits scoped to this branch.

- [ ] **Step 5: Run final fresh verification**

After the final commit, rerun the focused SyncSettings suite, `pnpm test`, `pnpm typecheck:test`, `pnpm build`, `git diff --check`, and confirm the worktree is clean. Record exact counts, commit, tree, and changed files for the event callback.
