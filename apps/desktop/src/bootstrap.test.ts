import { bootstrapApplication } from "./bootstrap";

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
});
