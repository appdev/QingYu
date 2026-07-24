import { fireEvent, render, screen } from "@testing-library/react";

import { IconButton } from "./IconButton";

describe("IconButton", () => {
  it("renders an accessible label, optional pressed state, and icon sizing", () => {
    const handleClick = vi.fn();

    render(
      <>
        <IconButton label="Toggle sidebar" pressed={true} onClick={handleClick}>
          <span aria-hidden="true">S</span>
        </IconButton>
        <IconButton label="Large action" size="icon-lg">
          <span aria-hidden="true">L</span>
        </IconButton>
        <IconButton label="Tiny action" size="icon-xs">
          <span aria-hidden="true">T</span>
        </IconButton>
      </>
    );

    const button = screen.getByRole("button", { name: "Toggle sidebar" });
    expect(button).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Large action" })).toHaveClass("size-10");
    expect(screen.getByRole("button", { name: "Tiny action" })).toHaveClass("size-6");

    fireEvent.click(button);
    expect(handleClick).toHaveBeenCalledTimes(1);
  });
});
