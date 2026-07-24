import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import type { CompactNavigation, CompactPage } from "../../hooks/useCompactNavigation";
import { CompactEditorScreen } from "./CompactEditorScreen";
import type { CompactAppController, CompactSaveState } from "./types";

function navigation(page: CompactPage = { kind: "editor" }) {
  return {
    page,
    push: vi.fn(() => true)
  } as unknown as CompactNavigation;
}

function deferred() {
  let resolve!: () => undefined;
  const promise = new Promise<unknown>((resolvePromise) => {
    resolve = () => {
      resolvePromise(undefined);
      return undefined;
    };
  });
  return { promise, resolve };
}

function saveState(
  status: CompactSaveState["status"],
  error: string | null = null
): CompactSaveState {
  return {
    error,
    flush: vi.fn().mockResolvedValue([]),
    retry: vi.fn().mockResolvedValue([]),
    status
  };
}

function controller(overrides: {
  open?: boolean;
  applicationSync?: boolean;
  saveState?: CompactSaveState;
  syncConfigured?: boolean;
  readOnly?: boolean;
} = {}) {
  const actions = {
    openDocumentHistory: vi.fn(),
    openDocumentSearch: vi.fn(),
    runApplicationSyncNow: vi.fn(),
    saveDocument: vi.fn()
  };
  const createBlankDocument = vi.fn().mockResolvedValue(true);
  const open = overrides.open ?? true;

  return {
    actions,
    controller: {
      actions,
      capabilities: {
        applicationSync: overrides.applicationSync ?? true,
        trueMobile: false
      },
      document: {
        createBlankDocument,
        document: {
          content: open ? "# Draft" : "",
          dirty: false,
          name: open ? "Draft.md" : "Untitled.md",
          open,
          path: open ? "/notes/Draft.md" : null,
          revision: 1,
          sizeBytes: null
        },
        saveCurrentDocument: vi.fn()
      },
      editor: {
        getSelectionFormattingState: vi.fn(() => ({ actions: [], headingLevel: null })),
        host: <section aria-label="Visual Milkdown editor">Visual editor</section>,
        insertMarkdownImage: vi.fn(),
        insertMarkdownLink: vi.fn(),
        readOnly: overrides.readOnly ?? false,
        runFormattingAction: vi.fn(),
        runEditorShortcut: vi.fn(),
        setSelectionHeadingLevel: vi.fn(),
        toggleTaskList: vi.fn()
      },
      language: "en",
      files: {},
      preferences: {},
      workspace: {
        primaryRoot: "/notes",
        syncConfigDocument: overrides.syncConfigured ? {
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
            webdav: {
              password: "",
              serverUrl: "https://dav.example.test",
              username: ""
            }
          },
          issues: [],
          revision: "rev-1",
          readiness: "ready"
        } : null
      },
      saveState: overrides.saveState ?? saveState("saved")
    } as unknown as CompactAppController,
    createBlankDocument
  };
}

describe("CompactEditorScreen", () => {
  it.each([
    ["saved", "Saved"],
    ["dirty", "Unsaved"],
    ["saving", "Saving"]
  ] as const)("shows the filename with the persistent %s state", (status, label) => {
    const setup = controller({ saveState: saveState(status) });

    render(<CompactEditorScreen controller={setup.controller} navigation={navigation()} />);

    const editorPage = screen.getByRole("region", { name: "Editor" });
    expect(editorPage.querySelector("header")).toHaveClass("pt-[var(--compact-safe-area-top)]");
    expect(screen.getByRole("heading", { name: "Draft.md" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(label);
    expect(screen.getByLabelText("Visual Milkdown editor")).toBeInTheDocument();
  });

  it("keeps a short top-bar failure state and the complete safe reason below it", () => {
    const setup = controller({
      saveState: saveState("error", "The note could not be written: disk full.")
    });

    render(<CompactEditorScreen controller={setup.controller} navigation={navigation()} />);

    expect(screen.getByRole("status")).toHaveTextContent("Save failed.");
    expect(screen.getByRole("alert")).toHaveTextContent("The note could not be written: disk full.");
  });

  it("keeps one Retry action beside a persistent save failure", () => {
    const retry = vi.fn().mockResolvedValue([]);
    const setup = controller({
      saveState: {
        ...saveState("error", "Not enough storage space to save this note."),
        retry
      } as unknown as CompactSaveState
    });

    render(<CompactEditorScreen controller={setup.controller} navigation={navigation()} />);

    expect(screen.getByRole("alert")).toHaveTextContent("Not enough storage space to save this note.");
    const retryButton = screen.getByRole("button", { name: "Retry Save" });
    expect(screen.getAllByRole("button", { name: "Retry Save" })).toHaveLength(1);
    expect(retryButton).toHaveClass("min-h-11", "min-w-11");
    fireEvent.click(retryButton);
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it("opens Files and delegates every supported More action", async () => {
    const setup = controller({ syncConfigured: true });
    const compactNavigation = navigation();
    render(<CompactEditorScreen controller={setup.controller} navigation={compactNavigation} />);

    fireEvent.click(screen.getByRole("button", { name: "Files" }));
    await waitFor(() => expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "files" }));

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    expect(screen.getByRole("menu", { name: "More" })).toHaveClass("top-full");
    fireEvent.click(screen.getByRole("menuitem", { name: "Save" }));
    expect(setup.actions.saveDocument).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Find" }));
    expect(setup.actions.openDocumentSearch).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "History" }));
    expect(setup.actions.openDocumentHistory).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Sync Now" }));
    expect(setup.actions.runApplicationSyncNow).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    await waitFor(() => expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "settings" }));
  });

  it("awaits an attempted local flush before opening Files", async () => {
    const pendingFlush = deferred();
    const flush = vi.fn(() => pendingFlush.promise);
    const setup = controller({
      saveState: {
        ...saveState("dirty"),
        flush
      }
    });
    const compactNavigation = navigation();
    render(<CompactEditorScreen controller={setup.controller} navigation={compactNavigation} />);

    fireEvent.click(screen.getByRole("button", { name: "Files" }));
    expect(flush).toHaveBeenCalledWith("navigation");
    expect(compactNavigation.push).not.toHaveBeenCalled();

    await act(async () => {
      pendingFlush.resolve();
      await pendingFlush.promise;
    });
    expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "files" });
  });

  it("continues to Files, Settings, and Sync after a rejected flush attempt", async () => {
    const flush = vi.fn().mockRejectedValue(new Error("local write failed"));
    const setup = controller({
      saveState: {
        ...saveState("error", "The note could not be saved."),
        flush
      },
      syncConfigured: false
    });
    const compactNavigation = navigation();
    render(<CompactEditorScreen controller={setup.controller} navigation={compactNavigation} />);

    fireEvent.click(screen.getByRole("button", { name: "Files" }));
    await waitFor(() => expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "files" }));

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    await waitFor(() => expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "settings" }));

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    await waitFor(() => expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "sync-status" }));
    expect(flush).toHaveBeenCalledTimes(3);
  });

  it("routes an unconfigured sync action to the Compact sync status page", async () => {
    const setup = controller({ syncConfigured: false });
    const compactNavigation = navigation();
    render(<CompactEditorScreen controller={setup.controller} navigation={compactNavigation} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));

    await waitFor(() => expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "sync-status" }));
    expect(setup.actions.runApplicationSyncNow).not.toHaveBeenCalled();
  });

  it("shows the welcome state and asks for the new document name in-app", async () => {
    const setup = controller({ applicationSync: true, open: false });
    const compactNavigation = navigation();
    render(<CompactEditorScreen controller={setup.controller} navigation={compactNavigation} />);

    expect(screen.getByRole("heading", { name: "Start writing" })).toBeInTheDocument();
    expect(screen.queryByRole("toolbar", { name: "Formatting" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Visual Milkdown editor")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "New Document" }));
    expect(screen.getByRole("dialog", { name: "New file name" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "New file name" }), {
      target: { value: "  First note  " }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(setup.createBlankDocument).toHaveBeenCalledWith("First note"));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    await waitFor(() => expect(compactNavigation.push).toHaveBeenCalledWith({ kind: "sync-status" }));
    expect(screen.queryByRole("button", { name: /open|choose|recent/i })).not.toBeInTheDocument();
  });

  it("keeps welcome-state creation failures readable and retryable in the dialog", async () => {
    const setup = controller({ open: false });
    setup.createBlankDocument.mockRejectedValueOnce(new Error(
      "token=super-secret failed at /Users/example/private-note.md"
    ));
    render(<CompactEditorScreen controller={setup.controller} navigation={navigation()} />);

    fireEvent.click(screen.getByRole("button", { name: "New Document" }));
    fireEvent.change(screen.getByRole("textbox", { name: "New file name" }), {
      target: { value: "Draft" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("The file operation failed.");
    expect(screen.queryByText(/super-secret|Users|private-note/u)).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "New file name" })).toBeInTheDocument();
  });

  it("mounts the touch formatting toolbar only for an active document", () => {
    const active = controller({ open: true });
    const { rerender } = render(
      <CompactEditorScreen controller={active.controller} navigation={navigation()} />
    );

    expect(screen.getByRole("toolbar", { name: "Formatting" })).toBeInTheDocument();

    const welcome = controller({ open: false });
    rerender(<CompactEditorScreen controller={welcome.controller} navigation={navigation()} />);

    expect(screen.queryByRole("toolbar", { name: "Formatting" })).not.toBeInTheDocument();
  });

  it("disables formatting actions while the shared editor is read-only", () => {
    const setup = controller({ readOnly: true });
    render(<CompactEditorScreen controller={setup.controller} navigation={navigation()} />);

    expect(screen.getByRole("button", { name: "Bold" })).toHaveAttribute("aria-disabled", "true");
  });

  it.each([
    { kind: "files" },
    { kind: "move-target", path: "/notes" },
    { kind: "settings" },
    { kind: "sync-status" }
  ] as const)("does not render the toolbar behind the $kind page", (page) => {
    const setup = controller({ open: true });
    render(
      <CompactEditorScreen
        controller={setup.controller}
        navigation={navigation(page)}
      />
    );

    expect(screen.queryByRole("toolbar", { name: "Formatting" })).not.toBeInTheDocument();
  });

  it("does not expose remote configuration in web narrow mode", () => {
    const setup = controller({ applicationSync: false, open: false });
    render(<CompactEditorScreen controller={setup.controller} navigation={navigation()} />);

    expect(screen.queryByRole("button", { name: "Configure Sync" })).not.toBeInTheDocument();
    expect(screen.getByText("Your notes stay on this device.")).toBeInTheDocument();
  });

  it("does not mount desktop-only editor capabilities or bottom navigation", () => {
    const setup = controller();
    render(<CompactEditorScreen controller={setup.controller} navigation={navigation()} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    for (const forbiddenName of ["AI", "Source", "Split", "Export", "Open Folder"]) {
      expect(screen.queryByRole("button", { name: forbiddenName })).not.toBeInTheDocument();
      expect(screen.queryByRole("menuitem", { name: forbiddenName })).not.toBeInTheDocument();
    }
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  });
});
