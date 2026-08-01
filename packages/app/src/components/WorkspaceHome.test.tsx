import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
  afterEach(() => {
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-theme-appearance");
    document.documentElement.removeAttribute("style");
  });

  it("renders supplied desktop actions in fixed order with supplied shortcuts", () => {
    render(
      <WorkspaceHome
        actions={actions()}
        language="en"
        presentation="desktop"
        shortcuts={{
          createDocument: ["⌘", "N"],
          openDocument: ["⌘", "O"],
          openSettings: ["⌘", ","],
          quickOpen: ["⌘", "P"],
          showFiles: ["⌘", "⇧", "E"]
        }}
      />
    );

    const buttons = screen.getAllByRole("button");
    expect(buttons.map((button) => button.textContent)).toEqual([
      "New Document⌘N",
      "Open Document⌘O",
      "Quick Open⌘P",
      "Show Files⌘⇧E",
      "Open Settings⌘,",
      "Configure Sync",
      "Switch Workspace"
    ]);
    expect([...buttons[0].querySelectorAll("kbd")].map((keycap) => keycap.textContent))
      .toEqual(["⌘", "N"]);
    expect(within(buttons[0]).queryByText("⌘+N")).not.toBeInTheDocument();
    expect([...buttons[4].querySelectorAll("kbd")].map((keycap) => keycap.textContent))
      .toEqual(["⌘", ","]);
    expect(buttons[3].querySelectorAll("kbd")).toHaveLength(3);
  });

  it("renders the approved index-first structure without marketing copy or action icons", () => {
    const { container } = render(
      <WorkspaceHome
        actions={actions()}
        language="en"
        presentation="desktop"
      />
    );

    const home = container.querySelector('[data-workspace-surface="home"]');
    const heading = screen.getByRole("heading", { name: "Welcome to QingYu" });
    const brand = container.querySelector("[data-workspace-home-brand]");

    expect(home).toBeInTheDocument();
    expect(heading).toHaveClass("sr-only");
    expect(screen.queryByText("Create a document or choose another way to continue in this workspace."))
      .not.toBeInTheDocument();
    expect(brand).toHaveAttribute("aria-hidden", "true");
    expect(brand?.children).toHaveLength(4);
    expect(container.querySelectorAll("svg")).toHaveLength(0);
    expect(container.querySelectorAll('[role="separator"]')).toHaveLength(1);
  });

  it("omits every action whose callback is absent", () => {
    const { container } = render(
      <WorkspaceHome
        actions={{ createDocument: vi.fn() }}
        language="en"
        presentation="desktop"
      />
    );

    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual([
      "New Document"
    ]);
    expect(container.querySelector('[role="separator"]')).not.toBeInTheDocument();
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

  it("uses the approved compact desktop composition and action density", () => {
    const { container, rerender } = render(
      <WorkspaceHome
        actions={actions()}
        language="en"
        presentation="desktop"
        shortcuts={{ createDocument: ["⌘", "N"] }}
      />
    );

    const desktopHome = container.querySelector<HTMLElement>('[data-workspace-surface="home"]');
    const desktopComposition = desktopHome?.firstElementChild;
    const desktopBrand = container.querySelector<HTMLElement>("[data-workspace-home-brand]");
    const desktopNewDocument = screen.getByRole("button", { name: "New Document" });

    expect(desktopComposition).toHaveClass("max-w-[16.25rem]", "gap-4", "my-auto");
    expect(desktopComposition).not.toHaveClass("-translate-x-6");
    expect(desktopBrand).toHaveClass("h-[8.25rem]", "w-[4.875rem]");
    expect(desktopNewDocument).toHaveClass("min-h-7", "gap-3", "px-1.5", "font-normal");
    expect(desktopNewDocument.querySelector("kbd")).toHaveClass(
      "min-h-[1.125rem]",
      "min-w-[1.125rem]"
    );

    rerender(
      <WorkspaceHome
        actions={actions()}
        language="en"
        presentation="compact"
        shortcuts={{ createDocument: ["⌘", "N"] }}
      />
    );

    const compactHome = container.querySelector<HTMLElement>('[data-workspace-surface="home"]');
    const compactComposition = compactHome?.firstElementChild;
    const compactBrand = container.querySelector<HTMLElement>("[data-workspace-home-brand]");

    expect(compactHome).toHaveClass("px-6", "py-4");
    expect(compactComposition).toHaveClass("my-auto", "gap-2.5");
    expect(compactBrand).toHaveClass("h-[4.875rem]", "w-[2.875rem]");
    expect(container.querySelector("kbd")).not.toBeInTheDocument();
  });

  it("keeps short desktop surfaces scrollable from the top while centering available space", () => {
    const { container } = render(
      <WorkspaceHome
        actions={actions()}
        language="en"
        presentation="desktop"
      />
    );

    const home = container.querySelector<HTMLElement>('[data-workspace-surface="home"]');
    const composition = home?.firstElementChild;

    expect(home).toHaveClass("items-start");
    expect(home).not.toHaveClass("items-center");
    expect(composition).toHaveClass("my-auto");
  });

  it("does not render shortcut hints in compact presentation", () => {
    const { container } = render(
      <WorkspaceHome
        actions={actions()}
        language="en"
        presentation="compact"
        shortcuts={{ createDocument: ["⌘", "N"], showFiles: ["⌘", "⇧", "M"] }}
      />
    );

    expect(container.querySelector("kbd")).not.toBeInTheDocument();
  });

  it("derives restrained brand and readable functional colors from active theme tokens", async () => {
    document.documentElement.dataset.themeAppearance = "dark";
    document.documentElement.style.setProperty("--bg-primary", "rgb(35 40 45)");
    document.documentElement.style.setProperty("--text-heading", "rgb(231 233 234)");
    document.documentElement.style.setProperty("--text-primary", "rgb(231 233 234)");
    document.documentElement.style.setProperty("--text-secondary", "rgb(171 178 191)");
    document.documentElement.style.setProperty("--accent", "rgb(106 154 132)");

    const { container } = render(
      <WorkspaceHome
        actions={actions()}
        language="en"
        presentation="desktop"
      />
    );
    const home = container.querySelector<HTMLElement>('[data-workspace-surface="home"]');

    await waitFor(() => {
      expect(Number(home?.dataset.brandBaseContrast)).toBeGreaterThanOrEqual(1.35);
      expect(Number(home?.dataset.brandBaseContrast)).toBeLessThan(1.47);
      expect(Number(home?.dataset.brandSliceContrast)).toBeGreaterThanOrEqual(1.6);
      expect(Number(home?.dataset.brandSliceContrast)).toBeLessThan(1.72);
      expect(home?.style.getPropertyValue("--workspace-home-brand-base")).toMatch(/^rgb\(/u);
      expect(home?.style.getPropertyValue("--workspace-home-text")).toMatch(/^rgb\(/u);
    });

    const previousBase = home?.style.getPropertyValue("--workspace-home-brand-base");
    document.documentElement.style.setProperty("--bg-primary", "rgb(255 255 255)");
    document.documentElement.style.setProperty("--text-heading", "rgb(38 38 38)");
    document.documentElement.dataset.theme = "classic-light";

    await waitFor(() => {
      expect(home?.style.getPropertyValue("--workspace-home-brand-base"))
        .not.toBe(previousBase);
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
