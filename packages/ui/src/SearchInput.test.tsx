import { fireEvent, render, screen } from "@testing-library/react";

import { SearchInput } from "./SearchInput";

describe("SearchInput", () => {
  it("renders a searchbox with optional leading icon and preserves change behavior", () => {
    const handleChange = vi.fn();

    render(
      <SearchInput
        aria-label="Search notes"
        icon={<span aria-hidden="true">S</span>}
        value="open"
        onChange={handleChange}
      />
    );

    const input = screen.getByRole("searchbox", { name: "Search notes" });
    expect(input).toHaveAttribute("type", "search");
    expect(input).toHaveClass("pl-8");
    expect(screen.getByText("S")).toHaveAttribute("aria-hidden", "true");

    fireEvent.change(input, { target: { value: "deep" } });
    expect(handleChange).toHaveBeenCalledTimes(1);
  });
});
