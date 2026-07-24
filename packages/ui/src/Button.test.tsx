import { fireEvent, render, screen } from "@testing-library/react";

import { Button } from "./Button";

describe("Button", () => {
  it("renders variants while preserving native button props", () => {
    const handleClick = vi.fn();

    render(
      <Button aria-label="Save changes" variant="primary" onClick={handleClick}>
        Save
      </Button>
    );

    const button = screen.getByRole("button", { name: "Save changes" });
    expect(button).toHaveAttribute("type", "button");
    expect(button).toHaveClass("bg-(--accent)");

    fireEvent.click(button);
    expect(handleClick).toHaveBeenCalledTimes(1);
  });
});
