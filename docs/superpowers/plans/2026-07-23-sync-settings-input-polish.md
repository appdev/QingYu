# Sync Settings Input Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish QingYu's Sync Settings text inputs by unifying their width, typography, responsive behavior, and focus treatment.

**Architecture:** Keep `SettingsTextInput` as the single settings-layer abstraction and change only its Tailwind class contract. Existing Sync Settings call sites inherit the new 256 px default while explicit wider callers remain unchanged.

**Tech Stack:** React 19, TypeScript 6, Tailwind CSS 4, Vitest, Testing Library

## Global Constraints

- Use existing QingYu design tokens; add no colors, dependencies, or new settings.
- Preserve all input behavior and sync business logic.
- Do not modify the already dirty `SyncSettings.tsx` or `SyncSettings.test.tsx` files in the primary checkout.
- Use `pnpm` for all JavaScript workflows.

---

### Task 1: Refine the shared settings text input

**Files:**
- Modify: `packages/app/src/components/settings/SettingsControls.tsx:127-159`
- Test: `packages/app/src/components/settings/SettingsControls.test.tsx`

**Interfaces:**
- Consumes: `SettingsTextInput({ label, onChange, placeholder, type, value, widthClassName })`
- Produces: the same component API with a `w-64` default and a polished visual class contract

- [x] **Step 1: Write the failing visual-contract test**

Add assertions to the existing `SettingsTextInput` test:

```tsx
expect(input).toHaveClass(
  "h-9",
  "w-64",
  "max-[760px]:w-full",
  "text-[13px]",
  "font-[520]",
  "transition-[background-color,border-color]",
  "focus:border-(--accent)",
  "focus:ring-2",
  "focus:ring-(--accent)/20"
);
expect(input.className).not.toContain("focus-visible:outline-");
expect(input).not.toHaveClass("focus-visible:ring-(--accent)");
```

- [x] **Step 2: Run the test to verify it fails**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/SettingsControls.test.tsx
```

Expected: the visual-contract assertion fails because the old input is `h-8 w-44` with an opaque focus-visible ring.

- [x] **Step 3: Implement the minimal class change**

Change the default width and input classes while preserving the component signature and event behavior:

```tsx
widthClassName = "w-64"

className={`h-9 ${widthClassName} max-[760px]:w-full rounded-md border border-(--border-default) bg-(--bg-primary) px-3 text-[13px] leading-5 font-[520] text-(--text-heading) caret-(--accent) outline-none transition-[background-color,border-color] duration-150 ease-out placeholder:text-(--text-secondary) hover:bg-(--bg-hover) focus:border-(--accent) focus:ring-2 focus:ring-(--accent)/20`}
```

- [x] **Step 4: Run focused verification**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/SettingsControls.test.tsx
```

Expected: one test file passes with all assertions green.

- [ ] **Step 5: Run repository verification and commit**

Run:

```bash
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: all commands exit 0.

Commit only the two implementation files after the design and plan commit:

```bash
git add packages/app/src/components/settings/SettingsControls.tsx packages/app/src/components/settings/SettingsControls.test.tsx
git commit -m "style: polish settings text inputs"
```
