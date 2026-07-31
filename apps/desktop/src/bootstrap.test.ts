import {
  bootstrapApplication,
  bootstrapApplicationMount
} from "./bootstrap";

describe("application bootstrap", () => {
  it("loads and configures the runtime before rendering the app", async () => {
    const runtime = { kind: "mobile" };
    const calls: string[] = [];
    const loadRuntime = vi.fn(async () => {
      calls.push("load");
      return runtime;
    });
    const configureRuntime = vi.fn((selectedRuntime: typeof runtime) => {
      calls.push(`configure:${selectedRuntime.kind}`);
    });
    const renderApp = vi.fn(() => {
      calls.push("render-app");
    });
    const renderError = vi.fn();

    await bootstrapApplication({
      configureRuntime,
      loadRuntime,
      reload: vi.fn(),
      renderApp,
      renderError
    });

    expect(calls).toEqual(["load", "configure:mobile", "render-app"]);
    expect(configureRuntime).toHaveBeenCalledWith(runtime);
    expect(renderError).not.toHaveBeenCalled();
  });

  it("renders the startup error and gives Retry a live reload callback when loading fails", async () => {
    const loadError = new Error("mobile runtime failed");
    const configureRuntime = vi.fn();
    const renderApp = vi.fn();
    const reload = vi.fn();
    let retry: (() => unknown) | undefined;
    const renderError = vi.fn((onRetry: () => unknown) => {
      retry = onRetry;
    });

    await bootstrapApplication({
      configureRuntime,
      loadRuntime: vi.fn().mockRejectedValue(loadError),
      reload,
      renderApp,
      renderError
    });

    expect(configureRuntime).not.toHaveBeenCalled();
    expect(renderApp).not.toHaveBeenCalled();
    expect(renderError).toHaveBeenCalledTimes(1);
    expect(retry).toEqual(expect.any(Function));

    retry?.();
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("starts an injected application mount owner and returns one idempotent stop", async () => {
    const log: string[] = [];
    const stop = await bootstrapApplicationMount({
      mountOwner: {
        start: async () => {
          log.push("start");
          return undefined;
        },
        close: () => {
          log.push("close");
          return undefined;
        }
      },
      reload: vi.fn(),
      renderError: vi.fn()
    });

    expect(log).toEqual(["start"]);
    stop();
    stop();
    expect(log).toEqual(["start", "close"]);
  });

  it("closes a failed mount before rendering the reload error", async () => {
    const log: string[] = [];
    const reload = vi.fn();
    let retry: (() => unknown) | undefined;
    const stop = await bootstrapApplicationMount({
      mountOwner: {
        start: async () => {
          log.push("start");
          throw new Error("authenticated mount failed");
        },
        close: () => {
          log.push("close");
          return undefined;
        }
      },
      reload,
      renderError: (onRetry) => {
        log.push("render-error");
        retry = onRetry;
      }
    });

    expect(log).toEqual(["start", "close", "render-error"]);
    stop();
    retry?.();
    expect(log).toEqual(["start", "close", "render-error"]);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("renders one reload error when a started mount reports a later failure", async () => {
    const log: string[] = [];
    const reload = vi.fn();
    let reportFailure: (() => unknown) | undefined;
    const stop = await bootstrapApplicationMount({
      mountOwner: {
        start: async (onFailure?: () => unknown) => {
          log.push("start");
          reportFailure = onFailure;
          return undefined;
        },
        close: () => {
          log.push("close");
          return undefined;
        }
      },
      reload,
      renderError: () => log.push("render-error")
    });

    reportFailure?.();
    reportFailure?.();
    stop();

    expect(log).toEqual(["start", "close", "render-error"]);
  });
});
