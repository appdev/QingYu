import {
  listenPrimaryCloudNotebookCatalogRequested,
  primaryCloudNotebookCatalogRequestedEvent,
  requestPrimaryCloudNotebookCatalog
} from "./cloud-notebook-catalog-events";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests
} from "../runtime";

describe("primary cloud notebook catalog requests", () => {
  afterEach(() => resetAppRuntimeForTests());

  it("requests the primary window without carrying notebook or sync data", async () => {
    const runtime = createDefaultAppRuntime();
    const request = vi.fn(async () => undefined);
    configureAppRuntime({
      ...runtime,
      window: {
        ...runtime.window,
        requestPrimaryCloudNotebookCatalog: request
      }
    });

    await expect(requestPrimaryCloudNotebookCatalog()).resolves.toBeUndefined();

    expect(request).toHaveBeenCalledOnce();
    expect(request).toHaveBeenCalledWith();
  });

  it("owns the exact event name and accepts only a unit payload", async () => {
    const runtime = createDefaultAppRuntime();
    let listener: ((event: { payload: unknown }) => unknown) | null = null;
    const listen = vi.fn(async (_event, next) => {
      listener = next as (event: { payload: unknown }) => unknown;
      return () => {
        listener = null;
      };
    });
    configureAppRuntime({
      ...runtime,
      events: {
        ...runtime.events,
        isAvailable: () => true,
        listen
      }
    });
    const received = vi.fn();

    await listenPrimaryCloudNotebookCatalogRequested(received);
    (listener as ((event: { payload: unknown }) => unknown) | null)?.({ payload: {} });
    (listener as ((event: { payload: unknown }) => unknown) | null)?.({ payload: undefined });
    (listener as ((event: { payload: unknown }) => unknown) | null)?.({ payload: null });

    expect(primaryCloudNotebookCatalogRequestedEvent).toBe(
      ["qingyu://cloud", "-notebook-catalog-requested"].join("")
    );
    expect(listen).toHaveBeenCalledWith(
      primaryCloudNotebookCatalogRequestedEvent,
      expect.any(Function)
    );
    expect(received).toHaveBeenCalledOnce();
    expect(received).toHaveBeenCalledWith();
  });

  it("does not claim to subscribe when runtime events are unavailable", async () => {
    const runtime = createDefaultAppRuntime();
    const listen = vi.fn();
    configureAppRuntime({
      ...runtime,
      events: {
        ...runtime.events,
        listen: listen as typeof runtime.events.listen
      }
    });
    const received = vi.fn();

    const cleanup = await listenPrimaryCloudNotebookCatalogRequested(received);

    expect(cleanup()).toBeUndefined();
    expect(listen).not.toHaveBeenCalled();
    expect(received).not.toHaveBeenCalled();
  });
});
