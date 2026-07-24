# QingYu Transient Sync Error Toast Design

## Status

The visual design was approved on 2026-07-23. This written specification is awaiting final review before implementation planning.

## Problem

The current application-sync failure toast exposes a safe internal diagnostic string such as `sync-failed · sync` directly beneath the localized title. That string is useful in logs but does not explain the failure to an end user.

The centered toaster also uses content-sized width. When a title, description, error icon, and close control share the row, the description column can collapse toward its minimum content width. This produces the narrow, word-by-word wrapping visible in the reported dark-theme screenshot.

The current presentation is heavier than the intended interaction. A synchronization failure is already recorded in the runtime and rotating diagnostic logs; the toast only needs to notify the user briefly and provide an immediate retry opportunity.

## Goals

- Present a lightweight, single-line synchronization failure notification.
- Show user-facing copy instead of internal synchronization codes.
- Keep the existing top-center placement and dynamic theme integration.
- Auto-dismiss after three seconds when the user does not interact.
- Pause dismissal while the toast or its retry action is hovered or keyboard-focused.
- Offer an inline retry action without adding a close control.
- Preserve structured failure diagnostics in the existing logs.
- Change only the application-sync failure presentation; other application toasts keep their current behavior.

## Non-goals

- Do not redesign success, update, diagnostics, file-operation, or settings-window toasts.
- Do not change synchronization execution, conflict handling, retry policy, or diagnostic collection.
- Do not remove safe error details from the runtime log or `QingYu.log`.
- Do not add a new notification center, modal, settings page, or persistent error panel.
- Do not add a success toast after a successful retry.

## Alternatives Considered

### Action-first two-line toast

This approach explained the failure in a short description and placed `Retry` and `View logs` beneath it. It was the clearest recovery surface, but it was taller and more interruptive than the requested transient notification.

### Minimal inline toast — selected

This approach keeps only the status title and retry action in one row. It is the quietest option, preserves writing focus, and avoids the layout failure caused by a secondary diagnostic line.

### Expandable diagnostic toast

This approach kept the internal error code inside a collapsed disclosure. It preserved a local diagnostic path but added technical weight to a message whose details already exist in the log surfaces.

## Visual Design

The selected toast is a single horizontal row:

1. a small Lucide error-status icon;
2. the localized title `Sync did not complete` / `同步未完成`;
3. an inline localized `Retry` / `重试` action.

There is no description row and no close button.

The reference size is 320 px wide by 48 px high. Width remains bounded by the available viewport on narrow windows. The title owns the flexible middle column, stays on one line, and truncates with an ellipsis only when a translation cannot fit. The retry label never wraps.

The toast follows the locked QingYu design system:

- system UI font stack;
- 13 px compact title typography;
- 6 px surface radius;
- theme tokens for background, border, text, focus, and semantic error colour;
- one subtle functional shadow appropriate to the active theme;
- no additional brand colour, gradient, nested container, or decorative animation.

The error icon supplies a shape signal and uses the current semantic danger token. The rest of the toast remains neutral so the message reads as informative rather than alarming.

## Copy and Diagnostics

The visible message is deliberately generic because `sync-failed` can represent multiple provider, transport, application, or editing-state failures.

- Title: `Sync did not complete` / `同步未完成`.
- Action: `Retry` / `重试`.

The toast must not render `safeErrorDescription`, stable error codes, HTTP status, provider codes, request IDs, paths, or other technical fields.

The existing structured `appLogger.error` record remains unchanged. Safe diagnostic fields continue to reach the in-app runtime log and rotating desktop log, where they can be inspected without competing with the transient notification.

Dedicated synchronization-toast localization keys are used so the existing settings and compact-runtime wording does not change as a side effect.

## Lifecycle and Interaction

### Appearance

The toast enters at the existing top-center position. Its only spatial motion is a short opacity and upward-position settle. Reduced-motion mode uses opacity only.

The toast reuses the existing `app-sync` ID. A later synchronization failure updates the existing item instead of stacking another one.

### Automatic dismissal

The dwell time is 3000 ms.

The timer pauses when any of the following is true:

- the pointer is over the toast;
- the retry action is keyboard-focused;
- a retry is running.

When pointer and focus interaction ends without a retry, the toast receives a fresh three-second dwell window. This is intentionally simpler and more predictable than exposing a partially elapsed countdown.

The toast has no close control and disables other manual dismissal affordances for this variant. It leaves through its timer or the retry lifecycle.

### Retry

Selecting `Retry` starts a manual synchronization against the current active workspace and current installed synchronization revision.

While retrying:

- cancel the auto-dismiss timer;
- disable the action;
- replace the action label with the existing compact loading spinner;
- change the title immediately to the localized syncing state;
- keep focus and accessibility state stable.

A successful retry dismisses the toast silently. A failed retry updates the same `app-sync` toast back to the failure state and starts a new three-second dwell window. It does not stack a second failure notification.

## Accessibility

- Use `role="status"` and `aria-live="polite"`; synchronization failure is important but not safety-critical.
- The retry action has a visible, non-animated `:focus-visible` ring with at least 3:1 contrast.
- The retry label remains on one line at all supported widths.
- The compact visual action expands to at least a 44 px hit target without increasing the 48 px toast height.
- Hover, focus, active, disabled, loading, error, and success lifecycle states are defined.
- State is never communicated through colour alone; the icon, title, action text, and spinner provide independent signals.
- Reduced-motion mode removes spatial entrance and exit movement and keeps a short opacity transition.

## Implementation Boundaries

The existing shared toast API remains the entry point. It forwards the Sonner options needed by this one variant, including the per-toast duration, close-button and dismissibility settings, styling hook, and action.

The application-sync coordinator owns the retry callback because it already owns synchronization generations, current workspace identity, installed revision checks, shared-run deduplication, logging, and failure notification deduplication.

The toaster component owns the transient sync presentation classes. The synchronization hook selects that presentation but must not embed raw layout or colour values.

Expected implementation surfaces are:

- `packages/app/src/lib/app-toast.ts` for typed per-toast presentation options;
- `packages/app/src/components/AppToaster.tsx` for the sync-only single-line layout and state styling;
- `packages/app/src/hooks/useAppSyncCoordinator.ts` for the new copy, duration, diagnostics omission, and retry action;
- focused toast, coordinator, and application integration tests;
- shared localization keys and primary locale strings for the new title and retry label.

No production file or component is deleted. Other toast call sites retain their current default close button, duration, width, descriptions, and actions.

## Error and Concurrency Behavior

Retry delegates to the existing manual synchronization path. It does not bypass readiness, workspace-generation, editing-session, or revision checks.

If the active workspace or configuration changes before the user retries, the current coordinator state remains authoritative. The stale toast action must not replay a captured obsolete request.

Existing shared-run deduplication prevents the inline retry from starting a duplicate execution when an equivalent run is already active. Failure logging remains once per shared run, and the `app-sync` toast ID prevents visual duplication.

If retry cannot start because synchronization is no longer ready, the coordinator replaces the same toast with the existing localized readiness message rather than silently doing nothing.

## Verification

### Toast API tests

- Forward a 3000 ms duration, `closeButton: false`, and disabled manual dismissal for the transient sync variant.
- Forward the inline retry action without changing default toast behavior.
- Preserve existing success, loading, notice-surface, long-description, and update-toast behavior.

### Coordinator tests

- A visible application-sync failure emits one toast with the new localized title, retry action, and no description.
- The visible toast never contains `sync-failed`, operation names, HTTP status, provider codes, or request IDs.
- Clicking retry uses the current manual synchronization path.
- A successful retry dismisses silently.
- A failed retry reuses the `app-sync` ID and does not stack another toast.
- Structured failure details still reach `appLogger.error` unchanged.
- Editing-state and document-membership failures use the same transient presentation without losing their logs.

### Rendered integration tests

- The sync toast has no close button.
- The title and retry action remain one line in English and Simplified Chinese.
- The toast uses the 320 px reference width, 48 px reference height, and narrow-window maximum.
- The retry action exposes hover, focus-visible, active, disabled, and loading states.
- The toast auto-dismisses after three seconds without interaction.
- Hover and keyboard focus pause dismissal; leaving interaction restarts the full dwell window.
- Reduced-motion mode removes spatial movement.
- Other toast types retain their existing close control and lifecycle.

### Repository gates

Run the smallest focused Vitest suites first, then the repository gates required by the change:

- `pnpm --filter @markra/app exec vitest run src/lib/app-toast.test.ts src/hooks/useAppSyncCoordinator.test.tsx src/App.test.tsx`;
- `pnpm test`;
- `pnpm typecheck:test`;
- `pnpm build`;
- `git diff --check`.
