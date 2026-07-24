# Settings Cloud Notebook Dialog Ownership Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Settings visible while its cloud notebook dialog is open, while leaving notebook restoration under the primary editor's existing transaction coordinator.

**Architecture:** Replace the settings-to-primary “open a dialog, then hide Settings” handoff with a correlated request/response event used only for the selected notebook operation. Settings owns catalog loading and renders the existing `RemoteNotebookDialog`; the primary desktop workspace owner validates the revision and performs the existing sync-or-restore transaction.

**Tech Stack:** React 19, TypeScript 6, Vitest, Testing Library, Tauri v2 events, Rust command registration tests, pnpm workspace.

## Global Constraints

- Preserve all pre-existing uncommitted files and stage only the exact files named by each task.
- Use `pnpm` for JavaScript and frontend workflows.
- Do not use the TypeScript `void` keyword or operator.
- Reuse `RemoteNotebookDialog`; do not change catalog layout, copy, or styling.
- Do not change Welcome restore, true-mobile selection, provider catalog semantics, bootstrap synchronization, conflict handling, or ordinary Settings close behavior.
- Settings must remain mounted and visible while the dialog is loading, retrying, cancelling, failing, and succeeding.
- Only the primary desktop workspace owner may execute a notebook restore or current-notebook sync request.
- Cross-window results must not expose raw provider errors, credentials, or filesystem internals.

---

## File Map

- Create `packages/app/src/lib/cloud-notebook-restore-events.ts`: validated correlated request/response contract for a settings-owned dialog.
- Create `packages/app/src/lib/cloud-notebook-restore-events.test.ts`: contract, correlation, safe failure, and timeout coverage.
- Create `packages/app/src/hooks/useSettingsRemoteNotebookDialog.ts`: Settings-owned catalog and dialog lifecycle.
- Create `packages/app/src/hooks/useSettingsRemoteNotebookDialog.test.tsx`: catalog, cancel, failure, and successful selection coverage.
- Modify `packages/app/src/App.tsx`: listen for restore requests only in the primary desktop owner and call the existing coordinator.
- Modify `packages/app/src/App.test.tsx`: replace obsolete catalog-handoff cases with headless selection request coverage.
- Modify `packages/app/src/hooks/useSettingsWindowState.ts`: compose the new dialog controller and remove explicit Settings hiding.
- Modify `packages/app/src/hooks/useSettingsWindowState.test.ts`: retain Sync session lifecycle tests without asserting the obsolete hide handoff.
- Modify `packages/app/src/components/SettingsWindow.tsx`: render `RemoteNotebookDialog` over Settings.
- Modify `packages/app/src/components/SettingsWindow.test.tsx`: prove Settings remains mounted and visible.
- Delete `packages/app/src/lib/cloud-notebook-catalog-events.ts` and its test after all consumers move.
- Modify runtime/native adapter files listed in Task 4 to remove the now-unused targeted catalog command.

---

### Task 1: Add the correlated cloud notebook selection contract

**Files:**
- Create: `packages/app/src/lib/cloud-notebook-restore-events.ts`
- Create: `packages/app/src/lib/cloud-notebook-restore-events.test.ts`

**Interfaces:**
- Consumes: `AppEventsRuntime` through `getAppRuntime().events`.
- Produces: `PrimaryCloudNotebookRestoreRequest`, `requestPrimaryCloudNotebookRestore(input)`, and `listenPrimaryCloudNotebookRestoreRequested(handler)`.

- [ ] **Step 1: Write failing contract tests**

Create tests with an in-memory event bus that cover all of these exact behaviors:

```ts
function validRequest(
  overrides: Partial<PrimaryCloudNotebookRestoreRequest> = {}
): PrimaryCloudNotebookRestoreRequest {
  return {
    remoteName: "Archive",
    requestId: "request-1",
    revision: "rev-2",
    ...overrides
  };
}

function createEventBus() {
  const listeners = new Map<string, Set<(event: { payload: unknown }) => unknown>>();
  const emitted: Array<{ event: string; payload: unknown }> = [];
  const events = {
    isAvailable: () => true,
    emit: async (event: string, payload: unknown) => {
      emitted.push({ event, payload });
      for (const listener of listeners.get(event) ?? []) {
        await listener({ payload });
      }
    },
    listen: async (event: string, listener: (event: { payload: unknown }) => unknown) => {
      const registered = listeners.get(event) ?? new Set();
      registered.add(listener);
      listeners.set(event, registered);
      return () => registered.delete(listener);
    }
  };
  return {
    events,
    emit: events.emit,
    latestPayload: (event: string) => emitted.findLast((item) => item.event === event)?.payload,
    listenerCount: (event: string) => listeners.get(event)?.size ?? 0
  };
}

it("correlates one successful restore response and ignores unrelated completions", async () => {
  const bus = createEventBus();
  configureAppRuntime({ ...getAppRuntime(), events: bus.events });
  const pending = requestPrimaryCloudNotebookRestore({
    remoteName: "Archive",
    revision: "rev-2",
    timeoutMs: 1_000
  });
  await waitFor(() => expect(
    bus.latestPayload(primaryCloudNotebookRestoreRequestedEvent)
  ).toBeDefined());
  const request = bus.latestPayload(
    primaryCloudNotebookRestoreRequestedEvent
  ) as PrimaryCloudNotebookRestoreRequest;
  await bus.emit(primaryCloudNotebookRestoreCompletedEvent, {
    requestId: "unrelated",
    succeeded: true
  });
  await bus.emit(primaryCloudNotebookRestoreCompletedEvent, {
    requestId: request.requestId,
    succeeded: true
  });
  await expect(pending).resolves.toBe(true);
});

it("publishes a safe failed completion when the primary handler rejects", async () => {
  const bus = createEventBus();
  configureAppRuntime({ ...getAppRuntime(), events: bus.events });
  const request = validRequest();
  await listenPrimaryCloudNotebookRestoreRequested(async () => {
    throw new Error("provider-secret-detail");
  });
  await bus.emit(primaryCloudNotebookRestoreRequestedEvent, request);
  expect(bus.latestPayload(primaryCloudNotebookRestoreCompletedEvent)).toEqual({
    requestId: request.requestId,
    succeeded: false
  });
  expect(JSON.stringify(
    bus.latestPayload(primaryCloudNotebookRestoreCompletedEvent)
  )).not.toContain("provider-secret-detail");
});

it("settles false when no primary owner responds before the supplied timeout", async () => {
  await expect(requestPrimaryCloudNotebookRestore({
    remoteName: "Archive",
    revision: "rev-2",
    timeoutMs: 1
  })).resolves.toBe(false);
});

it("cleans up and settles false when its caller aborts", async () => {
  const bus = createEventBus();
  configureAppRuntime({ ...getAppRuntime(), events: bus.events });
  const abortController = new AbortController();
  const pending = requestPrimaryCloudNotebookRestore({
    remoteName: "Archive",
    revision: "rev-2",
    signal: abortController.signal,
    timeoutMs: 1_000
  });
  abortController.abort();
  await expect(pending).resolves.toBe(false);
  expect(bus.listenerCount(primaryCloudNotebookRestoreCompletedEvent)).toBe(0);
});
```

Also reject malformed request payloads with empty identifiers, empty revisions, empty names, or embedded NUL characters before calling the primary handler.

- [ ] **Step 2: Run the new test to verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/cloud-notebook-restore-events.test.ts
```

Expected: FAIL because `cloud-notebook-restore-events.ts` and its exports do not exist.

- [ ] **Step 3: Implement the minimal event contract**

Create these exact public shapes:

```ts
export type PrimaryCloudNotebookRestoreRequest = {
  remoteName: string;
  requestId: string;
  revision: string;
};

export type PrimaryCloudNotebookRestoreCompletion = {
  requestId: string;
  succeeded: boolean;
};

export type PrimaryCloudNotebookRestoreInput = {
  remoteName: string;
  revision: string;
  signal?: AbortSignal;
  timeoutMs?: number;
};

export const primaryCloudNotebookRestoreRequestedEvent =
  "qingyu://cloud-notebook-restore-requested";
export const primaryCloudNotebookRestoreCompletedEvent =
  "qingyu://cloud-notebook-restore-completed";

export function requestPrimaryCloudNotebookRestore(
  input: PrimaryCloudNotebookRestoreInput
): Promise<boolean>;

export function listenPrimaryCloudNotebookRestoreRequested(
  onRequested: (request: PrimaryCloudNotebookRestoreRequest) => boolean | Promise<boolean>
): Promise<() => unknown>;
```

Use a 30-minute production default timeout so a large bootstrap restore is not cut off by a UI-scale timeout. Register the completion listener before emitting the request. Match only the generated `requestId`, resolve `false` on abort, timeout, or delivery failure, and always clear the abort listener, timer, and event listener exactly once. The primary listener must normalize the payload and emit only `{ requestId, succeeded }`; catch handler failures and publish `succeeded: false`.

- [ ] **Step 4: Run the contract tests to verify GREEN**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/cloud-notebook-restore-events.test.ts
```

Expected: PASS with the success, malformed-input, handler-failure, correlation, and timeout cases all green.

- [ ] **Step 5: Commit the contract**

```bash
git add packages/app/src/lib/cloud-notebook-restore-events.ts packages/app/src/lib/cloud-notebook-restore-events.test.ts
git commit -m "feat: add cloud notebook restore event contract"
```

---

### Task 2: Make the primary editor a headless selection executor

**Files:**
- Modify: `packages/app/src/App.tsx:840-1110`
- Modify: `packages/app/src/App.test.tsx:1246-1640`

**Interfaces:**
- Consumes: `listenPrimaryCloudNotebookRestoreRequested(handler)` from Task 1.
- Produces: one primary-owner listener that returns `true` only after current-notebook sync or remote restore completes.

- [ ] **Step 1: Replace obsolete primary catalog event tests with failing headless selection tests**

Add focused App tests for these cases:

```ts
it("executes a settings-owned remote selection without rendering a main-window dialog", async () => {
  const restoring = requestPrimaryCloudNotebookRestore({
    remoteName: "B",
    revision: "established-catalog-revision",
    timeoutMs: 1_000
  });

  await expect(restoring).resolves.toBe(true);
  expect(restoreDesktopNotebook).toHaveBeenCalledWith("B", "/Workspace");
  expect(screen.queryByRole("dialog", { name: "Restore notebook from cloud" }))
    .not.toBeInTheDocument();
});

it("runs current-notebook synchronization for a same-name settings selection", async () => {
  const selectingCurrent = requestPrimaryCloudNotebookRestore({
    remoteName: "A",
    revision: "established-catalog-revision",
    timeoutMs: 1_000
  });

  await expect(selectingCurrent).resolves.toBe(true);
  expect(run).toHaveBeenCalledWith("manual", "established-catalog-revision");
  expect(restoreDesktopNotebook).not.toHaveBeenCalled();
});

it("rejects a stale settings selection without switching notebooks", async () => {
  await expect(requestPrimaryCloudNotebookRestore({
    remoteName: "B",
    revision: "stale-revision",
    timeoutMs: 1_000
  })).resolves.toBe(false);
  expect(restoreDesktopNotebook).not.toHaveBeenCalled();
});
```

Retain the existing tests that open `RemoteNotebookDialog` from Welcome and other main-window entry points. Replace only the settings-specific tests that emit `primaryCloudNotebookCatalogRequestedEvent`, queue `establishedCatalogRequestPending`, or assert a catalog subscription in external windows.

- [ ] **Step 2: Run the focused App cases to verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/App.test.tsx -t "settings-owned|same-name settings selection|stale settings selection"
```

Expected: FAIL because WorkspaceApp does not listen for Task 1's request event.

- [ ] **Step 3: Add the primary-owner listener**

Import Task 1's listener and add a callback after `notebookSwitch`, `currentDesktopNotebookName`, and `appSync` are available:

```ts
const handlePrimaryCloudNotebookRestoreRequest = useCallback(async ({
  remoteName,
  revision
}: PrimaryCloudNotebookRestoreRequest) => {
  if (
    compactMode.trueMobile ||
    !primaryWindowOwner ||
    remoteNotebookDialogOpen ||
    syncConfig.appliedDocument?.revision !== revision
  ) return false;

  if (remoteName === currentDesktopNotebookName) {
    return Boolean(await appSync.run("manual", revision));
  }

  const restoredRoot = await notebookSwitch.restoreDesktopNotebook(
    remoteName,
    primaryWorkspace.workspaceRoot ?? undefined
  );
  return restoredRoot !== null;
}, [
  appSync.run,
  compactMode.trueMobile,
  currentDesktopNotebookName,
  notebookSwitch.restoreDesktopNotebook,
  primaryWindowOwner,
  primaryWorkspace.workspaceRoot,
  remoteNotebookDialogOpen,
  syncConfig.appliedDocument?.revision
]);
```

Subscribe only when `primaryWindowOwner && !compactMode.trueMobile`. The effect must clean up on unmount and let `listenPrimaryCloudNotebookRestoreRequested` convert thrown transaction failures into a safe `false` completion.

Remove `establishedCatalogRequestPending`, `establishedCatalogRequestPendingRef`, and the old `listenPrimaryCloudNotebookCatalogRequested` effect. Do not alter local state or callbacks used by Welcome, direct main-window catalog browsing, or true mobile.

- [ ] **Step 4: Run focused and adjacent App tests**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/App.test.tsx -t "cloud|settings-owned|same-name settings selection|stale settings selection"
```

Expected: PASS. Welcome, disabled-config, current-name, remote-name, stale-revision, mobile, and new headless request cases remain green.

- [ ] **Step 5: Commit the primary executor**

```bash
git add packages/app/src/App.tsx packages/app/src/App.test.tsx
git commit -m "feat: execute cloud notebook restores from settings"
```

---

### Task 3: Render and control the dialog inside Settings

**Files:**
- Create: `packages/app/src/hooks/useSettingsRemoteNotebookDialog.ts`
- Create: `packages/app/src/hooks/useSettingsRemoteNotebookDialog.test.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts:215-435,780-820`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts:208-415`
- Modify: `packages/app/src/components/SettingsWindow.tsx:1-280`
- Modify: `packages/app/src/components/SettingsWindow.test.tsx:39-90`

**Interfaces:**
- Consumes: `requestPrimaryCloudNotebookRestore(input)`, `getAppRuntime().syncConfig.load/listNotebooks`, `CompactSyncSettingsController.begin/end`, and the existing `RemoteNotebookDialog`.
- Produces: `SettingsRemoteNotebookDialogController` and `remoteNotebookDialog` on `useSettingsWindowState()`.

- [ ] **Step 1: Write failing controller tests**

Define the controller's public result exactly as:

```ts
export type SettingsRemoteNotebookDialogController = {
  currentNotebookName: string | null;
  entries: readonly RemoteNotebookCatalogEntry[];
  error: string | null;
  loading: boolean;
  open: boolean;
  openDialog: () => Promise<unknown>;
  cancel: () => unknown;
  refresh: () => Promise<unknown>;
  restore: (remoteName: string) => Promise<unknown>;
};
```

Write hook tests that prove:

```ts
it("ends editing, loads the authoritative catalog, and never hides Settings", async () => {
  await act(async () => result.current.openDialog());
  expect(syncSession.end).toHaveBeenCalledWith("catalog-handoff");
  expect(runtime.syncConfig.load).toHaveBeenCalled();
  expect(runtime.syncConfig.listNotebooks).toHaveBeenCalledWith({ revision: "rev-2" });
  expect(result.current.open).toBe(true);
  expect(runtime.window.hideSettingsWindow).not.toHaveBeenCalled();
});

it("cancels only the dialog and resumes the Sync editing session", async () => {
  await act(async () => result.current.openDialog());
  act(() => result.current.cancel());
  await waitFor(() => expect(syncSession.begin).toHaveBeenCalled());
  expect(result.current.open).toBe(false);
});

it("keeps a failed restore open and retryable", async () => {
  mockedRequestPrimaryCloudNotebookRestore.mockResolvedValueOnce(false);
  await expect(result.current.restore("Archive")).rejects.toThrow();
  expect(result.current.open).toBe(true);
});

it("closes only the dialog and resumes Sync after a successful restore", async () => {
  mockedRequestPrimaryCloudNotebookRestore.mockResolvedValueOnce(true);
  await act(async () => result.current.restore("Archive"));
  expect(result.current.open).toBe(false);
  expect(syncSession.begin).toHaveBeenCalled();
});
```

Also cover list failure followed by successful `refresh`, request-generation protection against a late catalog result after cancel, and current notebook name derivation from `primaryRoot` with `pathNameFromPath`.
For a remote name different from the current notebook, rerender with the restored primary root before expecting `syncSession.begin()`; for the current same-name selection, expect the session to resume immediately.

- [ ] **Step 2: Run the hook test to verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/hooks/useSettingsRemoteNotebookDialog.test.tsx
```

Expected: FAIL because the hook does not exist.

- [ ] **Step 3: Implement the isolated Settings dialog controller**

Use this exact input boundary:

```ts
export type UseSettingsRemoteNotebookDialogInput = {
  onSessionFailure: () => unknown;
  primaryRoot: string | null;
  syncSession: Pick<CompactSyncSettingsController, "begin" | "end">;
  translate: (key: I18nKey) => string;
};
```

The implementation must:

1. serialize `openDialog` calls with one promise ref;
2. call `syncSession.end("catalog-handoff")` before catalog access;
3. call `syncConfig.load()` and require `status === "loaded"`, `configured === true`, and a non-null revision;
4. set `open: true` and `loading: true` before calling `listNotebooks({ revision })`;
5. store only the localized `notebooks.remote.refreshError` on catalog failure;
6. invalidate late catalog responses with a monotonically increasing generation ref;
7. call `requestPrimaryCloudNotebookRestore({ remoteName, revision })` from `restore` and throw a local generic error when it returns false;
8. leave the dialog open on restore failure so `RemoteNotebookDialog` displays its existing localized operation error;
9. own an `AbortController` for the active restore request and abort it on unmount;
10. close dialog state immediately after a successful response, but wait until `pathNameFromPath(primaryRoot)` equals the selected remote name before restarting `syncSession.begin()`; this resumes immediately for the current same-name selection and after the primary-workspace event for a different notebook;
11. restart `syncSession.begin()` immediately after cancel;
12. call `onSessionFailure` if ending or restarting the editing session fails.

No function in this hook may call `hideSettingsWindow` or `requestPrimaryCloudNotebookCatalog`.

- [ ] **Step 4: Integrate the controller into Settings state and UI**

In `useSettingsWindowState`, compose the hook with the current `syncPrimaryRoot`, `syncSession`, `translate`, and `showSyncExitFailure`. Return:

```ts
remoteNotebookDialog,
handleSelectCloudNotebook: remoteNotebookDialog.openDialog
```

Remove the old catalog request latch and the `hideSettingsWindow()` call from `handleSelectCloudNotebook`. Keep `recoverVisibleSyncSession` for ordinary retained-window reopen behavior; simplify only branches that existed specifically to clear or recover the obsolete catalog-hide latch.

In `SettingsWindow`, import and render:

```tsx
{remoteNotebookDialog.open ? (
  <RemoteNotebookDialog
    allowCurrentNotebookSelection
    currentNotebookName={remoteNotebookDialog.currentNotebookName}
    entries={remoteNotebookDialog.entries}
    error={remoteNotebookDialog.error}
    language={appLanguage.language}
    loading={remoteNotebookDialog.loading}
    onCancel={remoteNotebookDialog.cancel}
    onRefresh={remoteNotebookDialog.refresh}
    onRestore={remoteNotebookDialog.restore}
  />
) : null}
```

Place it after the settings layout inside the existing top-level `<main>` so its fixed overlay dims Settings and its existing focus trap remains active.

- [ ] **Step 5: Add the SettingsWindow regression test**

Configure a ready primary workspace, a configured Sync load result, and `listNotebooks` returning `Archive`. Navigate to Sync, select the cloud action, and assert:

```ts
const settings = screen.getByRole("main", { name: "Settings" });
fireEvent.click(screen.getByRole("button", { name: "Select Cloud Notebook" }));
const dialog = await screen.findByRole("dialog", { name: "Restore notebook from cloud" });

expect(settings).toBeInTheDocument();
expect(within(dialog).getByRole("radio", { name: "Archive" })).toBeEnabled();
expect(mockedHideSettingsWindow).not.toHaveBeenCalled();

fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
expect(screen.queryByRole("dialog", { name: "Restore notebook from cloud" }))
  .not.toBeInTheDocument();
expect(screen.getByRole("button", { name: "Select Cloud Notebook" }))
  .toBeInTheDocument();
```

Update `useSettingsWindowState.test.ts` by deleting assertions that successful cloud selection calls `hideSettingsWindow`, completes a catalog-specific hide handshake, or reopens Settings solely to recover that latch. Retain and adapt tests for failed session end, ordinary hide failure, retained Sync reopen, category switching, and targetless reopen.

- [ ] **Step 6: Run the Settings tests to verify GREEN**

Run:

```bash
pnpm --filter @markra/app exec vitest run \
  src/hooks/useSettingsRemoteNotebookDialog.test.tsx \
  src/hooks/useSettingsWindowState.test.ts \
  src/components/SettingsWindow.test.tsx \
  src/components/notebooks/RemoteNotebookDialog.test.tsx
```

Expected: PASS, including explicit proof that `hideSettingsWindow` is never called by the cloud notebook action.

- [ ] **Step 7: Commit the Settings-owned dialog**

```bash
git add \
  packages/app/src/hooks/useSettingsRemoteNotebookDialog.ts \
  packages/app/src/hooks/useSettingsRemoteNotebookDialog.test.tsx \
  packages/app/src/hooks/useSettingsWindowState.ts \
  packages/app/src/hooks/useSettingsWindowState.test.ts \
  packages/app/src/components/SettingsWindow.tsx \
  packages/app/src/components/SettingsWindow.test.tsx
git commit -m "fix: keep settings open for cloud notebook dialog"
```

---

### Task 4: Remove the obsolete native catalog handoff and verify the product boundary

**Files:**
- Delete: `packages/app/src/lib/cloud-notebook-catalog-events.ts`
- Delete: `packages/app/src/lib/cloud-notebook-catalog-events.test.ts`
- Modify: `packages/app/src/runtime/index.ts:378-408,790-815`
- Modify: `packages/app/src/runtime/index.test.ts:93-99`
- Modify: `apps/desktop/src/runtime/desktop.ts:219-240`
- Modify: `apps/desktop/src/runtime/index.test.ts:185-195`
- Modify: `apps/desktop/src/runtime/tauri/window.ts:51-72`
- Modify: `apps/desktop/src/runtime/tauri/window.test.ts:295-301`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs:23-134,198-212,420-435,500-700`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs:120-154`
- Modify: `packages/app/src/test/app-harness.tsx`: remove obsolete native mock/export/reset entries if still present.

**Interfaces:**
- Consumes: Task 1's generic Tauri event transport.
- Produces: no `requestPrimaryCloudNotebookCatalog` runtime or native command remains.

- [ ] **Step 1: Add a failing source-boundary assertion before cleanup**

In the relevant runtime tests, assert that `AppWindowRuntime` and the desktop adapter no longer expose `requestPrimaryCloudNotebookCatalog`. In the Rust builder boundary test, remove `request_primary_cloud_notebook_catalog` from `DESKTOP_COMMANDS` and assert the registered handler does not contain that command.

Run:

```bash
pnpm --filter @markra/app exec vitest run src/runtime/index.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/index.test.ts src/runtime/tauri/window.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_registers_the_primary_cloud_catalog_request -- --exact
```

Expected: at least one failure while the obsolete TypeScript/Rust bridge still exists.

- [ ] **Step 2: Delete the obsolete bridge completely**

Remove all of the following symbols and their dedicated tests:

```text
requestPrimaryCloudNotebookCatalog
requestNativePrimaryCloudNotebookCatalog
request_primary_cloud_notebook_catalog
PRIMARY_CLOUD_NOTEBOOK_CATALOG_REQUESTED_EVENT
PrimaryCloudNotebookCatalogWindow
deliver_primary_cloud_notebook_catalog_request
primaryCloudNotebookCatalogRequestedEvent
listenPrimaryCloudNotebookCatalogRequested
```

Remove the command from `tauri::generate_handler!` and `DESKTOP_COMMANDS`. Do not remove the main-window `RemoteNotebookDialog` state used by Welcome/direct catalog entry points.

- [ ] **Step 3: Verify no stale handoff references remain**

Run:

```bash
rg -n "requestPrimaryCloudNotebookCatalog|request_primary_cloud_notebook_catalog|cloud-notebook-catalog-requested|PrimaryCloudNotebookCatalogWindow" \
  packages/app/src apps/desktop/src apps/desktop/src-tauri/src
```

Expected: no matches.

- [ ] **Step 4: Run focused native/runtime tests**

Run:

```bash
pnpm --filter @markra/app exec vitest run \
  src/lib/cloud-notebook-restore-events.test.ts \
  src/runtime/index.test.ts
pnpm --filter @markra/desktop exec vitest run \
  src/runtime/index.test.ts \
  src/runtime/tauri/window.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests
```

Expected: all commands exit 0.

- [ ] **Step 5: Commit obsolete bridge removal**

```bash
git add \
  packages/app/src/lib/cloud-notebook-catalog-events.ts \
  packages/app/src/lib/cloud-notebook-catalog-events.test.ts \
  packages/app/src/runtime/index.ts \
  packages/app/src/runtime/index.test.ts \
  packages/app/src/test/app-harness.tsx \
  apps/desktop/src/runtime/desktop.ts \
  apps/desktop/src/runtime/index.test.ts \
  apps/desktop/src/runtime/tauri/window.ts \
  apps/desktop/src/runtime/tauri/window.test.ts \
  apps/desktop/src-tauri/src/desktop_runtime.rs \
  apps/desktop/src-tauri/src/builder_boundary_tests.rs
git commit -m "refactor: remove cloud catalog window handoff"
```

- [ ] **Step 6: Run final verification from a clean understanding of workspace drift**

First record `git status --short`, then run:

```bash
pnpm --filter @markra/app exec vitest run \
  src/lib/cloud-notebook-restore-events.test.ts \
  src/hooks/useSettingsRemoteNotebookDialog.test.tsx \
  src/hooks/useSettingsWindowState.test.ts \
  src/components/SettingsWindow.test.tsx \
  src/components/notebooks/RemoteNotebookDialog.test.tsx \
  src/App.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
git status --short
```

Expected: every test/build command exits 0; `git diff --check` reports no whitespace errors; final status contains only the user's pre-existing unrelated work and no generated build drift.
