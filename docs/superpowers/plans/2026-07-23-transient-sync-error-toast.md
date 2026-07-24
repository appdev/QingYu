# Transient Sync Error Toast Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task with review checkpoints.

**Goal:** Replace the current technical, persistent sync failure toast with the approved compact single-line prompt that disappears after 3 seconds, has no close affordance, and offers an inline retry action.

**Architecture:** Keep Sonner as the shared renderer and add a narrow `sync-error` presentation contract to the existing app-toast adapter. The sync coordinator owns retry behavior and user-facing copy, while the adapter owns duration, dismissibility, icon, and layout classes. The existing `app-sync` id continues to update one toast in place, and structured diagnostics remain in `appLogger` only.

**Tech Stack:** React 19, TypeScript, Sonner 2, Lucide React, Tailwind CSS 4, Vitest, Testing Library.

**Global Constraints:** Do not change other toast defaults, sync protocol semantics, sync scheduling, or persisted diagnostics. Do not expose safe error fields in the UI. Do not touch the unrelated untracked `macos-icon.icns` file. Do not use the TypeScript `void` operator.

---

## Task 1: Define the dedicated toast presentation contract

**Files:**
- Modify: `packages/app/src/lib/app-toast.ts`
- Modify: `packages/app/src/lib/app-toast.test.ts`

### Step 1: Write the failing adapter test

Add a test that calls:

```ts
showAppToast({
  action: { label: "Retry", onClick: vi.fn() },
  id: "app-sync",
  message: "Sync did not complete",
  presentation: "sync-error",
  status: "error"
});
```

Assert that `toast.error` receives:

```ts
expect.objectContaining({
  closeButton: false,
  dismissible: false,
  duration: 3000,
  id: "app-sync"
})
```

Also assert that the option set includes the `app-toast-sync-error` marker, compact fixed-width Tailwind classes, per-toast title/action/icon classes, and no `description`.

### Step 2: Run the focused test and confirm RED

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/app-toast.test.ts
```

Expected: FAIL because `presentation` is not accepted and the sync-specific options are absent.

### Step 3: Implement the minimal presentation mapping

Extend the app-toast input with:

```ts
export type AppToastPresentation = "default" | "sync-error";
```

For `sync-error`, add:

```ts
{
  closeButton: false,
  dismissible: false,
  duration: 3000,
  className: "app-toast-sync-error ...",
  classNames: {
    actionButton: "...",
    content: "...",
    icon: "...",
    title: "..."
  }
}
```

Use a 16px Lucide `CircleAlert` icon only for the error state. Keep the loading state on Sonner's existing spinner. Make presentation options additive so every existing caller retains its current defaults.

### Step 4: Run the focused test and confirm GREEN

Run the same Vitest command. Expected: PASS.

### Step 5: Commit

```bash
git add packages/app/src/lib/app-toast.ts packages/app/src/lib/app-toast.test.ts
git commit -m "feat: add transient sync toast presentation"
```

## Task 2: Add dedicated localized copy and retry behavior

**Files:**
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/index.test.ts`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.test.tsx`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.ts`

### Step 1: Write failing locale and coordinator tests

Add dedicated keys and expectations for:

```ts
"settings.sync.toastIncomplete"
"settings.sync.toastRetry"
"settings.sync.toastRetrying"
```

Expected English copy:

```text
Sync did not complete
Retry
Syncing…
```

Expected Simplified Chinese copy:

```text
同步未完成
重试
正在同步…
```

Update the coordinator failure tests to require this exact call shape:

```ts
expect.objectContaining({
  action: expect.objectContaining({ label: "settings.sync.toastRetry" }),
  id: "app-sync",
  message: "settings.sync.toastIncomplete",
  presentation: "sync-error",
  status: "error"
})
```

Explicitly assert that `description` is absent. Keep the existing logger assertion to prove structured diagnostics still reach the runtime log.

Add a retry test that invokes the toast action and asserts:

1. `preventDefault()` is called so Sonner does not remove the toast before it is updated.
2. A loading toast replaces `app-sync` with `settings.sync.toastRetrying`.
3. `runApplicationSync` is called again with `trigger: "manual"` and the current root/revision.
4. A successful retry dismisses `app-sync`.
5. A failed retry restores the same one-shot error toast instead of stacking another id.

### Step 2: Run focused tests and confirm RED

Run:

```bash
pnpm --filter @markra/shared exec vitest run src/i18n/index.test.ts
pnpm --filter @markra/app exec vitest run src/hooks/useAppSyncCoordinator.test.tsx
```

Expected: FAIL because the keys, presentation, safe copy, and retry action are not implemented.

### Step 3: Implement the localized retry flow

Add a coordinator-local `showSyncFailureToast()` callback that:

```ts
showAppToast({
  action: {
    label: translateRef.current("settings.sync.toastRetry"),
    onClick: (event) => {
      event.preventDefault();
      // Validate current readiness, replace the same id with loading,
      // call runDetailedRef.current("manual"), and dismiss on success.
    }
  },
  id: "app-sync",
  message: translateRef.current("settings.sync.toastIncomplete"),
  presentation: "sync-error",
  status: "error"
});
```

Replace all sync coordinator failure descriptions with this callback, including membership and listener-registration failures. Remove `safeErrorDescription`; preserve the structured `appLogger.error` call. Read the active root and configuration from refs at click time so the retry never targets the stale failed request.

### Step 4: Run focused tests and confirm GREEN

Run the two focused commands again. Expected: PASS.

### Step 5: Commit

```bash
git add packages/shared/src/i18n/locales/types.ts packages/shared/src/i18n/locales/en.ts packages/shared/src/i18n/locales/zh-CN.ts packages/shared/src/i18n/index.test.ts packages/app/src/hooks/useAppSyncCoordinator.ts packages/app/src/hooks/useAppSyncCoordinator.test.tsx
git commit -m "feat: make sync failure toast retryable"
```

## Task 3: Verify rendered behavior and protect other toast styles

**Files:**
- Modify: `packages/app/src/App.test.tsx`

### Step 1: Write the failing rendered regression test

Extend the existing toast rendering coverage with a `sync-error` call and assert:

```ts
expect(toast).toHaveClass("app-toast-sync-error");
expect(toast).toHaveClass("w-[min(20rem,calc(100vw-1.5rem))]!");
expect(toast).toHaveAttribute("data-dismissible", "false");
expect(toast.querySelector("[data-close-button]")).not.toBeInTheDocument();
expect(toast.querySelector("[data-action]")).toHaveTextContent("Retry");
expect(toast.querySelector("[data-description]")).not.toBeInTheDocument();
```

Keep the adjacent generic long-error assertions unchanged so the test proves the new presentation does not alter other error toasts.

### Step 2: Run the rendered test and confirm RED

Run:

```bash
pnpm --filter @markra/app exec vitest run src/App.test.tsx -t "renders the transient sync failure toast"
```

Expected: FAIL until the test calls the new presentation and the adapter options render as specified.

### Step 3: Complete the rendered expectation

Call `showAppToast` with the dedicated presentation from the existing `AppToaster` test harness. If the focused test exposes a class conflict, adjust only the presentation's Tailwind important utilities; do not change the shared toaster defaults.

### Step 4: Run focused and full verification

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/app-toast.test.ts src/hooks/useAppSyncCoordinator.test.tsx src/App.test.tsx
pnpm --filter @markra/shared exec vitest run src/i18n/index.test.ts
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
git status --short
```

Expected: all commands pass; only the planned files plus the pre-existing untracked `macos-icon.icns` appear.

### Step 5: Commit

```bash
git add packages/app/src/App.test.tsx
git commit -m "test: cover transient sync failure toast"
```

## Plan Self-Review

- Design coverage: compact one-line layout, 320px responsive width, semantic error icon, no close control, 3-second auto-dismiss, inline retry, in-place loading, silent success, and same-id failure restoration are all mapped to implementation and tests.
- Scope coverage: only the sync coordinator opts into `sync-error`; ordinary success, loading, error, and notice toasts retain their current options.
- Safety coverage: current refs determine retry scope; technical fields stay in logging; existing sync run serialization and deduplication remain untouched.
- Accessibility coverage: Sonner keeps the polite live region, the action preserves a 44px hit target and focus ring, and motion classes include reduced-motion fallbacks.
- Verification coverage: adapter contract, localized copy, coordinator state transitions, rendered DOM, full tests, typecheck, build, and diff hygiene are included.
- Placeholder scan: no TODO, TBD, FIXME, ellipsis placeholder, or unresolved design choice remains.
