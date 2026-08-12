# Settings Shortcut Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the settings shortcut `Command+,` on macOS and `Ctrl+,` on Windows and Linux.

**Architecture:** Keep `config` as the single command and store its shortcut with the existing cross-platform primary-modifier notation. The renderer hotkey matcher and Electron native-menu converter consume the same configured value.

**Tech Stack:** TypeScript, Electron, Node test runner, pnpm

## Global Constraints

- Use `Command+,` on macOS and `Ctrl+,` on Windows and Linux.
- Preserve existing user keymap customization behavior.
- Do not add dependencies or platform-specific duplicate commands.
- Do not run `pnpm build`.

---

### Task 1: Unify the settings shortcut

**Files:**
- Create: `app/src/util/settingsShortcut.test.ts`
- Modify: `app/src/constants.ts:458`

**Interfaces:**
- Consumes: `Constants.SIYUAN_KEYMAP.general.config`, existing renderer hotkey matching, and native-menu accelerator conversion.
- Produces: `Constants.SIYUAN_KEYMAP.general.config.default` and `.custom` set to `⌘,`.

- [ ] **Step 1: Write the failing test**

```ts
import * as assert from "node:assert/strict";
import {describe, it} from "node:test";

Object.assign(globalThis, {SIYUAN_VERSION: "test", NODE_ENV: "test"});

describe("settings shortcut", () => {
    it("uses the platform primary modifier and comma", async () => {
        const {Constants} = await import("../constants");
        assert.deepEqual(Constants.SIYUAN_KEYMAP.general.config, {
            default: "⌘,",
            custom: "⌘,",
        });
    });
});
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `cd app && pnpm exec node --import tsx --test src/util/settingsShortcut.test.ts`

Expected: FAIL because the current value is `{default: "⌥P", custom: "⌥P"}`.

- [ ] **Step 3: Change the default keymap value**

```ts
config: {default: "⌘,", custom: "⌘,"},
```

- [ ] **Step 4: Run focused and native-menu tests**

Run: `cd app && pnpm exec node --import tsx --test src/util/settingsShortcut.test.ts src/util/nativeMenu.test.ts`

Expected: all tests pass.

- [ ] **Step 5: Run frontend verification**

Run: `cd app && pnpm run lint`

Expected: TypeScript and ESLint complete successfully.

- [ ] **Step 6: Inspect the final diff without committing**

Run: `git diff -- app/src/constants.ts app/src/util/settingsShortcut.test.ts docs/superpowers/specs/2026-08-10-settings-shortcut-design.md docs/superpowers/plans/2026-08-10-settings-shortcut.md`

Expected: only the settings shortcut, its regression test, and the two workflow documents are present.
