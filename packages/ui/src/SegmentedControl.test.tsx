import { fireEvent, render, screen } from "@testing-library/react";

import { SegmentedControl, SegmentedControlItem } from "./SegmentedControl";

describe("SegmentedControl", () => {
  it("renders selectable items inside a labelled group", () => {
    const handleLight = vi.fn();

    render(
      <SegmentedControl label="Theme">
        <SegmentedControlItem label="Light" selected={true} onClick={handleLight}>
          Light
        </SegmentedControlItem>
        <SegmentedControlItem label="Dark" selected={false}>
          Dark
        </SegmentedControlItem>
      </SegmentedControl>
    );

    expect(screen.getByRole("group", { name: "Theme" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Light" })).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(screen.getByRole("button", { name: "Light" }));
    expect(handleLight).toHaveBeenCalledTimes(1);
  });
});
