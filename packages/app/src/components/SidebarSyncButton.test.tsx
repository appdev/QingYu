import { fireEvent, render, screen } from "@testing-library/react";

import { SidebarSyncButton, type SidebarSyncButtonState } from "./SidebarSyncButton";

describe("SidebarSyncButton", () => {
  it("renders the idle state as an available cloud action", () => {
    const { container } = render(
      <SidebarSyncButton language="en" onSync={vi.fn()} state="idle" />
    );

    expect(screen.getByRole("button", { name: "Sync now" })).toHaveAttribute("data-sync-state", "idle");
    expect(container.querySelector(".lucide-cloud")).toBeInTheDocument();
    expect(container.querySelector(".lucide-cloud")).toHaveAttribute("width", "15");
    expect(container.querySelector(".lucide-cloud")).toHaveAttribute("height", "15");
  });

  it("uses the default base opacity without the muted opacity", () => {
    render(<SidebarSyncButton language="en" onSync={vi.fn()} state="idle" />);

    const syncButton = screen.getByRole("button", { name: "Sync now" });

    expect(syncButton).toHaveClass("opacity-70");
    expect(syncButton).not.toHaveClass("opacity-40");
  });

  it("uses the muted base opacity without the default opacity", () => {
    render(<SidebarSyncButton language="en" muted onSync={vi.fn()} state="idle" />);

    const syncButton = screen.getByRole("button", { name: "Sync now" });

    expect(syncButton).toHaveClass("opacity-40");
    expect(syncButton).not.toHaveClass("opacity-70");
  });

  it("renders unavailable, failed, and succeeded state details", () => {
    const onSync = vi.fn();
    const { container, rerender } = render(
      <SidebarSyncButton language="en" onSync={onSync} state="unavailable" />
    );

    expect(screen.getByRole("button", { name: "Sync now · Disabled" }))
      .toHaveAttribute("data-sync-state", "unavailable");
    expect(container.querySelector(".lucide-cloud-off")).toBeInTheDocument();
    expect(container.querySelector(".lucide-cloud-off")).toHaveAttribute("width", "15");
    expect(container.querySelector(".lucide-cloud-off")).toHaveAttribute("height", "15");

    rerender(<SidebarSyncButton language="en" onSync={onSync} state="failed" />);

    expect(screen.getByRole("button", { name: "Sync now · Failed" }))
      .toHaveAttribute("data-sync-state", "failed");
    expect(container.querySelector(".lucide-cloud")).toBeInTheDocument();
    expect(container.querySelector(".lucide-x")).toHaveClass("text-(--danger)");

    rerender(<SidebarSyncButton language="en" onSync={onSync} state="succeeded" />);

    expect(screen.getByRole("button", { name: "Sync now · Succeeded" }))
      .toHaveAttribute("data-sync-state", "succeeded");
    expect(container.querySelector(".lucide-cloud")).toBeInTheDocument();
    expect(container.querySelector(".lucide-check")).toHaveClass("text-(--accent)");
  });

  it("renders running as a disabled busy action with a spinning loader", () => {
    const { container } = render(
      <SidebarSyncButton language="en" onSync={vi.fn()} state="running" />
    );

    expect(screen.getByRole("button", { name: "Syncing..." })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Syncing..." })).toHaveAttribute("aria-busy", "true");
    expect(container.querySelector(".lucide-loader-circle")).toHaveClass("animate-spin");
    expect(container.querySelector(".lucide-loader-circle")).toHaveAttribute("width", "15");
    expect(container.querySelector(".lucide-loader-circle")).toHaveAttribute("height", "15");
  });

  it.each<SidebarSyncButtonState>(["idle", "unavailable", "failed", "succeeded"])(
    "runs sync when the %s control is clicked",
    (state) => {
      const onSync = vi.fn();
      render(<SidebarSyncButton language="en" onSync={onSync} state={state} />);

      fireEvent.click(screen.getByRole("button"));

      expect(onSync).toHaveBeenCalledTimes(1);
    }
  );

  it.each([
    { disabled: false, state: "running" as const },
    { disabled: true, state: "idle" as const }
  ])("does not run sync for a disabled $state control", ({ disabled, state }) => {
    const onSync = vi.fn();
    render(
      <SidebarSyncButton
        disabled={disabled}
        language="en"
        onSync={onSync}
        state={state}
      />
    );

    fireEvent.click(screen.getByRole("button"));

    expect(onSync).not.toHaveBeenCalled();
  });
});
