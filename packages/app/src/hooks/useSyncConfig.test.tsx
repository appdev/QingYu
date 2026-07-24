import { act, renderHook, waitFor } from "@testing-library/react";
import type { AppSyncConfigRuntime, SyncConfigLoadResult } from "../lib/sync-config";
import { emitSyncConfigChanged } from "../lib/sync-config-events";
import { configureAppRuntime, createDefaultAppRuntime, resetAppRuntimeForTests } from "../runtime";
import { useSyncConfig } from "./useSyncConfig";

function loaded(revision: string, password: string): SyncConfigLoadResult {
  return {
    config: {
      autoSyncOnSave: true,
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
      webdav: {
        password,
        serverUrl: "https://dav.example.test",
        username: "writer"
      }
    },
    configured: true,
    issues: [],
    readiness: "ready",
    revision,
    status: "loaded"
  };
}

function deferred<T>() {
  let resolve!: (value: T) => undefined;
  const promise = new Promise<T>((complete) => {
    resolve = (value) => {
      complete(value);
      return undefined;
    };
  });
  return { promise, resolve };
}

describe("useSyncConfig", () => {
  const load = vi.fn<AppSyncConfigRuntime["load"]>();

  beforeEach(() => {
    load.mockReset();
    const runtime = createDefaultAppRuntime();
    const listeners = new Map<string, Set<(event: { payload: unknown }) => unknown>>();
    configureAppRuntime({
      ...runtime,
      events: {
        emit: async (event, payload) => {
          for (const listener of listeners.get(event) ?? []) listener({ payload });
        },
        isAvailable: () => true,
        listen: async (event, listener) => {
          const registered = listeners.get(event) ?? new Set();
          registered.add(listener as (event: { payload: unknown }) => unknown);
          listeners.set(event, registered);
          return () => registered.delete(listener as (event: { payload: unknown }) => unknown);
        }
      },
      syncConfig: { ...runtime.syncConfig, load }
    });
  });

  afterEach(() => resetAppRuntimeForTests());

  it("loads the one application configuration and reloads only for a new revision", async () => {
    load.mockResolvedValueOnce(loaded("rev-1", "secret-1"));
    load.mockResolvedValueOnce(loaded("rev-2", "secret-2"));
    const { result } = renderHook(() => useSyncConfig());

    await waitFor(() => expect(result.current.appliedDocument?.revision).toBe("rev-1"));
    await act(() => emitSyncConfigChanged({ revision: "rev-1" }));
    expect(load).toHaveBeenCalledTimes(1);

    await act(() => emitSyncConfigChanged({ revision: "rev-2" }));
    await waitFor(() => expect(result.current.appliedDocument?.revision).toBe("rev-2"));
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("does not let a slower prior reload replace the newest revision", async () => {
    const stale = deferred<SyncConfigLoadResult>();
    const fresh = deferred<SyncConfigLoadResult>();
    load.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);
    const { result } = renderHook(() => useSyncConfig());

    await waitFor(() => expect(load).toHaveBeenCalledTimes(1));
    await act(() => emitSyncConfigChanged({ revision: "rev-2" }));
    await act(async () => {
      stale.resolve(loaded("rev-1", "secret-1"));
      await stale.promise;
    });
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    await act(async () => {
      fresh.resolve(loaded("rev-2", "secret-2"));
      await fresh.promise;
    });

    expect(result.current.appliedDocument?.revision).toBe("rev-2");
  });

  it("stays idle without loading application config while the window is inactive", async () => {
    load.mockResolvedValue(loaded("rev-active", "secret"));
    const { result, rerender } = renderHook(
      ({ active }) => useSyncConfig({ active }),
      { initialProps: { active: false } }
    );

    await act(async () => Promise.resolve());
    expect(result.current.status).toBe("idle");
    expect(load).not.toHaveBeenCalled();

    rerender({ active: true });
    await waitFor(() => expect(result.current.appliedDocument?.revision).toBe("rev-active"));
    expect(load).toHaveBeenCalledTimes(1);
  });
});
