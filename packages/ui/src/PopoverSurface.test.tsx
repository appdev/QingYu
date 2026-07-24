import { render, screen } from "@testing-library/react";

import { PopoverSurface } from "./PopoverSurface";

describe("PopoverSurface", () => {
  it("renders a floating surface with open and closed state classes", () => {
    const { rerender } = render(
      <PopoverSurface open role="menu" aria-label="Actions" openClassName="shown" closedClassName="hidden">
        <button type="button">Rename</button>
      </PopoverSurface>
    );

    const surface = screen.getByRole("menu", { name: "Actions" });
    expect(surface).toHaveClass("border-(--border-default)");
    expect(surface).toHaveClass("shown");
    expect(surface).not.toHaveClass("hidden");

    rerender(
      <PopoverSurface open={false} role="menu" aria-label="Actions" openClassName="shown" closedClassName="hidden">
        <button type="button">Rename</button>
      </PopoverSurface>
    );

    expect(screen.getByRole("menu", { name: "Actions" })).toHaveClass("hidden");
  });
});
