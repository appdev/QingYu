import {
  applySyncConfigPatch,
  normalizeSyncConfigLoadResult,
  type AppSyncConfigRuntime,
  type QingYuSyncConfig,
  type SyncConfigLoadResult
} from "./sync-config";

describe("application sync config contract", () => {
  it("applies every supported field patch without mutating the source config", () => {
    const source: QingYuSyncConfig = {
      version: 2,
      enabled: false,
      provider: "webdav",
      remoteRoot: "qingyu",
      autoSyncOnSave: false,
      intervalMinutes: 0,
      webdav: { serverUrl: "https://dav.old", username: "old", password: "secret" },
      s3: {
        endpointUrl: "",
        region: "",
        bucket: "",
        accessKeyId: "",
        secretAccessKey: "",
        requestTimeoutSeconds: 60,
        addressingStyle: "auto",
        tlsVerification: "verify"
      }
    };

    const remote = applySyncConfigPatch(source, { field: "remoteRoot", value: "qingyu/team" });
    const webdav = applySyncConfigPatch(source, { field: "webdav.username", value: "new" });
    const s3 = applySyncConfigPatch(source, { field: "s3.bucket", value: "notes" });
    const timeout = applySyncConfigPatch(source, {
      field: "s3.requestTimeoutSeconds",
      value: 120
    });
    const addressing = applySyncConfigPatch(source, {
      field: "s3.addressingStyle",
      value: "path"
    });
    const tls = applySyncConfigPatch(source, {
      field: "s3.tlsVerification",
      value: "skip"
    });

    expect(remote.remoteRoot).toBe("qingyu/team");
    expect(webdav.webdav).toEqual({ ...source.webdav, username: "new" });
    expect(s3.s3).toEqual({ ...source.s3, bucket: "notes" });
    expect(timeout.s3).toEqual({ ...source.s3, requestTimeoutSeconds: 120 });
    expect(addressing.s3).toEqual({ ...source.s3, addressingStyle: "path" });
    expect(tls.s3).toEqual({ ...source.s3, tlsVerification: "skip" });
    expect(source).toEqual(expect.objectContaining({
      remoteRoot: "qingyu",
      webdav: expect.objectContaining({ username: "old" }),
      s3: expect.objectContaining({ bucket: "" })
    }));
  });

  it("keeps the config flat and free of project identity fields", () => {
    const config: QingYuSyncConfig = {
      version: 2,
      enabled: false,
      provider: "webdav",
      remoteRoot: "qingyu",
      autoSyncOnSave: false,
      intervalMinutes: 0,
      webdav: { serverUrl: "", username: "", password: "" },
      s3: {
        endpointUrl: "",
        region: "",
        bucket: "",
        accessKeyId: "",
        secretAccessKey: "",
        requestTimeoutSeconds: 60,
        addressingStyle: "auto",
        tlsVerification: "verify"
      }
    };

    expect(config).not.toHaveProperty("projectRoot");
    expect(config).not.toHaveProperty("rootPath");
    expect(config).not.toHaveProperty("sync");
  });

  it("normalizes invalid load states without exposing raw content", () => {
    const malformed: SyncConfigLoadResult = {
      status: "malformed",
      revision: "rev-invalid",
      issue: { code: "sync-config-malformed", message: "Invalid sync configuration." }
    };

    expect(normalizeSyncConfigLoadResult(malformed)).toEqual(malformed);
    expect(JSON.stringify(normalizeSyncConfigLoadResult(malformed))).not.toMatch(/password|secret/i);
  });

  it.each([
    ["complete", true],
    ["default or partial", false]
  ])("preserves the explicit configured flag for a %s disabled config", (_label, configured) => {
    const loaded = {
      config: {
        autoSyncOnSave: false,
        enabled: false,
        intervalMinutes: 0,
        provider: "webdav" as const,
        remoteRoot: configured ? "qingyu" : "",
        s3: {
          accessKeyId: "",
          bucket: "",
          endpointUrl: "",
          region: "",
          secretAccessKey: "",
          requestTimeoutSeconds: 60,
          addressingStyle: "auto" as const,
          tlsVerification: "verify" as const
        },
        version: 2 as const,
        webdav: {
          password: "",
          serverUrl: configured ? "https://dav.example.test" : "",
          username: ""
        }
      },
      configured,
      issues: [],
      readiness: "disabled" as const,
      revision: `rev-${configured}`,
      status: "loaded" as const
    };

    expect(normalizeSyncConfigLoadResult(loaded as SyncConfigLoadResult)).toMatchObject({
      configured,
      readiness: "disabled",
      status: "loaded"
    });
  });

  it("defines an app identity editing and apply API without project roots", () => {
    const editing: Parameters<AppSyncConfigRuntime["setEditing"]>[0] = {
      active: true,
      revision: "rev-1",
      sessionId: "settings-1"
    };
    const apply: Parameters<AppSyncConfigRuntime["requestApply"]>[0] = {
      exitReason: "category-leave",
      revision: "rev-2",
      sessionId: "settings-1",
      source: "settings-exit",
      token: "apply-1"
    };

    expect(editing).not.toHaveProperty("projectRoot");
    expect(apply).not.toHaveProperty("projectRoot");
  });
});
