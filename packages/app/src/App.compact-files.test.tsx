import { act, waitFor } from "@testing-library/react";
import type { CompactAppController } from "./components/compact/types";
import {
  installAppTestHarness,
  mockedConfirmNativeMarkdownFileDelete,
  mockedDeleteNativeMarkdownTreeFile,
  mockedGetStoredWorkspaceState,
  mockedListNativeMarkdownFilesForPath,
  mockedMoveNativeMarkdownTreeFile,
  mockedNotifyAppEditorPreferencesChanged,
  mockedSaveStoredLanguage,
  mockedSaveStoredEditorPreferences,
  mockedSaveStoredThemePreferences,
  mockedOpenNativeMarkdownAttachment,
  mockedReadNativeMarkdownFile,
  mockedRenameNativeMarkdownTreeFile,
  renderApp
} from "./test/app-harness";
import { configureAppRuntime, createDefaultAppRuntime, resetAppRuntimeForTests } from "./runtime";

const captureCompactController = vi.hoisted(() => vi.fn());

vi.mock("./components/compact/CompactAppShell", async () => {
  const { createElement } = await vi.importActual<typeof import("react")>("react");

  return {
    CompactAppShell: ({ controller }: { controller: CompactAppController }) => {
      captureCompactController(controller);
      return createElement("main", { "data-testid": "compact-app-shell" }, controller.editor.host);
    }
  };
});

installAppTestHarness();

const rootPath = "/mock-files/vault";
const currentFile = {
  name: "current.md",
  path: `${rootPath}/current.md`,
  relativePath: "current.md"
};

function mockCompactViewport() {
  const compactMediaQuery = {
    matches: true,
    media: "(max-width: 720px)",
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn()
  } as unknown as MediaQueryList;
  const defaultMediaQuery = {
    ...compactMediaQuery,
    matches: false,
    media: "(prefers-color-scheme: dark)"
  } as unknown as MediaQueryList;

  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn((query: string) => query === compactMediaQuery.media ? compactMediaQuery : defaultMediaQuery)
  });
}

function latestController() {
  const controller = captureCompactController.mock.calls.at(-1)?.[0] as CompactAppController | undefined;
  if (!controller) throw new Error("Expected App to provide a Compact controller.");
  return controller;
}

async function renderCompactWorkspace(extraFiles: CompactAppController["files"]["files"] = []) {
  const runtime = createDefaultAppRuntime();
  mockCompactViewport();
  configureAppRuntime({
    ...runtime,
    features: {
      ...runtime.features,
      openLocalAttachments: true
    },
    platform: {
      ...runtime.platform,
      resolveFormFactor: () => "desktop"
    }
  });
  mockedGetStoredWorkspaceState.mockResolvedValue({
    filePath: currentFile.path,
    fileTreeOpen: true,
    folderName: "vault",
    folderPath: rootPath,
    openFilePaths: [currentFile.path]
  });
  mockedListNativeMarkdownFilesForPath.mockResolvedValue([currentFile, ...extraFiles]);
  mockedReadNativeMarkdownFile.mockResolvedValue({
    content: "# Current",
    name: currentFile.name,
    path: currentFile.path
  });
  captureCompactController.mockClear();

  renderApp();
  await waitFor(() => expect(captureCompactController).toHaveBeenCalled());
  await waitFor(() => expect(latestController().document.document.path).toBe(currentFile.path));
  return latestController();
}

describe("App Compact file actions", () => {
  afterEach(() => {
    resetAppRuntimeForTests();
  });

  it("routes rename and move through the App handlers that update the open document path", async () => {
    const controller = await renderCompactWorkspace();
    const renamedFile = {
      ...currentFile,
      name: "renamed.md",
      path: `${rootPath}/renamed.md`,
      relativePath: "renamed.md"
    };
    mockedRenameNativeMarkdownTreeFile.mockResolvedValueOnce(renamedFile);

    await act(async () => controller.files.renameFile(currentFile, "renamed.md"));
    await waitFor(() => expect(latestController().document.document.path).toBe(renamedFile.path));

    const movedFile = {
      ...renamedFile,
      path: `${rootPath}/archive/renamed.md`,
      relativePath: "archive/renamed.md"
    };
    mockedMoveNativeMarkdownTreeFile.mockResolvedValueOnce(movedFile);
    await act(async () => latestController().files.moveFile(renamedFile, `${rootPath}/archive`));

    await waitFor(() => expect(latestController().document.document.path).toBe(movedFile.path));
  });

  it("routes delete through confirmation and detaches the active document", async () => {
    const controller = await renderCompactWorkspace();

    await act(async () => controller.files.deleteFile(currentFile));

    expect(mockedConfirmNativeMarkdownFileDelete).toHaveBeenCalledTimes(1);
    expect(mockedDeleteNativeMarkdownTreeFile).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(latestController().document.document.path).toBeNull());
  });

  it("reports a caught native move failure without changing the active document path", async () => {
    const controller = await renderCompactWorkspace();
    mockedMoveNativeMarkdownTreeFile.mockRejectedValueOnce(new Error("native move failed"));

    let moved: unknown;
    await act(async () => {
      moved = await controller.files.moveFile(currentFile, `${rootPath}/archive`);
    });

    expect(moved).toBe(false);
    expect(mockedMoveNativeMarkdownTreeFile).toHaveBeenCalledTimes(1);
    expect(latestController().document.document.path).toBe(currentFile.path);
  });

  it("routes asset and attachment entries through the existing App open handler", async () => {
    const asset = {
      kind: "asset" as const,
      name: "image.png",
      path: `${rootPath}/assets/image.png`,
      relativePath: "assets/image.png"
    };
    const attachment = {
      kind: "attachment" as const,
      name: "reference.pdf",
      path: `${rootPath}/assets/reference.pdf`,
      relativePath: "assets/reference.pdf"
    };
    const controller = await renderCompactWorkspace([asset, attachment]);
    const markdownReadCount = mockedReadNativeMarkdownFile.mock.calls.length;

    await act(async () => controller.files.openFile(asset));
    await act(async () => latestController().files.openFile(attachment));

    expect(mockedReadNativeMarkdownFile).toHaveBeenCalledTimes(markdownReadCount);
    expect(mockedOpenNativeMarkdownAttachment).toHaveBeenCalledWith({
      documentPath: null,
      rootPath,
      src: attachment.relativePath
    });
  });

  it("wires Compact language changes through the existing app-language hook", async () => {
    const controller = await renderCompactWorkspace();

    act(() => controller.selectLanguage("zh-CN"));

    await waitFor(() => expect(mockedSaveStoredLanguage).toHaveBeenCalledWith("zh-CN"));
    await waitFor(() => expect(latestController().language).toBe("zh-CN"));
  });

  it("wires Compact appearance changes through the existing app-theme hook", async () => {
    const controller = await renderCompactWorkspace();

    act(() => controller.appearance.selectAppearanceMode("dark"));

    await waitFor(() => expect(mockedSaveStoredThemePreferences).toHaveBeenCalledWith({
      appearanceMode: "dark",
      darkTheme: "dark",
      lightTheme: "light"
    }));
    await waitFor(() => expect(latestController().appearance.appearanceMode).toBe("dark"));
  });

  it("persists Compact editor preference changes before notifying the app", async () => {
    const controller = await renderCompactWorkspace();
    await waitFor(() => expect(latestController().preferences.loading).toBe(false));
    let finishSave: (() => unknown) | null = null;
    mockedSaveStoredEditorPreferences.mockImplementationOnce(() => new Promise((resolve) => {
      finishSave = resolve;
    }));
    const nextPreferences = {
      ...controller.preferences.preferences,
      bodyFontSize: 18
    };

    act(() => controller.preferences.updatePreferences(nextPreferences));

    await waitFor(() => expect(mockedSaveStoredEditorPreferences).toHaveBeenCalledWith(nextPreferences));
    expect(mockedNotifyAppEditorPreferencesChanged).not.toHaveBeenCalled();
    expect(latestController().preferences.preferences).toEqual(nextPreferences);

    act(() => finishSave?.());

    await waitFor(() => expect(mockedNotifyAppEditorPreferencesChanged).toHaveBeenCalledWith(nextPreferences));
  });
});
