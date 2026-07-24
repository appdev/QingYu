import { fireEvent, render, screen } from "@testing-library/react";
import { t } from "@markra/shared";
import { SettingsContent, SettingsSidebar } from "./SettingsShell";

function translate(key: Parameters<typeof t>[1]) {
  return t("en", key);
}

function renderSettingsSidebar(onCategoryChange = vi.fn()) {
  render(
    <SettingsSidebar
      activeCategory="general"
      appVersion="9.9.9"
      platform="macos"
      translate={translate}
      onCategoryChange={onCategoryChange}
    />
  );

  return onCategoryChange;
}

function renderSettingsContent(platform?: "linux" | "macos" | "windows") {
  const { container } = render(
    <SettingsContent activeCategory="general" platform={platform} translate={translate}>
      <div />
    </SettingsContent>
  );

  return container.querySelector(".settings-content");
}

describe("SettingsShell", () => {
  it("shows keyboard shortcuts as its own settings category", () => {
    const onCategoryChange = renderSettingsSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Keyboard shortcuts" }));

    expect(onCategoryChange).toHaveBeenCalledWith("keyboardShortcuts");
  });

  it("shows the configured app version in the sidebar footer", () => {
    renderSettingsSidebar();

    expect(screen.getByText("QingYu v9.9.9")).toBeInTheDocument();
  });

  it("styles the active category through its page-specific current state", () => {
    renderSettingsSidebar();

    const activeCategory = screen.getByRole("button", { name: "General" });
    const inactiveCategory = screen.getByRole("button", { name: "Sync" });

    expect(activeCategory).toHaveAttribute("aria-current", "page");
    expect(activeCategory).toHaveClass(
      "aria-[current=page]:bg-(--bg-active)",
      "aria-[current=page]:text-(--accent)"
    );
    expect(inactiveCategory).not.toHaveAttribute("aria-current");
  });

  it("keeps settings shell chrome treatment scoped to Windows", () => {
    const defaultContent = renderSettingsContent();

    expect(defaultContent).not.toHaveClass("rounded-tl-md");
    expect(defaultContent).not.toHaveClass("border-l");

    const { container } = render(
      <SettingsSidebar
        activeCategory="general"
        appVersion="9.9.9"
        platform="windows"
        translate={translate}
        onCategoryChange={() => {}}
      />
    );

    expect(container.querySelector(".settings-sidebar")).toHaveClass("border-r-0", "bg-(--bg-chrome)");
  });

  it("rounds the Windows settings content corner", () => {
    const content = renderSettingsContent("windows");

    expect(content).toHaveClass("border-t", "border-l", "rounded-tl-md");
  });

  it("does not mark the Linux settings header as a native drag region", () => {
    const content = renderSettingsContent("linux");

    expect(content?.querySelector(".settings-content-header")).not.toHaveAttribute("data-tauri-drag-region");
  });

  it("does not expose a standalone storage category", () => {
    renderSettingsSidebar();

    expect(screen.queryByRole("button", { name: "Storage" })).not.toBeInTheDocument();
  });

  it("shows view as its own settings category", () => {
    const onCategoryChange = renderSettingsSidebar();

    fireEvent.click(screen.getByRole("button", { name: "View" }));

    expect(onCategoryChange).toHaveBeenCalledWith("view");
  });

  it("does not expose a network settings category", () => {
    renderSettingsSidebar();

    expect(screen.queryByRole("button", { name: "Network" })).not.toBeInTheDocument();
  });

  it("shows sync as its own settings category", () => {
    const onCategoryChange = renderSettingsSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Sync" }));

    expect(onCategoryChange).toHaveBeenCalledWith("sync");
  });

  it("can hide the desktop-only MCP category", () => {
    render(
      <SettingsSidebar
        activeCategory="general"
        appVersion="9.9.9"
        hiddenCategories={["mcp"]}
        platform="macos"
        translate={translate}
        onCategoryChange={() => {}}
      />
    );

    expect(screen.queryByRole("button", { name: "MCP" })).not.toBeInTheDocument();
  });

  it("shows logs as its own settings category", () => {
    const onCategoryChange = renderSettingsSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Logs" }));

    expect(onCategoryChange).toHaveBeenCalledWith("logs");
  });

  it("shows resources as its own settings category", () => {
    const onCategoryChange = renderSettingsSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Resources" }));

    expect(onCategoryChange).toHaveBeenCalledWith("resources");
  });

  it("can hide the desktop-only resources category", () => {
    render(
      <SettingsSidebar
        activeCategory="general"
        appVersion="9.9.9"
        hiddenCategories={["resources"]}
        platform="macos"
        translate={translate}
        onCategoryChange={() => {}}
      />
    );

    expect(screen.queryByRole("button", { name: "Resources" })).not.toBeInTheDocument();
  });

  it("shows templates as its own settings category", () => {
    const onCategoryChange = renderSettingsSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Templates" }));

    expect(onCategoryChange).toHaveBeenCalledWith("templates");
  });

   it("uses the keyboard shortcuts category title for the active panel", () => {
    render(
      <SettingsContent activeCategory="keyboardShortcuts" translate={translate}>
        <div />
      </SettingsContent>
    );

    expect(screen.getByRole("heading", { name: "Keyboard shortcuts" })).toBeInTheDocument();
  });

  it("uses the view category title for the active panel", () => {
    render(
      <SettingsContent activeCategory="view" translate={translate}>
        <div />
      </SettingsContent>
    );

    expect(screen.getByRole("heading", { name: "View" })).toBeInTheDocument();
  });

  it("uses the sync category title for the active panel", () => {
    render(
      <SettingsContent activeCategory="sync" translate={translate}>
        <div />
      </SettingsContent>
    );

    expect(screen.getByRole("heading", { name: "Sync" })).toBeInTheDocument();
  });

  it("uses the logs category title for the active panel", () => {
    render(
      <SettingsContent activeCategory="logs" translate={translate}>
        <div />
      </SettingsContent>
    );

    expect(screen.getByRole("heading", { name: "Logs" })).toBeInTheDocument();
  });

  it("uses the resources category title for the active panel", () => {
    render(
      <SettingsContent activeCategory="resources" translate={translate}>
        <div />
      </SettingsContent>
    );

    expect(screen.getByRole("heading", { name: "Resources" })).toBeInTheDocument();
  });

  it("uses the templates category title for the active panel", () => {
    render(
      <SettingsContent activeCategory="templates" translate={translate}>
        <div />
      </SettingsContent>
    );

    expect(screen.getByRole("heading", { name: "Templates" })).toBeInTheDocument();
  });

   it("resets the content scroll position when switching settings categories", () => {
    const { container, rerender } = render(
      <SettingsContent activeCategory="general" translate={translate}>
        <div />
      </SettingsContent>
    );
    const settingsScroll = container.querySelector(".settings-scroll") as HTMLElement;
    settingsScroll.scrollTop = 48;

    rerender(
      <SettingsContent activeCategory="sync" translate={translate}>
        <div />
      </SettingsContent>
    );

    expect(settingsScroll.scrollTop).toBe(0);
  });
});
