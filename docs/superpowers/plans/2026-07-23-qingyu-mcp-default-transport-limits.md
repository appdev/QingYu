# QingYu MCP Default Transport Limits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove transport-limit controls from the MCP settings page while retaining the existing internal defaults and runtime enforcement.

**Architecture:** Keep `McpConfig`, normalization, persistence, and Rust runtime behavior unchanged. Remove only the React rendering path and i18n keys that exist solely for those controls, with a UI regression test proving the defaults remain internal.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, shared QingYu i18n.

## Global Constraints

- Do not change the seven values returned by `defaultMcpConfig()`.
- Do not change MCP config serialization, normalization, persistence, or Rust runtime enforcement.
- Do not add an advanced-settings disclosure.
- Preserve the user's unrelated primary-worktree changes.

---

### Task 1: Remove transport limits from the settings surface

**Files:**
- Modify: `packages/app/src/components/settings/McpSettings.test.tsx`
- Modify: `packages/app/src/components/settings/McpSettings.tsx`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`
- Modify: `packages/shared/src/i18n/locales/types.ts`

**Interfaces:**
- Consumes: `defaultMcpConfig(): McpConfig` and the existing `McpSettings` component.
- Produces: an MCP settings surface with no transport-limit heading or inputs; the `McpConfig` interface remains unchanged.

- [ ] **Step 1: Write the failing UI test**

Add a test to `McpSettings.test.tsx` that asserts the exact defaults and confirms none of the seven inputs is rendered:

```tsx
it("keeps transport safeguards at defaults without exposing them as settings", async () => {
  const config = defaultMcpConfig();
  render(<McpSettings runtime={runtime()} />);

  expect(await screen.findByRole("button", { name: "Enable MCP" })).toBeEnabled();
  expect(config).toMatchObject({
    documentLimitBytes: 8 * 1024 * 1024,
    requestLimitBytes: 8 * 1024 * 1024,
    responseLimitBytes: 8 * 1024 * 1024,
    requestsPerMinute: 120,
    burstRequests: 20,
    concurrentCalls: 8,
    toolTimeoutSecs: 60
  });
  expect(screen.queryByRole("heading", { name: "Transport limits" })).not.toBeInTheDocument();
  for (const label of [
    "Document limit",
    "Request limit",
    "Response limit",
    "Requests per minute",
    "Burst requests",
    "Concurrent calls",
    "Tool timeout"
  ]) {
    expect(screen.queryByRole("spinbutton", { name: label })).not.toBeInTheDocument();
  }
});
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/McpSettings.test.tsx
```

Expected: FAIL because the `Transport limits` heading and seven spinbuttons still render.

- [ ] **Step 3: Remove the settings UI and UI-only messages**

In `McpSettings.tsx`, delete the `numberFields` constant and the `SettingsSection` that maps it to `NumberInput`. Keep `NumberInput` because the audit retention and entry-count controls still use it.

In all three locale files and `locales/types.ts`, delete only these keys:

```text
settings.mcp.section.transport
settings.mcp.transport.documentLimit
settings.mcp.transport.requestLimit
settings.mcp.transport.responseLimit
settings.mcp.transport.requestsPerMinute
settings.mcp.transport.burstRequests
settings.mcp.transport.concurrentCalls
settings.mcp.transport.toolTimeout
```

Update the existing broad rendering test so its required-label list no longer includes those seven transport-limit labels.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/McpSettings.test.tsx
```

Expected: the test file passes; the UI test confirms defaults remain and controls are absent.

- [ ] **Step 5: Run repository verification**

Run:

```bash
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
```

Expected: every command exits with status 0.

- [ ] **Step 6: Commit the implementation**

```bash
git add packages/app/src/components/settings/McpSettings.test.tsx \
  packages/app/src/components/settings/McpSettings.tsx \
  packages/shared/src/i18n/locales/en.ts \
  packages/shared/src/i18n/locales/zh-CN.ts \
  packages/shared/src/i18n/locales/zh-TW.ts \
  packages/shared/src/i18n/locales/types.ts
git commit -m "feat: simplify MCP transport settings"
```
