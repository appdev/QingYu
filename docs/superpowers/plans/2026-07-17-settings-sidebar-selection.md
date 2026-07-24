# Settings Sidebar Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the active desktop settings category visibly selected with the existing themed background and accent colors.

**Architecture:** Preserve `aria-current="page"` as the navigation source of truth and change only the Tailwind variants on the settings category button so the generated selector matches that exact value. Add a focused component regression test, then verify the generated production CSS and the live Tauri desktop UI.

**Tech Stack:** React 19, TypeScript, Tailwind CSS 4, Vitest, Testing Library, Tauri v2, pnpm workspace

## Global Constraints

- Keep the change limited to the main settings category sidebar.
- Use the existing `--bg-active` background token and `--accent` foreground token.
- Preserve `aria-current="page"`, hover behavior, focus treatment, spacing, routing, and settings behavior.
- Do not change theme variables, add a selection bar, refactor navigation state, or change other `aria-current` components.
- Use `pnpm` for every JavaScript workflow and do not add dependencies.
- Do not modify or commit the unrelated untracked `bg.png` file.

---

### Task 1: Match the Settings Category Selected Styles to `aria-current="page"`

**Files:**
- Modify: `packages/app/src/components/SettingsShell.test.tsx`
- Modify: `packages/app/src/components/SettingsShell.tsx:187-198`

**Interfaces:**
- Consumes: `SettingsSidebar({ activeCategory, ... })`, which already passes `active={category.id === activeCategory}` to each category button.
- Produces: an active category button with `aria-current="page"`, `aria-[current=page]:bg-(--bg-active)`, and `aria-[current=page]:text-(--accent)`.

- [ ] **Step 1: Write the failing regression test**

Add this test inside the existing `describe("SettingsShell", ...)` block in `packages/app/src/components/SettingsShell.test.tsx`:

```tsx
it("styles the active category through its page-specific current state", () => {
  renderSettingsSidebar();

  const activeCategory = screen.getByRole("button", { name: "General" });
  const inactiveCategory = screen.getByRole("button", { name: "AI" });

  expect(activeCategory).toHaveAttribute("aria-current", "page");
  expect(activeCategory).toHaveClass(
    "aria-[current=page]:bg-(--bg-active)",
    "aria-[current=page]:text-(--accent)"
  );
  expect(inactiveCategory).not.toHaveAttribute("aria-current");
});
```

- [ ] **Step 2: Run the test and verify the RED state**

Run:

```bash
pnpm --filter @markra/app test -- src/components/SettingsShell.test.tsx
```

Expected: FAIL in `styles the active category through its page-specific current state` because the active button still has `aria-current:bg-(--bg-active)` and `aria-current:text-(--accent)` instead of the page-specific classes.

- [ ] **Step 3: Implement the minimal selector correction**

In `SettingsNavButton`, replace only the two broken selected-state variants. The button must read:

```tsx
<button
  className="group inline-flex h-9 w-full items-center gap-3 rounded-md border-0 bg-transparent px-3 text-left text-[13px] leading-5 font-[620] tracking-normal text-(--text-secondary) transition-colors duration-150 ease-out hover:bg-(--bg-hover) hover:text-(--text-heading) aria-[current=page]:bg-(--bg-active) aria-[current=page]:text-(--accent) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
  type="button"
  aria-current={active ? "page" : undefined}
  aria-label={label}
  onClick={onClick}
>
  <Icon aria-hidden="true" size={15} />
  <span>{label}</span>
</button>
```

- [ ] **Step 4: Run the focused test and verify the GREEN state**

Run:

```bash
pnpm --filter @markra/app test -- src/components/SettingsShell.test.tsx
```

Expected: PASS with no failed tests in `SettingsShell.test.tsx`.

- [ ] **Step 5: Run package and production-build verification**

Run:

```bash
pnpm --filter @markra/app test
pnpm build
```

Expected: both commands exit with code 0. The app package reports zero failed tests, and the workspace production build completes without TypeScript, Tailwind, or Vite errors.

- [ ] **Step 6: Confirm the production CSS contains the corrected selector**

Run:

```bash
rg -o '.{0,100}aria-\\\[current=page\\\]\\:bg-\\\(--bg-active\\\).{0,100}' apps/desktop/dist/assets/*.css
rg -o '.{0,100}aria-\\\[current=page\\\]\\:text-\\\(--accent\\\).{0,100}' apps/desktop/dist/assets/*.css
```

Expected: both commands print a generated selector whose class uses `aria-[current=page]` and whose attribute selector is `[aria-current=page]`.

- [ ] **Step 7: Verify the behavior in the Tauri desktop app**

Start the actual desktop runtime:

```bash
pnpm tauri dev
```

In another shell, verify the runtime is live:

```bash
pgrep -f '/target/debug/markra'
curl -I --max-time 5 http://127.0.0.1:1420/
```

Expected: `pgrep` prints a process ID and `curl` returns HTTP 200. Open Settings, select at least two categories, and confirm the `--bg-active` background plus `--accent` label/icon move to the current category while the previous category returns to its normal state.

- [ ] **Step 8: Review the final diff and commit the fix**

Run:

```bash
git diff --check
git diff -- packages/app/src/components/SettingsShell.test.tsx packages/app/src/components/SettingsShell.tsx
git status --short
git add packages/app/src/components/SettingsShell.test.tsx packages/app/src/components/SettingsShell.tsx
git commit -m "fix(settings): show active sidebar category"
```

Expected: the diff contains only the focused test and two Tailwind variant replacements; `bg.png` remains untracked and unstaged; the commit succeeds locally without pushing.
