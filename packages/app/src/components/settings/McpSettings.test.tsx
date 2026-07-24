import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { t } from "@markra/shared";
import type { AppMcpRuntime } from "../../runtime";
import { defaultMcpConfig, type McpSettingsSnapshot } from "../../lib/mcp";
import { listenMcpPolicyChanged, listenMcpRuntimeChanged } from "../../lib/settings/settings-events";
import { McpSettings } from "./McpSettings";

vi.mock("../../lib/settings/settings-events", () => ({
  listenMcpPolicyChanged: vi.fn(),
  listenMcpRuntimeChanged: vi.fn()
}));

const mockedListenMcpPolicyChanged = vi.mocked(listenMcpPolicyChanged);
const mockedListenMcpRuntimeChanged = vi.mocked(listenMcpRuntimeChanged);

function snapshot(): McpSettingsSnapshot {
  return {
    clientCommand: "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp",
    config: defaultMcpConfig(),
    endpoint: "local-ipc",
    health: { state: "disabled", endpoint: null, errorCode: null },
    revision: "revision-1",
    workspace: {
      available: true,
      displayName: "Notes",
      leafName: "Notes",
      workspaceId: "workspace",
      workspaceGeneration: 1
    }
  };
}

function runtime(overrides: Partial<AppMcpRuntime> = {}): AppMcpRuntime {
  return {
    clearAuditEntries: vi.fn(async () => undefined),
    getHealth: vi.fn(async () => snapshot().health),
    getSettings: vi.fn(async () => snapshot()),
    listAuditEntries: vi.fn(async () => []),
    localServiceAvailable: true,
    policyAvailable: true,
    setPrimaryWorkspace: vi.fn(async () => snapshot()),
    updateSettings: vi.fn(async ({ config }) => ({ ...snapshot(), config })),
    ...overrides
  };
}

const auditEntry = {
  requestId: "request",
  timestampMs: 1_721_234_567_000,
  tool: "document_read",
  workspaceId: "workspace",
  workspaceDisplayName: "Notes",
  logicalTarget: "draft.md",
  dryRun: false,
  confirmation: "allowed" as const,
  outcome: "succeeded" as const,
  errorCode: null,
  revisionBefore: "before",
  revisionAfter: "after",
  syncRunId: null,
  durationMs: 12,
  counts: { documents: 1 }
};

describe("McpSettings", () => {
  beforeEach(() => {
    mockedListenMcpPolicyChanged.mockReset();
    mockedListenMcpPolicyChanged.mockResolvedValue(() => undefined);
    mockedListenMcpRuntimeChanged.mockReset();
    mockedListenMcpRuntimeChanged.mockResolvedValue(() => undefined);
  });

  it("keeps the application policy free of paths, ports, and credentials", () => {
    const config = defaultMcpConfig() as unknown as Record<string, unknown>;

    expect(config).not.toHaveProperty("port");
    expect(config).not.toHaveProperty("workspaces");
    expect(config).not.toHaveProperty("token");
  });

  it("renders application authorization, policy, IPC, and audit controls without secrets", async () => {
    render(<McpSettings runtime={runtime()} />);

    expect(await screen.findByText("local-ipc")).toBeInTheDocument();
    expect(screen.getByText("Notes")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Enable MCP" })).toBeEnabled();
    expect(screen.getByRole("checkbox", { name: "Read documents" })).not.toBeChecked();
    for (const label of [
      "Enable MCP",
      "Read documents",
      "Write documents",
      "Move documents",
      "Delete documents",
      "Read settings",
      "Write settings",
      "Read sync configuration",
      "Write sync configuration",
      "Write sync credentials",
      "Run synchronization",
      "Confirmation policy",
      "Dry-run policy",
      "Deletion policy",
      "Sync after document writes",
      "Sync execution",
      "Audit log"
    ]) {
      expect(screen.getAllByText(label).length).toBeGreaterThan(0);
    }
    for (const removedLabel of ["Bearer Token", "Copy Bearer Token", "Port", "Add authorized directory"]) {
      expect(screen.queryByText(removedLabel)).not.toBeInTheDocument();
    }
  });

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

  it("updates application policy by revision", async () => {
    const mcp = runtime();
    render(<McpSettings runtime={mcp} />);
    fireEvent.click(await screen.findByRole("button", { name: "Enable MCP" }));
    await waitFor(() => expect(mcp.updateSettings).toHaveBeenCalled());
  });

  it("offers preset cleanup periods only for the QingYu recycle bin", async () => {
    const mcp = runtime();
    render(<McpSettings runtime={mcp} />);

    expect(await screen.findByRole("button", { name: "Enable MCP" })).toBeEnabled();
    expect(screen.queryByRole("combobox", { name: "Recycle bin cleanup" })).not.toBeInTheDocument();

    fireEvent.change(screen.getByRole("combobox", { name: "Deletion policy" }), {
      target: { value: "qing-yu-recycle-bin" }
    });

    const cleanup = await screen.findByRole("combobox", { name: "Recycle bin cleanup" });
    expect(cleanup).toHaveValue("30");
    expect(Array.from((cleanup as HTMLSelectElement).options).map((option) => option.textContent)).toEqual([
      "Never automatically clean up",
      "After 7 days",
      "After 30 days",
      "After 90 days"
    ]);

    fireEvent.change(cleanup, { target: { value: "0" } });

    await waitFor(() => expect(mcp.updateSettings).toHaveBeenLastCalledWith(expect.objectContaining({
      config: expect.objectContaining({ recycleBinRetentionDays: 0 }),
      expectedRevision: "revision-1"
    })));
  });

  it("shows quick client configuration only after desktop MCP is enabled", async () => {
    const mcp = runtime();
    render(<McpSettings runtime={mcp} />);

    expect(await screen.findByRole("button", { name: "Enable MCP" })).toBeEnabled();
    expect(screen.queryByRole("heading", { name: "Client connection" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Enable MCP" }));

    expect(await screen.findByRole("heading", { name: "Client connection" })).toBeInTheDocument();
    expect(screen.getByText(/mcp_servers\.qingyu/u)).toBeInTheDocument();
  });

  it("never shows client configuration on a policy-only device", async () => {
    const enabled = {
      ...snapshot(),
      clientCommand: null,
      config: { ...defaultMcpConfig(), enabled: true }
    };
    const mcp = runtime({
      getSettings: vi.fn(async () => enabled),
      localServiceAvailable: false
    });

    render(<McpSettings compact runtime={mcp} />);

    expect(await screen.findByRole("button", { name: "Disable MCP" })).toBeEnabled();
    expect(screen.queryByRole("heading", { name: "Client connection" })).not.toBeInTheDocument();
  });

  it("keeps settings usable when the bundled bridge path is unavailable", async () => {
    const enabled = {
      ...snapshot(),
      clientCommand: null,
      config: { ...defaultMcpConfig(), enabled: true }
    };
    const mcp = runtime({ getSettings: vi.fn(async () => enabled) });

    render(<McpSettings runtime={mcp} />);

    expect(await screen.findByRole("heading", { name: "Client connection" })).toBeInTheDocument();
    expect(screen.getByRole("note")).toHaveTextContent(/bridge path is unavailable/u);
    expect(screen.getByRole("button", { name: "Disable MCP" })).toBeEnabled();
  });

  it("renders only redacted audit fields and pages at one hundred entries", async () => {
    const mcp = runtime();
    vi.mocked(mcp.listAuditEntries).mockResolvedValue([auditEntry]);
    render(<McpSettings runtime={mcp} />);

    expect(await screen.findByText("document_read")).toBeInTheDocument();
    expect(screen.getAllByText("Notes").length).toBeGreaterThan(0);
    expect(screen.getByText("draft.md")).toBeInTheDocument();
    expect(screen.getByText("12 ms")).toBeInTheDocument();
    expect(mcp.listAuditEntries).toHaveBeenCalledWith(0, 100);
    expect(screen.queryByText(/Bearer secret|document body|absolute path/i)).not.toBeInTheDocument();
  });

  it("reloads after a revision conflict and shows a localized conflict message", async () => {
    const mcp = runtime();
    vi.mocked(mcp.updateSettings).mockRejectedValue(new Error("revision-conflict"));
    render(<McpSettings runtime={mcp} />);

    fireEvent.click(await screen.findByRole("button", { name: "Enable MCP" }));
    await waitFor(() => expect(mcp.getSettings).toHaveBeenCalledTimes(2));
    expect(screen.getByRole("alert")).toHaveTextContent("MCP settings changed elsewhere. Reloaded the latest values.");
  });

  it("shows server health failures without exposing transport details", async () => {
    const mcp = runtime();
    vi.mocked(mcp.getSettings).mockResolvedValue({
      ...snapshot(),
      health: { state: "error", endpoint: null, errorCode: "bind_failed" }
    });
    render(<McpSettings runtime={mcp} />);

    expect(await screen.findByText("MCP could not start.")).toBeInTheDocument();
    expect(screen.getByText("bind_failed")).toBeInTheDocument();
  });

  it("requires local confirmation before clearing the audit log", async () => {
    const mcp = runtime();
    const confirmAction = vi.fn(async () => false);
    render(<McpSettings runtime={mcp} confirmAction={confirmAction} />);

    fireEvent.click(await screen.findByRole("button", { name: "Clear audit log" }));
    await waitFor(() => expect(confirmAction).toHaveBeenCalled());
    expect(mcp.clearAuditEntries).not.toHaveBeenCalled();

    confirmAction.mockResolvedValue(true);
    fireEvent.click(screen.getByRole("button", { name: "Clear audit log" }));
    await waitFor(() => expect(mcp.clearAuditEntries).toHaveBeenCalledTimes(1));
  });

  it("allows policy edits without a current notebook and labels document tools unavailable", async () => {
    const mcp = runtime();
    vi.mocked(mcp.getSettings).mockResolvedValue({ ...snapshot(), workspace: null });

    render(<McpSettings runtime={mcp} translate={(key) => t("zh-CN", key)} />);

    expect(await screen.findByText("尚未选择笔记目录")).toBeInTheDocument();
    expect(screen.getByText(/文档工具不可用/u)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "启用 MCP" }));
    await waitFor(() => expect(mcp.updateSettings).toHaveBeenCalledWith(expect.objectContaining({
      config: expect.objectContaining({ enabled: true }),
      expectedRevision: "revision-1"
    })));
  });

  it("refreshes an open settings surface when the current notebook changes or clears", async () => {
    let runtimeChanged: (() => unknown) | undefined;
    mockedListenMcpRuntimeChanged.mockImplementation(async (listener) => {
      runtimeChanged = listener;
      return () => undefined;
    });
    const mcp = runtime();
    vi.mocked(mcp.getSettings)
      .mockResolvedValueOnce({
        ...snapshot(),
        workspace: { ...snapshot().workspace!, displayName: "Notes A", leafName: "Notes A" }
      })
      .mockResolvedValueOnce({
        ...snapshot(),
        workspace: {
          ...snapshot().workspace!,
          displayName: "Notes B",
          leafName: "Notes B",
          workspaceGeneration: 2
        }
      })
      .mockResolvedValueOnce({ ...snapshot(), workspace: null });

    render(<McpSettings runtime={mcp} />);
    expect(await screen.findByText("Notes A")).toBeInTheDocument();

    act(() => {
      runtimeChanged?.();
    });
    expect(await screen.findByText("Notes B")).toBeInTheDocument();
    expect(screen.queryByText("Notes A")).not.toBeInTheDocument();

    act(() => {
      runtimeChanged?.();
    });
    expect(await screen.findByText("No notebook directory selected")).toBeInTheDocument();
    expect(screen.getByText(/Document tools are unavailable/u)).toBeInTheDocument();
    expect(mcp.getSettings).toHaveBeenCalledTimes(3);
  });

  it("shows portable policy on mobile without fake endpoint, health, or runtime audit controls", async () => {
    const mcp = runtime({ localServiceAvailable: false });
    render(<McpSettings compact runtime={mcp} />);

    const enable = await screen.findByRole("button", { name: "Enable MCP" });
    expect(enable).toHaveClass("min-h-11", "min-w-11");
    expect(screen.getByText(/does not run a local MCP service/u)).toBeInTheDocument();
    expect(screen.queryByText("local-ipc")).not.toBeInTheDocument();
    expect(screen.queryByText("Health")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Clear audit log" })).not.toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    expect(mcp.listAuditEntries).not.toHaveBeenCalled();

    fireEvent.click(enable);
    await waitFor(() => expect(mcp.updateSettings).toHaveBeenCalled());
  });
});
