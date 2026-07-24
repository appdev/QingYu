# QingYu MCP Quick Client Configuration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a secret-free, ready-to-copy MCP client configuration after MCP is enabled in the QingYu desktop settings.

**Architecture:** The Rust desktop runtime resolves the bundled `qingyu-mcp` executable next to the running QingYu executable and returns it as `clientCommand` in the existing settings snapshot. A pure TypeScript formatter produces Codex TOML or generic JSON, while a focused React component owns format selection, previews, clipboard actions, and feedback. Mobile policy-only runtimes return no command and never show the component.

**Tech Stack:** Rust, Tauri v2 commands, React 19, TypeScript 6, Tailwind CSS, Vitest, Testing Library, pnpm.

## Global Constraints

- Preserve `MCP client -> qingyu-mcp stdio -> private local IPC -> QingYu`; add no HTTP listener, port, URL, token, Keychain access, or direct filesystem capability.
- The quick configuration is desktop-only and visible only while MCP is enabled.
- The generated configuration grants no MCP permissions and contains no notebook path.
- Do not directly edit another application's configuration.
- Use `pnpm` for JavaScript workflows and do not use the TypeScript `void` keyword or operator.
- Preserve unrelated user changes in the primary checkout.

---

### Task 1: Expose the installed sidecar command in MCP settings

**Files:**
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`
- Modify: `packages/app/src/lib/mcp.ts`
- Modify: `apps/desktop/src/runtime/tauri/mcp-policy.ts`
- Modify: `packages/app/src/test/app-harness.tsx`
- Modify: `packages/app/src/hooks/useMcpSettings.test.tsx`
- Modify: `packages/app/src/components/settings/McpSettings.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.test.tsx`
- Modify: `packages/app/src/lib/settings/app-settings.test.ts`

**Interfaces:**
- Produces: `McpSettingsSnapshot.clientCommand: string | null`.
- Produces: Rust `sidecar_command_for_executable(executable: &Path, executable_suffix: &str) -> Option<PathBuf>`.
- Consumes: `std::env::current_exe()` and `std::env::consts::EXE_SUFFIX`.

- [ ] **Step 1: Write failing Rust path-derivation tests**

Add tests that require the bridge to be a sibling of the app executable and preserve spaces while applying the requested executable suffix:

```rust
#[test]
fn sidecar_command_is_resolved_beside_the_application_executable() {
    let app = std::path::Path::new("/Applications/QingYu Preview.app/Contents/MacOS/QingYu");
    assert_eq!(
        super::sidecar_command_for_executable(app, ""),
        Some(std::path::PathBuf::from(
            "/Applications/QingYu Preview.app/Contents/MacOS/qingyu-mcp"
        ))
    );
}

#[test]
fn sidecar_command_uses_the_platform_executable_suffix() {
    let app = std::path::Path::new("/opt/qingyu/QingYu.exe");
    assert_eq!(
        super::sidecar_command_for_executable(app, ".exe"),
        Some(std::path::PathBuf::from("/opt/qingyu/qingyu-mcp.exe"))
    );
}
```

- [ ] **Step 2: Run the Rust tests and confirm the missing helper failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sidecar_command_ -- --nocapture`

Expected: FAIL because `sidecar_command_for_executable` does not exist.

- [ ] **Step 3: Implement the backend contract**

Add the pure helper and a fallible current-process wrapper:

```rust
fn sidecar_command_for_executable(
    executable: &std::path::Path,
    executable_suffix: &str,
) -> Option<std::path::PathBuf> {
    Some(executable.parent()?.join(format!("qingyu-mcp{executable_suffix}")))
}

fn current_sidecar_command() -> Option<String> {
    let executable = std::env::current_exe().ok()?;
    sidecar_command_for_executable(&executable, std::env::consts::EXE_SUFFIX)
        .map(|command| command.to_string_lossy().into_owned())
}
```

Store that optional string on `McpState`, serialize it as `clientCommand` in `McpSettingsSnapshot`, and copy it into every settings response without making settings fail when `current_exe()` is unavailable.

Add the required frontend field:

```ts
export type McpSettingsSnapshot = {
  revision: string;
  config: McpConfig;
  clientCommand: string | null;
  endpoint: string | null;
  health: McpServerHealth;
  workspace: McpCurrentWorkspace | null;
};
```

Set `clientCommand: null` in the mobile policy snapshot. Set explicit representative values in all named test fixtures: `/Applications/QingYu.app/Contents/MacOS/qingyu-mcp` for desktop fixtures and `null` for mobile fixtures.

- [ ] **Step 4: Run focused backend and TypeScript contract tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sidecar_command_ -- --nocapture
pnpm --filter @markra/app typecheck:test
```

Expected: both PASS.

- [ ] **Step 5: Commit the runtime contract**

```bash
git add apps/desktop/src-tauri/src/mcp/mod.rs apps/desktop/src-tauri/src/mcp/tests.rs packages/app/src/lib/mcp.ts apps/desktop/src/runtime/tauri/mcp-policy.ts packages/app/src/test/app-harness.tsx packages/app/src/hooks/useMcpSettings.test.tsx packages/app/src/components/settings/McpSettings.test.tsx packages/app/src/components/compact/CompactSettingsDetail.test.tsx packages/app/src/lib/settings/app-settings.test.ts
git commit -m "feat: expose QingYu MCP client command"
```

### Task 2: Format Codex and generic client configurations

**Files:**
- Create: `packages/app/src/lib/mcp-client-config.ts`
- Create: `packages/app/src/lib/mcp-client-config.test.ts`

**Interfaces:**
- Produces: `McpClientConfigFormat = "codex" | "json"`.
- Produces: `formatMcpClientConfiguration(command: string, format: McpClientConfigFormat): string`.

- [ ] **Step 1: Write failing formatter tests**

```ts
import { formatMcpClientConfiguration } from "./mcp-client-config";

describe("formatMcpClientConfiguration", () => {
  it("formats a Codex TOML server with an escaped command", () => {
    expect(formatMcpClientConfiguration('C:\\Program Files\\QingYu\\qingyu-mcp.exe', "codex"))
      .toBe('[mcp_servers.qingyu]\ncommand = "C:\\\\Program Files\\\\QingYu\\\\qingyu-mcp.exe"');
  });

  it("formats generic MCP JSON without credentials or arguments", () => {
    const value = JSON.parse(formatMcpClientConfiguration(
      "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp",
      "json"
    ));
    expect(value).toEqual({
      mcpServers: {
        qingyu: { command: "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp" }
      }
    });
  });
});
```

- [ ] **Step 2: Run the formatter test and confirm the missing module failure**

Run: `pnpm --filter @markra/app exec vitest run src/lib/mcp-client-config.test.ts`

Expected: FAIL because `mcp-client-config.ts` does not exist.

- [ ] **Step 3: Implement the pure formatter**

```ts
export type McpClientConfigFormat = "codex" | "json";

export function formatMcpClientConfiguration(
  command: string,
  format: McpClientConfigFormat
) {
  if (format === "codex") {
    return `[mcp_servers.qingyu]\ncommand = ${JSON.stringify(command)}`;
  }
  return JSON.stringify({ mcpServers: { qingyu: { command } } }, null, 2);
}
```

- [ ] **Step 4: Run the formatter test**

Run: `pnpm --filter @markra/app exec vitest run src/lib/mcp-client-config.test.ts`

Expected: PASS.

- [ ] **Step 5: Commit the formatter**

```bash
git add packages/app/src/lib/mcp-client-config.ts packages/app/src/lib/mcp-client-config.test.ts
git commit -m "feat: format QingYu MCP client configuration"
```

### Task 3: Add the enabled-state quick-configuration UI

**Files:**
- Create: `packages/app/src/components/settings/McpClientConfiguration.tsx`
- Create: `packages/app/src/components/settings/McpClientConfiguration.test.tsx`
- Modify: `packages/app/src/components/settings/McpSettings.tsx`
- Modify: `packages/app/src/components/settings/McpSettings.test.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`

**Interfaces:**
- Consumes: `formatMcpClientConfiguration(command, format)` and `McpSettingsSnapshot.clientCommand`.
- Produces: `McpClientConfiguration` with `{ command, translate, writeClipboard? }` props.
- Produces: an optional `writeClipboard` prop on `McpSettings` for deterministic tests.

- [ ] **Step 1: Write failing component tests**

Cover these exact behaviors in `McpClientConfiguration.test.tsx`:

```ts
it("copies the selected Codex configuration", async () => {
  const writeClipboard = vi.fn(async () => undefined);
  render(<McpClientConfiguration command="/Applications/QingYu.app/Contents/MacOS/qingyu-mcp" translate={englishTranslate} writeClipboard={writeClipboard} />);
  fireEvent.click(screen.getByRole("button", { name: "Copy configuration" }));
  await waitFor(() => expect(writeClipboard).toHaveBeenCalledWith(
    '[mcp_servers.qingyu]\ncommand = "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp"'
  ));
});

it("switches to generic JSON and copies an AI installation request", async () => {
  const writeClipboard = vi.fn(async () => undefined);
  render(<McpClientConfiguration command="/opt/qingyu/qingyu-mcp" translate={englishTranslate} writeClipboard={writeClipboard} />);
  fireEvent.change(screen.getByLabelText("Configuration format"), { target: { value: "json" } });
  fireEvent.click(screen.getByRole("button", { name: "Copy for AI tool" }));
  await waitFor(() => expect(writeClipboard).toHaveBeenCalledWith(
    expect.stringContaining('"command": "/opt/qingyu/qingyu-mcp"')
  ));
  expect(vi.mocked(writeClipboard).mock.calls[0][0]).toContain("Do not add a URL or token");
});
```

Also test localized success status and a rejected clipboard promise rendering an alert.

In `McpSettings.test.tsx`, add one test proving the section is absent when disabled, appears after enabling, and is absent for `localServiceAvailable: false`.

- [ ] **Step 2: Run the component tests and confirm failure**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/McpClientConfiguration.test.tsx src/components/settings/McpSettings.test.tsx
```

Expected: FAIL because the component and translation keys do not exist.

- [ ] **Step 3: Add localized copy and connection text**

Add typed keys for section title, summary, transport/authentication facts, format labels, copy buttons, AI instruction, copied status, unavailable state, and copy error to `types.ts`, `en.ts`, `zh-CN.ts`, and `zh-TW.ts`.

The English AI instruction is:

```text
Add the following QingYu MCP server to your MCP client configuration. Keep the command unchanged. Do not add a URL or token.
```

The Simplified Chinese instruction is:

```text
请将下面的轻语 MCP 服务添加到你的 MCP 客户端配置中。保持 command 不变，不要添加 URL 或 Token。
```

- [ ] **Step 4: Implement the focused UI component**

Create a component that:

```ts
type Props = {
  command: string;
  translate: (key: I18nKey) => string;
  writeClipboard?: (text: string) => Promise<unknown>;
};
```

It defaults to Codex, derives the preview with `formatMcpClientConfiguration`, renders `Transport: stdio + private local IPC` and `Authentication: No token required`, uses a native select for the format, puts the preview in a wrapping/scrolling `<pre><code>`, and catches clipboard rejection to show `role="alert"`.

Insert it in `McpSettings` immediately after the service section only when:

```ts
snapshot.config.enabled && runtime.localServiceAvailable && snapshot.clientCommand
```

When desktop MCP is enabled but `clientCommand` is null, render the localized unavailable note instead. Do not render either branch in the policy-only/mobile presentation.

- [ ] **Step 5: Run the focused component tests**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/McpClientConfiguration.test.tsx src/components/settings/McpSettings.test.tsx
pnpm --filter @markra/app typecheck:test
```

Expected: all PASS.

- [ ] **Step 6: Commit the UI**

```bash
git add packages/app/src/components/settings/McpClientConfiguration.tsx packages/app/src/components/settings/McpClientConfiguration.test.tsx packages/app/src/components/settings/McpSettings.tsx packages/app/src/components/settings/McpSettings.test.tsx packages/shared/src/i18n/locales/types.ts packages/shared/src/i18n/locales/en.ts packages/shared/src/i18n/locales/zh-CN.ts packages/shared/src/i18n/locales/zh-TW.ts
git commit -m "feat: add MCP quick client configuration"
```

### Task 4: Update user documentation and run release gates

**Files:**
- Modify: `docs/qingyu-mcp.md`
- Add: `docs/superpowers/specs/2026-07-22-qingyu-mcp-quick-configuration-design.md`
- Add: `docs/superpowers/plans/2026-07-22-qingyu-mcp-quick-configuration.md`

**Interfaces:**
- Documents: the same Codex TOML and generic JSON produced by the application.

- [ ] **Step 1: Update the MCP connection instructions**

State that users enable MCP, copy the current installation's configuration from Settings -> MCP, and paste it into their MCP host. Include both formats and explicitly state that no token, URL, or manual notebook path belongs in the configuration.

- [ ] **Step 2: Run focused verification**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sidecar_command_ -- --nocapture
pnpm --filter @markra/app exec vitest run src/lib/mcp-client-config.test.ts src/components/settings/McpClientConfiguration.test.tsx src/components/settings/McpSettings.test.tsx
git diff --check
```

Expected: all tests PASS and `git diff --check` prints nothing.

- [ ] **Step 3: Run repository gates**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: every command exits 0.

- [ ] **Step 4: Review security-sensitive output**

Run:

```bash
rg -n 'Bearer|token|http://|https://|workspacePath|absolute/path' packages/app/src/components/settings/McpClientConfiguration.tsx packages/app/src/lib/mcp-client-config.ts docs/qingyu-mcp.md
git diff --stat
git diff -- apps/desktop/src-tauri/src/mcp packages/app/src/components/settings packages/app/src/lib/mcp-client-config.ts packages/shared/src/i18n/locales docs/qingyu-mcp.md
```

Expected: only explanatory statements reject URLs/tokens; generated configuration contains only the bundled sidecar command.

- [ ] **Step 5: Commit documentation and merge the verified branch**

```bash
git add docs/qingyu-mcp.md docs/superpowers/specs/2026-07-22-qingyu-mcp-quick-configuration-design.md docs/superpowers/plans/2026-07-22-qingyu-mcp-quick-configuration.md
git commit -m "docs: explain MCP quick configuration"
```

After all gates pass, merge `codex/mcp-quick-config` into local `main` without staging, modifying, or discarding the primary checkout's existing `Cargo.toml`, `SyncSettings`, or `macos-icon.icns` changes.
