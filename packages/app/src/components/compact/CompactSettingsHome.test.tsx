import { fireEvent, render, screen } from "@testing-library/react";
import type { CompactNavigation } from "../../hooks/useCompactNavigation";
import { CompactSettingsHome } from "./CompactSettingsHome";
import type { CompactAppController } from "./types";

function navigation(): CompactNavigation {
  return {
    canGoBack: true,
    page: { kind: "settings" },
    pop: vi.fn(async () => true),
    popIfCurrent: vi.fn(async () => true),
    popToEditor: vi.fn(async () => true),
    push: vi.fn(() => true),
    replace: vi.fn(() => true),
    stack: [{ kind: "editor" }, { kind: "settings" }]
  };
}

function controller(capabilities: Partial<CompactAppController["capabilities"]> = {}) {
  return {
    capabilities: {
      applicationSync: true,
      mcpPolicy: true,
      trueMobile: false,
      ...capabilities
    },
    language: "en"
  } as unknown as CompactAppController;
}

describe("CompactSettingsHome", () => {
  it("renders the policy order in grouped, full-screen, vertically scrollable content", () => {
    render(<CompactSettingsHome controller={controller()} navigation={navigation()} />);

    const settings = screen.getByRole("region", { name: "Settings" });
    expect(settings).toHaveClass("absolute", "inset-0", "overflow-x-hidden");
    expect(settings.querySelector("header")).toHaveClass("pt-[var(--compact-safe-area-top)]");
    expect(settings.querySelector("[data-compact-settings-scroll]"))
      .toHaveAttribute("data-compact-scroll", "vertical");
    expect(settings.querySelector("[data-compact-settings-scroll]"))
      .toHaveClass("overflow-y-auto", "overflow-x-hidden", "pb-[calc(1.25rem+var(--compact-bottom-inset))]");
    expect(screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent))
      .toEqual(["App", "Workspace", "Editing experience"]);
    expect(Array.from(settings.querySelectorAll("[data-compact-settings-category]"), (element) =>
      element.getAttribute("data-compact-settings-category")))
      .toEqual(["general", "mcp", "storage", "sync", "appearance", "editor"]);
  });

  it("uses 44px targets, filters runtime-only categories, and never leaks desktop settings", () => {
    const { rerender } = render(
      <CompactSettingsHome
        controller={controller({ applicationSync: false })}
        navigation={navigation()}
      />
    );

    screen.getAllByRole("button").forEach((button) => {
      expect(button).toHaveClass("min-h-11", "min-w-11");
    });
    expect(screen.queryByRole("button", { name: "Sync" })).not.toBeInTheDocument();
    expect(screen.queryByText(/AI|Web Search|View|Templates|Keyboard Shortcuts|Export|Network|Logs|About/i))
      .not.toBeInTheDocument();

    rerender(<CompactSettingsHome controller={controller()} navigation={navigation()} />);
    expect(screen.getByRole("button", { name: "Sync" })).toBeInTheDocument();
  });

  it("routes Sync into the existing status screen and other rows into curated details", () => {
    const nav = navigation();
    render(<CompactSettingsHome controller={controller()} navigation={nav} />);

    fireEvent.click(screen.getByRole("button", { name: "Sync" }));
    expect(nav.push).toHaveBeenCalledWith({ kind: "sync-status" });

    fireEvent.click(screen.getByRole("button", { name: "Storage" }));
    expect(nav.push).toHaveBeenCalledWith({ kind: "settings-detail", category: "storage" });

    fireEvent.click(screen.getByRole("button", { name: "MCP" }));
    expect(nav.push).toHaveBeenCalledWith({ kind: "settings-detail", category: "mcp" });
  });
});
