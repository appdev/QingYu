import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import { CompactAppShell } from "./CompactAppShell";
import type { CompactAppController } from "./types";
import type { CompactPage, CompactNavigation } from "../../hooks/useCompactNavigation";

function controllerWithEditorHost() {
  return {
    actions: {
      openDocumentHistory: vi.fn(),
      openDocumentSearch: vi.fn(),
      runApplicationSyncNow: vi.fn(),
      saveDocument: vi.fn()
    },
    capabilities: {
      applicationSync: true,
      trueMobile: false
    },
    document: {
      document: {
        name: "Draft.md",
        open: true
      }
    },
    editor: {
      getSelectionFormattingState: vi.fn(() => ({ actions: [], headingLevel: null })),
      host: <section aria-label="Compact editor host">Editor</section>,
      readOnly: false
    },
    files: {},
    language: "en",
    preferences: {},
    workspace: {
      primaryRoot: "/notes",
      syncConfigDocument: null
    },
    saveState: {
      error: null,
      status: "saved"
    },
    selectLanguage: vi.fn(),
    sync: {
      available: true,
      begin: vi.fn(async () => undefined),
      configDocument: null,
      dirty: false,
      enable: vi.fn(async () => undefined),
      end: vi.fn(async () => undefined),
      loadResult: {
        revision: null,
        status: "absent"
      },
      patch: vi.fn(async () => undefined),
      primaryRoot: "/notes",
      recover: vi.fn(async () => undefined),
      reset: vi.fn(async () => undefined),
      runImmediate: vi.fn(async () => undefined),
      saving: false,
      sessionId: "session-1",
      status: null,
      syncRunning: false,
      testConnection: vi.fn(async () => undefined),
      testing: false
    }
  } as unknown as CompactAppController;
}

describe("CompactAppShell", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders only the minimal editor host", () => {
    const { container } = render(<CompactAppShell controller={controllerWithEditorHost()} />);

    expect(screen.getByTestId("compact-app-shell")).toBeInTheDocument();
    expect(screen.getByLabelText("Compact editor host")).toBeInTheDocument();
    expect(container.querySelector(".native-title")).not.toBeInTheDocument();
    expect(container.querySelector(".markdown-file-tree-drawer")).not.toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "QingYu AI" })).not.toBeInTheDocument();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  });

  it("consumes an explicit controller request by opening the real Compact sync stack", async () => {
    const setup = controllerWithEditorHost();
    setup.navigationRequest = {
      id: 1,
      page: { kind: "sync-status" },
      retainUntilEditor: true
    };

    render(<CompactAppShell controller={setup} />);

    expect(await screen.findByRole("heading", { name: "Local mode" })).toBeInTheDocument();
    expect(screen.getByLabelText("Compact editor host").closest("[data-compact-editor-layer]"))
      .toHaveAttribute("aria-hidden", "true");
  });

  it("completes a non-onboarding navigation request once its exact target opens", async () => {
    const setup = controllerWithEditorHost();
    const onNavigationRequestComplete = vi.fn();
    setup.navigationRequest = {
      id: 2,
      page: { kind: "sync-status" },
      retainUntilEditor: false
    } as NonNullable<CompactAppController["navigationRequest"]> & {
      retainUntilEditor: boolean;
    };

    render(
      <CompactAppShell
        controller={setup}
        onNavigationRequestComplete={onNavigationRequestComplete}
      />
    );

    expect(await screen.findByRole("heading", { name: "Local mode" })).toBeInTheDocument();
    await waitFor(() => expect(onNavigationRequestComplete).toHaveBeenCalledWith(2));
  });

  it("clears a request safely when a pending transition rejects its push from an unrelated page", async () => {
    let finishExit: (() => unknown) | undefined;
    const setup = controllerWithEditorHost();
    const onNavigationRequestComplete = vi.fn();
    const onExitSyncForm = vi.fn(() => new Promise<unknown>((resolve) => {
      finishExit = () => resolve(undefined);
    }));
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);

    function renderPage(page: CompactPage, navigation: CompactNavigation) {
      if (page.kind === "editor") {
        return <button onClick={() => navigation.push({ kind: "sync-form", mode: "edit" })}>Open form</button>;
      }
      if (page.kind === "sync-form") {
        return <button onClick={() => navigation.pop()}>Leave form</button>;
      }
      return <p>{page.kind}</p>;
    }

    const rendered = render(
      <CompactAppShell
        controller={setup}
        onExitSyncForm={onExitSyncForm}
        onNavigationRequestComplete={onNavigationRequestComplete}
        renderPage={renderPage}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Open form" }));
    fireEvent.click(screen.getByRole("button", { name: "Leave form" }));
    await waitFor(() => expect(onExitSyncForm).toHaveBeenCalledOnce());

    setup.navigationRequest = {
      id: 3,
      page: { kind: "sync-status" },
      retainUntilEditor: true
    } as NonNullable<CompactAppController["navigationRequest"]> & {
      retainUntilEditor: boolean;
    };
    rendered.rerender(
      <CompactAppShell
        controller={setup}
        onExitSyncForm={onExitSyncForm}
        onNavigationRequestComplete={onNavigationRequestComplete}
        renderPage={renderPage}
      />
    );

    await waitFor(() => expect(onNavigationRequestComplete).toHaveBeenCalledWith(3));
    expect(screen.getByRole("button", { name: "Leave form" })).toBeInTheDocument();
    await act(async () => finishExit?.());
  });

  it("retains an onboarding request on the exact target until navigation returns to editor", async () => {
    const setup = controllerWithEditorHost();
    const onNavigationRequestComplete = vi.fn();
    setup.navigationRequest = {
      id: 4,
      page: { kind: "sync-status" },
      retainUntilEditor: true
    } as NonNullable<CompactAppController["navigationRequest"]> & {
      retainUntilEditor: boolean;
    };
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);

    function renderPage(page: CompactPage, navigation: CompactNavigation) {
      if (page.kind === "editor") return <p>Editor page</p>;
      return <button onClick={() => navigation.pop()}>Return to editor</button>;
    }

    render(
      <CompactAppShell
        controller={setup}
        onNavigationRequestComplete={onNavigationRequestComplete}
        renderPage={renderPage}
      />
    );
    expect(await screen.findByRole("button", { name: "Return to editor" })).toBeInTheDocument();
    expect(onNavigationRequestComplete).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Return to editor" }));

    await waitFor(() => expect(screen.getByText("Editor page")).toBeInTheDocument());
    expect(onNavigationRequestComplete).toHaveBeenCalledWith(4);
  });

  it("exposes Compact viewport variables on the semantic app root", () => {
    render(<CompactAppShell controller={controllerWithEditorHost()} />);

    const shell = screen.getByRole("main");
    expect(shell).toHaveAttribute("data-compact", "true");
    expect(shell.getAttribute("style")).toContain(
      "--compact-safe-area-top: env(safe-area-inset-top, 0px)"
    );
    expect(shell.getAttribute("style")).toContain(
      "--compact-safe-area-bottom: env(safe-area-inset-bottom, 0px)"
    );
    expect(shell.getAttribute("style")).toContain("--compact-keyboard-inset: 0px");
    expect(shell.getAttribute("style")).toContain(`--compact-visual-viewport-height: ${window.innerHeight}px`);
  });

  it("routes the default Settings home into curated details and back", async () => {
    const setup = controllerWithEditorHost();
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "General" }));
    expect(screen.getByRole("heading", { name: "General" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument());
  });

  it("routes the Settings Sync row into the existing sync status", () => {
    const setup = controllerWithEditorHost();
    render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Sync" }));
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();
  });

  it("layers non-editor pages full screen while isolating the mounted editor", async () => {
    function renderPage(page: CompactPage, navigation: CompactNavigation) {
      if (page.kind === "editor") {
        return <button onClick={() => navigation.push({ kind: "files" })}>Open files</button>;
      }

      return (
        <section aria-label={`${page.kind} page`}>
          Full-screen page
          <button onClick={() => navigation.pop()}>Back to editor</button>
        </section>
      );
    }

    const { container } = render(
      <CompactAppShell controller={controllerWithEditorHost()} renderPage={renderPage} />
    );
    fireEvent.click(screen.getByRole("button", { name: "Open files" }));

    const page = screen.getByLabelText("files page");
    const editorLayer = container.querySelector("[data-compact-editor-layer]");
    expect(editorLayer).toHaveClass("z-0");
    expect(editorLayer).toHaveAttribute("inert");
    expect(editorLayer).toHaveAttribute("aria-hidden", "true");
    expect(page.parentElement).toHaveClass("absolute", "inset-0");
    expect(page.parentElement).toHaveClass("z-10");
    expect(page.parentElement).toHaveAttribute("data-compact-page", "files");
    expect(editorLayer?.querySelector('[aria-label="Compact editor host"]')).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Compact editor host" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Back to editor" }));

    await waitFor(() => {
      expect(screen.getByRole("region", { name: "Compact editor host" })).toBeInTheDocument();
    });
    expect(editorLayer).not.toHaveAttribute("inert");
    expect(editorLayer).not.toHaveAttribute("aria-hidden");
  });

  it("awaits sync settings exit before revealing the previous page", async () => {
    let finishExit: (() => unknown) | undefined;
    const onExitSyncForm = vi.fn(() => new Promise<unknown>((resolve) => {
      finishExit = () => resolve(undefined);
    }));
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);

    function renderPage(page: CompactPage, navigation: CompactNavigation) {
      if (page.kind === "editor") {
        return <button onClick={() => navigation.push({ kind: "sync-status" })}>Open sync</button>;
      }
      if (page.kind === "sync-status") {
        return <button onClick={() => navigation.push({ kind: "sync-form", mode: "edit" })}>Edit sync</button>;
      }
      return <button onClick={() => navigation.pop()}>Back from sync form</button>;
    }

    render(
      <CompactAppShell
        controller={controllerWithEditorHost()}
        onExitSyncForm={onExitSyncForm}
        renderPage={renderPage}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Open sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Edit sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Back from sync form" }));

    await waitFor(() => expect(onExitSyncForm).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("button", { name: "Back from sync form" })).toBeInTheDocument();

    await act(async () => finishExit?.());

    expect(screen.getByRole("button", { name: "Edit sync" })).toBeInTheDocument();
  });

  it("surfaces sync exit failures without removing the form page", async () => {
    const exitError = new Error("sync exit failed");
    const onExitSyncForm = vi.fn().mockRejectedValue(exitError);
    const onNavigationError = vi.fn();

    function renderPage(page: CompactPage, navigation: CompactNavigation) {
      if (page.kind === "editor") {
        return <button onClick={() => navigation.push({ kind: "sync-status" })}>Open sync</button>;
      }
      if (page.kind === "sync-status") {
        return <button onClick={() => navigation.push({ kind: "sync-form", mode: "edit" })}>Edit sync</button>;
      }
      return <button onClick={() => navigation.pop().catch(() => {})}>Back from failing sync form</button>;
    }

    render(
      <CompactAppShell
        controller={controllerWithEditorHost()}
        onExitSyncForm={onExitSyncForm}
        onNavigationError={onNavigationError}
        renderPage={renderPage}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Open sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Edit sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Back from failing sync form" }));

    await waitFor(() => expect(onNavigationError).toHaveBeenCalledWith(exitError));
    expect(onExitSyncForm).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Back from failing sync form" })).toBeInTheDocument();
  });

  it("keeps the editor document mounted when back is requested at root", async () => {
    const historyBack = vi.spyOn(window.history, "back").mockImplementation(() => undefined);

    function renderPage(page: CompactPage, navigation: CompactNavigation) {
      if (page.kind !== "editor") return null;
      return <button onClick={() => navigation.pop()}>Back at root</button>;
    }

    render(<CompactAppShell controller={controllerWithEditorHost()} renderPage={renderPage} />);
    fireEvent.click(screen.getByRole("button", { name: "Back at root" }));

    expect(screen.getByLabelText("Compact editor host")).toBeInTheDocument();
    expect(historyBack).not.toHaveBeenCalled();
  });

  it("renders the default full-screen sync status and form and ends the form session once", async () => {
    const setup = controllerWithEditorHost();
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    expect(screen.getByRole("heading", { name: "Sync Configuration" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();
  });

  it("routes default form Done through the same awaited session-end guard exactly once", async () => {
    const setup = controllerWithEditorHost();
    let finishEnd: (() => unknown) | undefined;
    setup.sync.end = vi.fn(() => new Promise<unknown>((resolve) => {
      finishEnd = () => resolve(undefined);
    }));
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Done" }));

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
    expect(screen.getByRole("heading", { name: "Sync Configuration" })).toBeInTheDocument();

    await act(async () => finishEnd?.());

    expect(setup.sync.end).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();
  });

  it("coalesces system Back while default form exit is pending and ends the session once", async () => {
    const setup = controllerWithEditorHost();
    let finishEnd: (() => unknown) | undefined;
    setup.sync.end = vi.fn(() => new Promise<unknown>((resolve) => {
      finishEnd = () => resolve(undefined);
    }));
    let systemBack: (() => Promise<boolean>) | undefined;
    const subscribeToSystemBack = vi.fn(async (handler: () => Promise<boolean>) => {
      systemBack = handler;
      return vi.fn();
    });
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(
      <CompactAppShell
        controller={setup}
        subscribeToSystemBack={subscribeToSystemBack}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));

    let firstBack: Promise<boolean> | undefined;
    let secondBack: Promise<boolean> | undefined;
    act(() => {
      firstBack = systemBack?.();
      secondBack = systemBack?.();
    });

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
    expect(screen.getByRole("heading", { name: "Sync Configuration" })).toBeInTheDocument();

    await act(async () => {
      finishEnd?.();
      await Promise.all([firstBack, secondBack]);
    });

    expect(setup.sync.end).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();
  });

  it("waits for a pending form begin before toolbar Back ends the session", async () => {
    const setup = controllerWithEditorHost();
    let finishBegin: (() => unknown) | undefined;
    setup.sync.sessionId = null;
    setup.sync.begin = vi.fn(() => new Promise<unknown>((resolve) => {
      finishBegin = () => resolve(undefined);
    }));
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Back" }));

    expect(setup.sync.begin).toHaveBeenCalledTimes(1);
    expect(setup.sync.end).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Sync Configuration" })).toBeInTheDocument();

    await act(async () => finishBegin?.());

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
    expect(setup.sync.enable).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();
  });

  it("waits for a pending form begin before system Back ends the session", async () => {
    const setup = controllerWithEditorHost();
    let finishBegin: (() => unknown) | undefined;
    setup.sync.sessionId = null;
    setup.sync.begin = vi.fn(() => new Promise<unknown>((resolve) => {
      finishBegin = () => resolve(undefined);
    }));
    let systemBack: (() => Promise<boolean>) | undefined;
    const subscribeToSystemBack = async (handler: () => Promise<boolean>) => {
      systemBack = handler;
      return vi.fn();
    };
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(
      <CompactAppShell
        controller={setup}
        subscribeToSystemBack={subscribeToSystemBack}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    let backAttempt: Promise<boolean> | undefined;
    act(() => {
      backAttempt = systemBack?.();
    });

    expect(setup.sync.end).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Sync Configuration" })).toBeInTheDocument();

    await act(async () => {
      finishBegin?.();
      await backAttempt;
    });

    expect(setup.sync.end).toHaveBeenCalledTimes(1);
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
    expect(setup.sync.enable).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();
  });

  it("ends an active form session once when the Compact shell unmounts", async () => {
    const setup = controllerWithEditorHost();
    const rendered = render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    await waitFor(() => expect(setup.sync.begin).toHaveBeenCalledTimes(1));

    rendered.unmount();

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
  });

  it("retries a direct teardown end failure once without a user navigation attempt", async () => {
    const setup = controllerWithEditorHost();
    setup.sync.end = vi.fn()
      .mockRejectedValueOnce(new Error("session end failed"))
      .mockResolvedValueOnce(undefined);
    const rendered = render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    await waitFor(() => expect(setup.sync.begin).toHaveBeenCalledTimes(1));

    rendered.unmount();

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(2));
    expect(setup.sync.end).toHaveBeenNthCalledWith(1, "category-leave");
    expect(setup.sync.end).toHaveBeenNthCalledWith(2, "category-leave");
  });

  it("coalesces teardown end while a pending recovery settles after unmount", async () => {
    const setup = controllerWithEditorHost();
    let finishRecover: (() => unknown) | undefined;
    setup.sync.loadResult = {
      issue: { code: "invalid-json", message: "Malformed configuration." },
      revision: "bad-rev",
      status: "malformed"
    };
    setup.sync.recover = vi.fn(() => new Promise<unknown>((resolve) => {
      finishRecover = () => resolve(undefined);
    }));
    const rendered = render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() => expect(setup.sync.recover).toHaveBeenCalledTimes(1));

    rendered.unmount();
    expect(setup.sync.end).not.toHaveBeenCalled();
    await act(async () => finishRecover?.());

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(setup.sync.recover).toHaveBeenCalledTimes(1);
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
  });

  it("waits for pending begin before ending once when the Compact shell unmounts", async () => {
    const setup = controllerWithEditorHost();
    let finishBegin: (() => unknown) | undefined;
    setup.sync.sessionId = null;
    setup.sync.begin = vi.fn(() => new Promise<unknown>((resolve) => {
      finishBegin = () => resolve(undefined);
    }));
    const rendered = render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    rendered.unmount();

    expect(setup.sync.end).not.toHaveBeenCalled();
    await act(async () => finishBegin?.());
    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
    expect(setup.sync.enable).not.toHaveBeenCalled();
  });

  it("forces a failed patch session to end once when the Compact shell unmounts", async () => {
    const setup = controllerWithEditorHost();
    setup.sync.patch = vi.fn().mockRejectedValue(
      new Error("failed patch with https://private.example.test password=never-render")
    );
    const rendered = render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    await waitFor(() => expect(setup.sync.enable).toHaveBeenCalledTimes(1));
    fireEvent.change(screen.getByLabelText("Remote root"), { target: { value: "failed" } });
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(
      "Sync configuration could not be saved. Try again."
    ));

    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    rendered.unmount();

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
  });

  it("forces a session with failed enable to end after pending begin settles and the shell unmounts", async () => {
    const setup = controllerWithEditorHost();
    let finishBegin: (() => unknown) | undefined;
    setup.sync.sessionId = null;
    setup.sync.begin = vi.fn(() => new Promise<unknown>((resolve) => {
      finishBegin = () => resolve(undefined);
    }));
    setup.sync.enable = vi.fn().mockRejectedValue(
      new Error("failed enable with accessKeyId=never-render")
    );
    const rendered = render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    expect(setup.sync.end).not.toHaveBeenCalled();

    await act(async () => finishBegin?.());
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(
      "Sync configuration could not be saved. Try again."
    ));
    rendered.unmount();

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
  });

  it("does not double-end when a pending normal exit succeeds during shell teardown", async () => {
    const setup = controllerWithEditorHost();
    let finishEnd: (() => unknown) | undefined;
    setup.sync.end = vi.fn(() => new Promise<unknown>((resolve) => {
      finishEnd = () => resolve(undefined);
    }));
    const rendered = render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));

    rendered.unmount();
    await act(async () => finishEnd?.());

    expect(setup.sync.end).toHaveBeenCalledTimes(1);
    expect(setup.sync.end).toHaveBeenCalledWith("category-leave");
  });

  it("retries a failed pending normal exit once during shell teardown", async () => {
    const setup = controllerWithEditorHost();
    let failEnd: (() => unknown) | undefined;
    setup.sync.end = vi.fn()
      .mockImplementationOnce(() => new Promise<unknown>((_resolve, reject) => {
        failEnd = () => reject(new Error("session end failed"));
      }))
      .mockResolvedValueOnce(undefined);
    const rendered = render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));

    rendered.unmount();
    await act(async () => failEnd?.());

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(2));
    expect(setup.sync.end).toHaveBeenNthCalledWith(1, "category-leave");
    expect(setup.sync.end).toHaveBeenNthCalledWith(2, "category-leave");
  });

  it("blocks toolbar Back after a failed patch until a later write succeeds", async () => {
    const setup = controllerWithEditorHost();
    setup.sync.patch = vi.fn().mockRejectedValueOnce(
      new Error("failed patch with https://private.example.test password=never-render")
    ).mockResolvedValueOnce(undefined);
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    await waitFor(() => expect(setup.sync.enable).toHaveBeenCalledTimes(1));
    fireEvent.change(screen.getByLabelText("Remote root"), { target: { value: "failed" } });
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(
      "Sync configuration could not be saved. Try again."
    ));

    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await act(async () => Promise.resolve());
    expect(setup.sync.end).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Sync Configuration" })).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("private.example.test");
    expect(document.body.textContent).not.toContain("never-render");

    fireEvent.change(screen.getByLabelText("Remote root"), { target: { value: "saved" } });
    await waitFor(() => expect(setup.sync.patch).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole("button", { name: "Back" }));

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();
  });

  it("keeps a failed field blocking Done after an unrelated field saves", async () => {
    const setup = controllerWithEditorHost();
    setup.sync.patch = vi.fn()
      .mockRejectedValueOnce(new Error("remotePath failed with password=never-render"))
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(undefined);
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    await waitFor(() => expect(setup.sync.enable).toHaveBeenCalledTimes(1));
    fireEvent.change(screen.getByLabelText("Remote root"), { target: { value: "failed-a" } });
    await waitFor(() => expect(setup.sync.patch).toHaveBeenCalledTimes(1));
    fireEvent.change(screen.getByLabelText("WebDAV server URL"), {
      target: { value: "https://saved-b.example.test" }
    });
    await waitFor(() => expect(setup.sync.patch).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole("button", { name: "Done" }));

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(
      "Sync configuration could not be saved. Try again."
    ));
    expect(setup.sync.end).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Sync Configuration" })).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("never-render");

    fireEvent.change(screen.getByLabelText("Remote root"), { target: { value: "saved-a" } });
    await waitFor(() => expect(setup.sync.patch).toHaveBeenCalledTimes(3));
    fireEvent.click(screen.getByRole("button", { name: "Done" }));

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();
  });

  it("leaves without ending when form begin rejects before a session becomes active", async () => {
    const setup = controllerWithEditorHost();
    setup.sync.sessionId = null;
    setup.sync.begin = vi.fn().mockRejectedValue(
      new Error("begin failed with https://private.example.test password=never-render")
    );
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(
      "Sync configuration could not be saved. Try again."
    ));
    fireEvent.click(screen.getByRole("button", { name: "Back" }));

    await waitFor(() => expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument());
    expect(setup.sync.end).not.toHaveBeenCalled();
    expect(document.body.textContent).not.toContain("private.example.test");
    expect(document.body.textContent).not.toContain("never-render");
  });

  it("blocks system Back after automatic create enable fails until a retry succeeds", async () => {
    const setup = controllerWithEditorHost();
    setup.sync.enable = vi.fn().mockRejectedValueOnce(
      new Error("failed enable with accessKeyId=never-render")
    ).mockResolvedValueOnce(undefined);
    let systemBack: (() => Promise<boolean>) | undefined;
    const subscribeToSystemBack = async (handler: () => Promise<boolean>) => {
      systemBack = handler;
      return vi.fn();
    };
    vi.spyOn(window.history, "back").mockImplementation(() => undefined);
    render(
      <CompactAppShell
        controller={setup}
        subscribeToSystemBack={subscribeToSystemBack}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(
      "Sync configuration could not be saved. Try again."
    ));

    await act(async () => systemBack?.());
    expect(setup.sync.end).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Sync Configuration" })).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("never-render");

    fireEvent.change(screen.getByLabelText("Remote root"), { target: { value: "retry" } });
    await waitFor(() => expect(setup.sync.enable).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(setup.sync.patch).toHaveBeenCalledTimes(1));
    await act(async () => systemBack?.());

    expect(setup.sync.end).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("heading", { name: "Local mode" })).toBeInTheDocument();
  });

  it("keeps the default form visible with safe retry copy when session end fails", async () => {
    const setup = controllerWithEditorHost();
    setup.sync.end = vi.fn(async () => {
      throw new Error("https://private.example.test password=never-render");
    });
    render(<CompactAppShell controller={setup} />);

    fireEvent.click(screen.getByRole("button", { name: "More" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));
    fireEvent.click(screen.getByRole("button", { name: "Back" }));

    await waitFor(() => expect(setup.sync.end).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("heading", { name: "Sync Configuration" })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Sync configuration could not be saved. Try again.");
    expect(document.body.textContent).not.toContain("private.example.test");
    expect(document.body.textContent).not.toContain("never-render");
  });
});
