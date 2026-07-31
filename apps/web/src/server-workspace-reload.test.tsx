import { StrictMode } from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import App, {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
} from "@markra/app";

import { serverWorkspaceRoot } from "./runtime/server/files";

function configureFixedServerRuntime() {
  const runtime = createDefaultAppRuntime();
  const loadMarkdownFilesForPath = vi.fn(async () => ([{
    name: "note.md",
    path: `${serverWorkspaceRoot}/note.md`,
    relativePath: "note.md",
  }]));
  configureAppRuntime({
    ...runtime,
    features: {
      ...runtime.features,
      nativeWindowChrome: false,
      projectSync: false,
    },
    files: {
      ...runtime.files,
      listMarkdownFilesForPath: loadMarkdownFilesForPath,
      loadMarkdownFilesForPath,
      resolveMarkdownFolder: async () => ({
        name: "primary",
        path: serverWorkspaceRoot,
      }),
    },
    platform: {
      resolveDesktopOsVersion: () => null,
      resolveDesktopPlatform: () => null,
      resolveFormFactor: () => "desktop",
    },
    workspace: {
      ...runtime.workspace,
      rootPolicy: {
        canChooseLocalRoot: false,
        kind: "fixed",
        resolveRoot: async () => serverWorkspaceRoot,
      },
    },
  });

  return loadMarkdownFilesForPath;
}

describe("server Web fixed workspace reload", () => {
  beforeEach(() => {
    const mediaQuery = {
      addEventListener: vi.fn(),
      addListener: vi.fn(),
      dispatchEvent: vi.fn(),
      matches: false,
      media: "",
      onchange: null,
      removeEventListener: vi.fn(),
      removeListener: vi.fn(),
    } as unknown as MediaQueryList;
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn(() => mediaQuery),
    });
  });

  afterEach(() => {
    cleanup();
    resetAppRuntimeForTests();
  });

  it("hydrates the fixed primary tree after an authenticated mount and full page remount", async () => {
    const firstLoad = configureFixedServerRuntime();
    const firstPage = render(<StrictMode><App /></StrictMode>);

    await waitFor(() => expect(firstLoad).toHaveBeenCalledWith(
      serverWorkspaceRoot,
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    ));
    expect(await screen.findByRole("heading", { name: "primary" })).not.toBeNull();
    expect(screen.queryByText("No folder")).toBeNull();

    firstPage.unmount();
    resetAppRuntimeForTests();

    const reloadLoad = configureFixedServerRuntime();
    render(<StrictMode><App /></StrictMode>);

    await waitFor(() => expect(reloadLoad).toHaveBeenCalledWith(
      serverWorkspaceRoot,
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    ));
    expect(await screen.findByRole("heading", { name: "primary" })).not.toBeNull();
    expect(screen.queryByText("No folder")).toBeNull();
  });
});
