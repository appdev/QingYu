import { act, renderHook, waitFor } from "@testing-library/react";
import { configureAppRuntime, createDefaultAppRuntime, resetAppRuntimeForTests } from "../runtime";
import { useThemeCatalog } from "./useThemeCatalog";

describe("useThemeCatalog", () => {
  afterEach(() => {
    resetAppRuntimeForTests();
  });

  it("loads on mount and refreshes when another window changes the catalog", async () => {
    const runtime = createDefaultAppRuntime();
    const listeners = new Map<string, () => unknown>();
    runtime.events.isAvailable = () => true;
    runtime.events.listen = vi.fn(async (event, handler) => {
      listeners.set(event, () => handler({ payload: { revision: "2" } }));
      return () => undefined;
    });
    runtime.themes.list = vi.fn(async () => ({ invalidFiles: [], themes: [] }));
    configureAppRuntime(runtime);

    const { result } = renderHook(() => useThemeCatalog());
    await waitFor(() => expect(runtime.themes.list).toHaveBeenCalledTimes(1));

    act(() => {
      listeners.get("markra://theme-catalog-changed")?.();
    });

    await waitFor(() => expect(runtime.themes.list).toHaveBeenCalledTimes(2));
    expect(result.current.lightThemes[0].id).toBe("light");
    expect(result.current.darkThemes[0].id).toBe("dark");
  });

  it("keeps the previous snapshot when a later refresh fails", async () => {
    const runtime = createDefaultAppRuntime();
    runtime.themes.list = vi.fn()
      .mockResolvedValueOnce({
        invalidFiles: [],
        themes: [{
          appearance: "dark",
          fileName: "nord.css",
          fingerprint: "fingerprint",
          id: "nord",
          name: "Nord",
          preview: { accent: "#88c0d0", background: "#2e3440", panel: "#3b4252", text: "#eceff4" },
          source: "third-party",
          storageKind: "inlineCss"
        }]
      })
      .mockRejectedValueOnce(new Error("scan failed"));
    configureAppRuntime(runtime);

    const { result } = renderHook(() => useThemeCatalog());
    await waitFor(() => expect(result.current.darkThemes.some(({ id }) => id === "nord")).toBe(true));

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.darkThemes.some(({ id }) => id === "nord")).toBe(true);
    expect(result.current.error).toBe("scan failed");
  });
});
