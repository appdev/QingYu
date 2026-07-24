import { act, renderHook, waitFor } from "@testing-library/react";
import type { SyncConfigDocument, SyncStatus } from "../lib/sync-config";
import { syncStatusChangedEvent } from "../lib/sync-config-events";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppEventsRuntime
} from "../runtime";
import { useCompactSyncSettings } from "./useCompactSyncSettings";

function document(revision = "rev-1"): SyncConfigDocument {
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
    revision
  };
}

function status(notesRoot: string, revision: string): SyncStatus {
  return {
    completionState: "succeeded",
    error: null,
    lastAttemptAt: "2030-01-01T00:00:00.000Z",
    lastSuccessfulSyncAt: "2030-01-01T00:00:00.000Z",
    lastTrigger: "manual",
    notebookName: notesRoot.split("/").at(-1) ?? "",
    notesRoot,
    provider: "webdav",
    revision,
    summary: {
      bytesDownloaded: 0,
      bytesUploaded: 0,
      conflictFiles: 0,
      downloadedFiles: 0,
      scannedFiles: 1,
      skippedFiles: 0,
      uploadedFiles: 0
    },
    version: 1
  };
}

describe("useCompactSyncSettings", () => {
  beforeEach(() => resetAppRuntimeForTests());
  afterEach(() => resetAppRuntimeForTests());

  it("uses the application config and filters status by primary root and current revision", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    let statusListener: ((event: { payload: unknown }) => unknown) | null = null;
    const listen: AppEventsRuntime["listen"] = async (event, handler) => {
      if (event === syncStatusChangedEvent) statusListener = handler as (event: { payload: unknown }) => unknown;
      return () => {
        statusListener = null;
      };
    };
    configureAppRuntime({
      ...defaultRuntime,
      events: { emit: vi.fn(async () => undefined), isAvailable: () => true, listen },
      syncConfig: {
        ...defaultRuntime.syncConfig,
        loadStatus: vi.fn(async () => status("/Other", "rev-1"))
      }
    });
    const observedLoadResult = { ...document(), status: "loaded" as const };
    const { result } = renderHook(() => useCompactSyncSettings({
      available: true,
      observedLoadResult,
      primaryRoot: "/Notes",
      shouldBegin: false
    }));

    await waitFor(() => expect(statusListener).not.toBeNull());
    expect(result.current.configDocument?.revision).toBe("rev-1");
    expect(result.current.status).toBeNull();

    act(() => {
      statusListener?.({
        payload: { notesRoot: "/Notes", revision: "stale-rev", status: status("/Notes", "stale-rev") }
      });
    });
    expect(result.current.status).toBeNull();

    act(() => {
      statusListener?.({
        payload: { notesRoot: "/Notes", revision: "rev-1", status: status("/Notes", "rev-1") }
      });
    });
    expect(result.current.status?.completionState).toBe("succeeded");
  });

  it("begins an app-level settings session even without a primary root", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    const load = vi.fn(async () => ({ ...document(), status: "loaded" as const }));
    configureAppRuntime({
      ...defaultRuntime,
      syncConfig: { ...defaultRuntime.syncConfig, load }
    });
    renderHook(() => useCompactSyncSettings({
      available: true,
      observedLoadResult: null,
      primaryRoot: null,
      shouldBegin: true
    }));

    await waitFor(() => expect(load).toHaveBeenCalledTimes(1));
  });
});
