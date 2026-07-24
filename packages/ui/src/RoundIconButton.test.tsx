import { fireEvent, render, screen } from "@testing-library/react";

import { RoundIconButton } from "./RoundIconButton";

describe("RoundIconButton", () => {
  it("renders round icon action buttons", () => {
    const handleClick = vi.fn();

    render(
      <RoundIconButton label="Send message" size="lg" disabled={false} onClick={handleClick}>
        <span aria-hidden="true">↑</span>
      </RoundIconButton>
    );

    const button = screen.getByRole("button", { name: "Send message" });
    expect(button).toHaveAttribute("type", "button");
    expect(button).toHaveClass("rounded-full");
    expect(button).toHaveClass("size-10");

    fireEvent.click(button);
    expect(handleClick).toHaveBeenCalledTimes(1);
  });
});
