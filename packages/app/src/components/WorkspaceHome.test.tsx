import { fireEvent, render, screen, within } from "@testing-library/react";
import { WorkspaceHome, type WorkspaceHomeProps } from "./WorkspaceHome";

function actions(): WorkspaceHomeProps["actions"] {
  return {
    createDocument: vi.fn(),
    openDocument: vi.fn(),
    quickOpen: vi.fn(),
    showFiles: vi.fn(),
    openSettings: vi.fn(),
    configureSync: vi.fn(),
    switchWorkspace: vi.fn()
  };
}

describe("WorkspaceHome", () => {
  it("renders supplied desktop actions in fixed order with supplied shortcuts", () => {
    render(
      <WorkspaceHome
        actions={actions()}
        language="en"
        presentation="desktop"
        shortcuts={{
          createDocument: "⌘N",
          openSettings: "⌘,",
          quickOpen: "⌘P",
          showFiles: "⌘⇧E"
        }}
      />
    );

    const buttons = screen.getAllByRole("button");
    expect(buttons.map((button) => button.textContent)).toEqual([
      "New Document⌘N",
      "Open Document",
      "Quick Open⌘P",
      "Show Files⌘⇧E",
      "Open Settings⌘,",
      "Configure Sync",
      "Switch Workspace"
    ]);
    expect(within(buttons[0]).getByText("⌘N")).toBeVisible();
    expect(within(buttons[1]).queryByText("⌘N")).not.toBeInTheDocument();
  });

  it("omits every action whose callback is absent", () => {
    render(
      <WorkspaceHome
        actions={{ createDocument: vi.fn() }}
        language="en"
        presentation="desktop"
      />
    );

    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual([
      "New Document"
    ]);
  });

  it("invokes only the selected supplied callback", () => {
    const suppliedActions = actions();
    render(
      <WorkspaceHome
        actions={suppliedActions}
        language="en"
        presentation="desktop"
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Configure Sync" }));

    expect(suppliedActions.configureSync).toHaveBeenCalledOnce();
    expect(suppliedActions.createDocument).not.toHaveBeenCalled();
    expect(suppliedActions.openDocument).not.toHaveBeenCalled();
    expect(suppliedActions.quickOpen).not.toHaveBeenCalled();
    expect(suppliedActions.showFiles).not.toHaveBeenCalled();
    expect(suppliedActions.openSettings).not.toHaveBeenCalled();
    expect(suppliedActions.switchWorkspace).not.toHaveBeenCalled();
  });

  it("keeps compact actions at accessible 44px targets", () => {
    render(
      <WorkspaceHome
        actions={actions()}
        language="en"
        presentation="compact"
      />
    );

    screen.getAllByRole("button").forEach((button) => {
      expect(button).toHaveClass("min-h-11", "min-w-11");
    });
  });

  it("renders the same action set regardless of ambient platform globals", () => {
    const suppliedActions = actions();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: { platform: "android" }
    });
    const { rerender } = render(
      <WorkspaceHome
        actions={suppliedActions}
        language="en"
        presentation="desktop"
      />
    );
    const withPlatformGlobal = screen.getAllByRole("button").map((button) => button.textContent);

    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    rerender(
      <WorkspaceHome
        actions={suppliedActions}
        language="en"
        presentation="desktop"
      />
    );

    expect(screen.getAllByRole("button").map((button) => button.textContent))
      .toEqual(withPlatformGlobal);
  });
});
