import { fireEvent, render, screen } from "@testing-library/react";
import { SettingsModalFrame } from "./SettingsModalFrame";

describe("SettingsModalFrame", () => {
  it.each(["macos", "windows", "linux"] as const)(
    "renders one %s close control in a fixed settings dialog",
    (platform) => {
      const onClose = vi.fn();

      render(
        <SettingsModalFrame label="Settings" onClose={onClose} platform={platform}>
          <p>Settings content</p>
        </SettingsModalFrame>
      );

      const dialog = screen.getByRole("dialog", { name: "Settings" });
      const close = screen.getByRole("button", { name: "Close window" });

      expect(screen.getAllByRole("button")).toHaveLength(1);
      expect(dialog).toHaveClass(
        "h-[720px]",
        "w-[1040px]",
        "max-h-[calc(100dvh-32px)]",
        "max-w-[calc(100vw-32px)]"
      );
      expect(close).toHaveAttribute("data-settings-modal-close", platform);

      fireEvent.click(close);

      expect(onClose).toHaveBeenCalledTimes(1);
    }
  );

  it("dismisses with Escape but not from the backdrop", () => {
    const onClose = vi.fn();

    render(
      <SettingsModalFrame label="Settings" onClose={onClose} platform="linux">
        <p>Settings content</p>
      </SettingsModalFrame>
    );

    fireEvent.mouseDown(screen.getByTestId("settings-modal-backdrop"));
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.keyDown(screen.getByRole("dialog", { name: "Settings" }), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("contains keyboard focus and restores the previous focus on unmount", () => {
    const opener = document.createElement("button");
    opener.textContent = "Open settings";
    document.body.append(opener);
    opener.focus();

    const view = render(
      <SettingsModalFrame label="Settings" onClose={vi.fn()} platform="windows">
        <button type="button">Last setting action</button>
      </SettingsModalFrame>
    );

    const close = screen.getByRole("button", { name: "Close window" });
    const lastAction = screen.getByRole("button", { name: "Last setting action" });
    expect(close).toHaveFocus();

    lastAction.focus();
    fireEvent.keyDown(lastAction, { key: "Tab" });
    expect(close).toHaveFocus();

    fireEvent.keyDown(close, { key: "Tab", shiftKey: true });
    expect(lastAction).toHaveFocus();

    view.unmount();
    expect(opener).toHaveFocus();
    opener.remove();
  });
});
