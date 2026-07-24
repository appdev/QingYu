import { act, renderHook, waitFor } from "@testing-library/react";
import { defaultMcpConfig, type McpSettingsSnapshot } from "../lib/mcp";
import { listenMcpPolicyChanged, listenMcpRuntimeChanged } from "../lib/settings/settings-events";
import type { AppMcpRuntime } from "../runtime";
import { useMcpSettings } from "./useMcpSettings";

vi.mock("../lib/settings/settings-events", () => ({
  listenMcpPolicyChanged: vi.fn(),
  listenMcpRuntimeChanged: vi.fn()
}));

const mockedListenMcpPolicyChanged = vi.mocked(listenMcpPolicyChanged);
const mockedListenMcpRuntimeChanged = vi.mocked(listenMcpRuntimeChanged);

function deferred<TValue>() {
  let resolve!: (value: TValue) => void;
  const promise = new Promise<TValue>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function snapshot(revision = "revision-1"): McpSettingsSnapshot {
  return {
    clientCommand: "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp",
    config: defaultMcpConfig(),
    endpoint: "local-ipc",
    health: { state: "disabled", endpoint: null, errorCode: null },
    revision,
    workspace: null
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
    updateSettings: vi.fn(async ({ config }) => ({ ...snapshot("revision-2"), config })),
    ...overrides
  };
}

describe("useMcpSettings", () => {
  beforeEach(() => {
    mockedListenMcpPolicyChanged.mockReset();
    mockedListenMcpPolicyChanged.mockResolvedValue(() => undefined);
    mockedListenMcpRuntimeChanged.mockReset();
    mockedListenMcpRuntimeChanged.mockResolvedValue(() => undefined);
  });

  it("subscribes to policy and runtime changes before the initial snapshot read", async () => {
    const order: string[] = [];
    mockedListenMcpPolicyChanged.mockImplementation(async () => {
      order.push("policy-listener");
      return () => undefined;
    });
    mockedListenMcpRuntimeChanged.mockImplementation(async () => {
      order.push("runtime-listener");
      return () => undefined;
    });
    const mcp = runtime({
      getSettings: vi.fn(async () => {
        order.push("read");
        return snapshot();
      })
    });

    renderHook(() => useMcpSettings(mcp));

    await waitFor(() => expect(mcp.getSettings).toHaveBeenCalledTimes(1));
    expect(order.indexOf("read")).toBeGreaterThan(order.indexOf("policy-listener"));
    expect(order.indexOf("read")).toBeGreaterThan(order.indexOf("runtime-listener"));
  });

  it("keeps the newest snapshot when an older reload resolves last", async () => {
    let runtimeChanged: (() => unknown) | undefined;
    mockedListenMcpRuntimeChanged.mockImplementation(async (listener) => {
      runtimeChanged = listener;
      return () => undefined;
    });
    const oldRequest = deferred<McpSettingsSnapshot>();
    const newRequest = deferred<McpSettingsSnapshot>();
    const mcp = runtime();
    vi.mocked(mcp.getSettings)
      .mockImplementationOnce(() => oldRequest.promise)
      .mockImplementationOnce(() => newRequest.promise);

    const { result } = renderHook(() => useMcpSettings(mcp));
    await waitFor(() => expect(mcp.getSettings).toHaveBeenCalledTimes(1));
    act(() => {
      runtimeChanged?.();
    });
    await waitFor(() => expect(mcp.getSettings).toHaveBeenCalledTimes(2));

    await act(async () => {
      newRequest.resolve(snapshot("revision-new"));
      await newRequest.promise;
    });
    expect(result.current.snapshot?.revision).toBe("revision-new");

    await act(async () => {
      oldRequest.resolve(snapshot("revision-old"));
      await oldRequest.promise;
    });
    expect(result.current.snapshot?.revision).toBe("revision-new");
  });

  it("cleans up late listener installation without reading after unmount", async () => {
    const listenerInstallation = deferred<() => unknown>();
    const cleanup = vi.fn();
    mockedListenMcpRuntimeChanged.mockImplementation(() => listenerInstallation.promise);
    const mcp = runtime();

    const hook = renderHook(() => useMcpSettings(mcp));
    hook.unmount();
    listenerInstallation.resolve(cleanup);
    await listenerInstallation.promise;
    await act(async () => Promise.resolve());

    expect(cleanup).toHaveBeenCalledTimes(1);
    expect(mcp.getSettings).not.toHaveBeenCalled();
  });

  it("loads the initial snapshot after cleaning up a partial subscription", async () => {
    const cleanup = vi.fn();
    mockedListenMcpPolicyChanged.mockResolvedValue(cleanup);
    mockedListenMcpRuntimeChanged.mockRejectedValue(new Error("runtime listener unavailable"));
    const mcp = runtime();

    const { result } = renderHook(() => useMcpSettings(mcp));

    await waitFor(() => expect(cleanup).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(result.current.snapshot?.revision).toBe("revision-1"));
    expect(mcp.getSettings).toHaveBeenCalledTimes(1);
  });

  it("reloads the revisioned application policy after synchronized settings change", async () => {
    let changed: (() => unknown) | undefined;
    mockedListenMcpPolicyChanged.mockImplementation(async (listener) => {
      changed = () => listener({ ...defaultMcpConfig(), enabled: true });
      return () => undefined;
    });
    const mcp = runtime();
    vi.mocked(mcp.getSettings)
      .mockResolvedValueOnce(snapshot("revision-1"))
      .mockResolvedValueOnce({
        ...snapshot("revision-2"),
        config: { ...defaultMcpConfig(), enabled: true }
      });

    const { result } = renderHook(() => useMcpSettings(mcp));
    await waitFor(() => expect(result.current.snapshot?.revision).toBe("revision-1"));

    await act(async () => {
      changed?.();
      await Promise.resolve();
    });

    await waitFor(() => expect(result.current.snapshot?.revision).toBe("revision-2"));
    expect(mcp.getSettings).toHaveBeenCalledTimes(2);
  });

  it("does not load or subscribe when portable MCP policy is unavailable", async () => {
    const mcp = runtime({ policyAvailable: false });

    const { result } = renderHook(() => useMcpSettings(mcp));
    await act(async () => Promise.resolve());

    expect(result.current.loading).toBe(false);
    expect(mcp.getSettings).not.toHaveBeenCalled();
    expect(mockedListenMcpPolicyChanged).not.toHaveBeenCalled();
    expect(mockedListenMcpRuntimeChanged).not.toHaveBeenCalled();
  });

  it("reloads when the application settings service reports its native revision conflict code", async () => {
    const mcp = runtime();
    vi.mocked(mcp.updateSettings).mockRejectedValue(new Error("settings_revision_conflict"));
    const { result } = renderHook(() => useMcpSettings(mcp));
    await waitFor(() => expect(result.current.snapshot?.revision).toBe("revision-1"));

    await act(async () => {
      await result.current.updateConfig({ ...defaultMcpConfig(), enabled: true });
    });

    expect(mcp.getSettings).toHaveBeenCalledTimes(2);
    expect(result.current.error).toContain("settings_revision_conflict");
  });

  it("does not restore an obsolete revision-conflict error after a newer event reload", async () => {
    let runtimeChanged: (() => unknown) | undefined;
    mockedListenMcpRuntimeChanged.mockImplementation(async (listener) => {
      runtimeChanged = listener;
      return () => undefined;
    });
    const conflictReload = deferred<McpSettingsSnapshot>();
    const eventReload = deferred<McpSettingsSnapshot>();
    const mcp = runtime();
    vi.mocked(mcp.getSettings)
      .mockResolvedValueOnce(snapshot("revision-1"))
      .mockImplementationOnce(() => conflictReload.promise)
      .mockImplementationOnce(() => eventReload.promise);
    vi.mocked(mcp.updateSettings).mockRejectedValue(new Error("settings_revision_conflict"));
    const { result } = renderHook(() => useMcpSettings(mcp));
    await waitFor(() => expect(result.current.snapshot?.revision).toBe("revision-1"));

    let updatePromise: Promise<McpSettingsSnapshot | null> | undefined;
    act(() => {
      updatePromise = result.current.updateConfig({ ...defaultMcpConfig(), enabled: true });
    });
    await waitFor(() => expect(mcp.getSettings).toHaveBeenCalledTimes(2));

    act(() => {
      runtimeChanged?.();
    });
    await waitFor(() => expect(mcp.getSettings).toHaveBeenCalledTimes(3));
    await act(async () => {
      eventReload.resolve(snapshot("revision-new"));
      await eventReload.promise;
    });
    expect(result.current.snapshot?.revision).toBe("revision-new");
    expect(result.current.error).toBeNull();

    await act(async () => {
      conflictReload.resolve(snapshot("revision-old"));
      await updatePromise;
    });
    expect(result.current.snapshot?.revision).toBe("revision-new");
    expect(result.current.error).toBeNull();
  });
});
