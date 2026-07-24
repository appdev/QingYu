import { appLogger } from "@markra/app/runtime";
import { invokeNative } from "./invoke";
import {
  enableNativeSyncConfig,
  listNativeNotebooks,
  loadNativeSyncConfig,
  patchNativeSyncConfig,
  recoverNativeSyncConfig,
  requestNativeSyncConfigApply,
  resetNativeSyncConfig,
  setNativeSyncConfigEditing,
  syncApplication,
  testSyncConnection
} from "./sync-config";

vi.mock("./invoke", () => ({ invokeNative: vi.fn() }));

const mockedInvoke = vi.mocked(invokeNative);

describe("native application sync config runtime", () => {
  beforeEach(() => mockedInvoke.mockReset());

  it("maps config mutations without a project or notes root", async () => {
    mockedInvoke.mockResolvedValue({});
    const config = {
      version: 2 as const,
      enabled: true,
      provider: "webdav" as const,
      remoteRoot: "qingyu",
      autoSyncOnSave: false,
      intervalMinutes: 0,
      webdav: { serverUrl: "https://dav.example.test", username: "writer", password: "password" },
      s3: {
        endpointUrl: "",
        region: "",
        bucket: "",
        accessKeyId: "",
        secretAccessKey: "",
        requestTimeoutSeconds: 60,
        addressingStyle: "auto" as const,
        tlsVerification: "verify" as const
      }
    };

    await loadNativeSyncConfig();
    await listNativeNotebooks({ revision: "rev-1" });
    await enableNativeSyncConfig({ expectedRevision: null });
    await patchNativeSyncConfig({
      expectedRevision: "rev-1",
      patch: { field: "remoteRoot", value: "qingyu/team" }
    });
    await recoverNativeSyncConfig({ config, expectedRevision: "bad-rev" });
    await resetNativeSyncConfig({ confirmed: true, expectedRevision: "bad-rev" });
    await setNativeSyncConfigEditing({ active: true, revision: "rev-1", sessionId: "settings-1" });
    await requestNativeSyncConfigApply({
      exitReason: "window-close",
      revision: "rev-2",
      sessionId: "settings-1",
      source: "settings-exit",
      token: "apply-1"
    });

    expect(mockedInvoke.mock.calls).toEqual([
      ["load_sync_config"],
      ["list_remote_notebooks", { request: { revision: "rev-1" } }],
      ["enable_sync_config", { expectedRevision: null }],
      ["patch_sync_config", { request: {
        expectedRevision: "rev-1",
        patch: { field: "remoteRoot", value: "qingyu/team" }
      } }],
      ["recover_sync_config", { request: { config, expectedRevision: "bad-rev" } }],
      ["reset_sync_config", { request: { confirmed: true, expectedRevision: "bad-rev" } }],
      ["set_sync_config_editing", { request: {
        active: true,
        revision: "rev-1",
        sessionId: "settings-1"
      } }],
      ["request_sync_config_apply", { request: {
        exitReason: "window-close",
        revision: "rev-2",
        sessionId: "settings-1",
        source: "settings-exit",
        token: "apply-1"
      } }]
    ]);
    expect(JSON.stringify(mockedInvoke.mock.calls)).not.toMatch(/projectRoot|rootPath|notesRoot/);
    expect(JSON.stringify(mockedInvoke.mock.calls[1])).not.toMatch(
      /serverUrl|username|password|endpointUrl|accessKeyId|secretAccessKey/
    );
  });

  it("maps sync and connection testing to the application native commands", async () => {
    mockedInvoke.mockResolvedValue({
      notebookName: "Notes",
      notesRoot: "/Notes",
      provider: "webdav",
      revision: "rev-1",
      summary: {
        bytesDownloaded: 0,
        bytesUploaded: 0,
        conflictFiles: 0,
        downloadedFiles: 0,
        scannedFiles: 0,
        skippedFiles: 0,
        uploadedFiles: 0
      },
      trigger: "manual"
    });
    await syncApplication({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "manual"
    });
    await testSyncConnection({ revision: "rev-1" });
    expect(mockedInvoke.mock.calls).toEqual([
      ["sync_application", { request: {
        notebookName: "Notes",
        notesRoot: "/Notes",
        revision: "rev-1",
        trigger: "manual"
      } }],
      ["test_sync_connection", { request: { revision: "rev-1" } }]
    ]);
  });

  it("logs a successful synchronization summary without notebook paths", async () => {
    const logSpy = vi.spyOn(appLogger, "info").mockImplementation(() => ({
      area: "sync",
      details: {},
      level: "info",
      message: "Application synchronization completed",
      timestamp: "2026-07-23T01:53:04Z"
    }));
    mockedInvoke.mockResolvedValue({
      notebookName: "Private Notes",
      notesRoot: "/Users/example/Private Notes",
      provider: "s3",
      revision: "rev-safe",
      summary: {
        bytesDownloaded: 2,
        bytesUploaded: 4,
        conflictFiles: 0,
        downloadedFiles: 1,
        scannedFiles: 3,
        skippedFiles: 0,
        uploadedFiles: 1
      },
      trigger: "manual"
    });

    await syncApplication({
      notebookName: "Private Notes",
      notesRoot: "/Users/example/Private Notes",
      revision: "rev-safe",
      trigger: "manual"
    });

    expect(logSpy).toHaveBeenCalledWith("sync", "Application synchronization completed", {
      bytesDownloaded: 2,
      bytesUploaded: 4,
      conflictFiles: 0,
      downloadedFiles: 1,
      provider: "s3",
      revision: "rev-safe",
      scannedFiles: 3,
      skippedFiles: 0,
      trigger: "manual",
      uploadedFiles: 1
    });
    expect(JSON.stringify(logSpy.mock.calls)).not.toContain("/Users/example");
    expect(JSON.stringify(logSpy.mock.calls)).not.toContain("Private Notes");
    logSpy.mockRestore();
  });

  it("rejects a native run result whose immutable notebook identity changed", async () => {
    mockedInvoke.mockResolvedValue({
      notebookName: "Other",
      notesRoot: "/Notes",
      provider: "webdav",
      revision: "rev-1",
      summary: {
        bytesDownloaded: 0,
        bytesUploaded: 0,
        conflictFiles: 0,
        downloadedFiles: 0,
        scannedFiles: 0,
        skippedFiles: 0,
        uploadedFiles: 0
      },
      trigger: "manual"
    });

    await expect(syncApplication({
      notebookName: "Notes",
      notesRoot: "/Notes",
      revision: "rev-1",
      trigger: "manual"
    })).rejects.toThrow("sync-result-mismatch");
  });
});
