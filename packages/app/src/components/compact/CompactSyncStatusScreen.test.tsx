import { fireEvent, render, screen } from "@testing-library/react";
import type { CompactNavigation } from "../../hooks/useCompactNavigation";
import type { CompactSyncSettingsController } from "../../hooks/useCompactSyncSettings";
import type { SyncConfigDocument } from "../../lib/sync-config";
import { CompactSyncStatusScreen } from "./CompactSyncStatusScreen";

function document(): SyncConfigDocument {
  return {
    config: {
      autoSyncOnSave: false,
      enabled: true,
      intervalMinutes: 0,
      provider: "webdav",
      remoteRoot: "qingyu",
      s3: {
        accessKeyId: "",
        bucket: "",
        endpointUrl: "",
        region: "",
        secretAccessKey: "",
        requestTimeoutSeconds: 60,
        addressingStyle: "auto",
        tlsVerification: "verify"
      },
      version: 2,
      webdav: { password: "", serverUrl: "https://dav.example.test", username: "" }
    },
    configured: true,
    issues: [],
    readiness: "ready",
    revision: "rev-1"
  };
}

function navigation(): CompactNavigation {
  return {
    canGoBack: true,
    page: { kind: "sync-status" },
    pop: vi.fn(async () => true),
    popIfCurrent: vi.fn(async () => true),
    popToEditor: vi.fn(async () => true),
    push: vi.fn(() => true),
    replace: vi.fn(() => true),
    stack: [{ kind: "editor" }, { kind: "sync-status" }]
  };
}

function controller(overrides: Partial<CompactSyncSettingsController> = {}): CompactSyncSettingsController {
  const configDocument = document();
  return {
    available: true,
    begin: vi.fn(async () => undefined),
    configDocument,
    dirty: false,
    enable: vi.fn(async () => undefined),
    end: vi.fn(async () => undefined),
    loadResult: { ...configDocument, status: "loaded" },
    patch: vi.fn(async () => undefined),
    primaryRoot: "/Notes",
    recover: vi.fn(async () => undefined),
    reset: vi.fn(async () => undefined),
    runImmediate: vi.fn(async () => undefined),
    saving: false,
    sessionId: null,
    status: null,
    syncRunning: false,
    testConnection: vi.fn(async () => undefined),
    testing: false,
    ...overrides
  };
}

describe("CompactSyncStatusScreen application scope", () => {
  it("allows configuration but not synchronization when there is no primary notes workspace", () => {
    const setup = controller({ primaryRoot: null });
    const compactNavigation = navigation();
    render(<CompactSyncStatusScreen controller={setup} language="en" navigation={compactNavigation} />);

    expect(screen.getByText("No notebook selected")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Sync Now" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "sync-form", mode: "edit" });
  });

  it.each([
    [{ revision: null, status: "absent" } as const, "create" as const],
    [{ issue: { code: "bad", message: "bad" }, revision: "rev-bad", status: "malformed" } as const, "recover" as const],
    [{ issue: { code: "new", message: "new" }, revision: "rev-new", status: "unsupported", version: 2 } as const, "recover" as const]
  ])("chooses the %s configuration path without a primary workspace", (loadResult, mode) => {
    const setup = controller({ configDocument: null, loadResult, primaryRoot: null });
    const compactNavigation = navigation();
    render(<CompactSyncStatusScreen controller={setup} language="en" navigation={compactNavigation} />);

    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "sync-form", mode });
  });

  it("does not expose configuration entry while application sync config is loading", () => {
    render(<CompactSyncStatusScreen
      controller={controller({ configDocument: null, loadResult: null, primaryRoot: null })}
      language="en"
      navigation={navigation()}
    />);

    expect(screen.getByRole("status")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Configure Sync" })).not.toBeInTheDocument();
  });

  it("runs the shared application sync for a ready primary workspace", () => {
    const setup = controller();
    render(<CompactSyncStatusScreen controller={setup} language="en" navigation={navigation()} />);

    fireEvent.click(screen.getByRole("button", { name: "Sync Now" }));
    expect(setup.runImmediate).toHaveBeenCalledTimes(1);
    expect(screen.getByText(/WebDAV/)).toBeVisible();
  });

  it("allows retrying a manual sync after the previous attempt failed", () => {
    const configDocument = document();
    const setup = controller({
      status: {
        completionState: "failed",
        error: null,
        lastAttemptAt: "2026-07-20T00:00:00Z",
        lastSuccessfulSyncAt: null,
        lastTrigger: "manual",
        notebookName: "Notes",
        notesRoot: "/Notes",
        provider: "webdav",
        revision: configDocument.revision,
        summary: null,
        version: 1
      }
    });
    render(<CompactSyncStatusScreen controller={setup} language="en" navigation={navigation()} />);

    fireEvent.click(screen.getByRole("button", { name: "Sync Now" }));
    expect(setup.runImmediate).toHaveBeenCalledTimes(1);
  });
});
