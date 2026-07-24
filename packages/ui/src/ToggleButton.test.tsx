import { fireEvent, render, screen } from "@testing-library/react";

import { ToggleButton } from "./ToggleButton";

describe("ToggleButton", () => {
  it("renders pill toggle buttons with pressed state", () => {
    const handleClick = vi.fn();

    render(
      <ToggleButton label="Deep thinking" pressed={false} onClick={handleClick}>
        Deep thinking
      </ToggleButton>
    );

    const button = screen.getByRole("button", { name: "Deep thinking" });
    expect(button).toHaveAttribute("aria-pressed", "false");
    expect(button).toHaveClass("rounded-full");

    fireEvent.click(button);
    expect(handleClick).toHaveBeenCalledTimes(1);
  });
});
