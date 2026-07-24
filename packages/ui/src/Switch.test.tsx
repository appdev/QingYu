import { fireEvent, render, screen } from "@testing-library/react";

import { Switch } from "./Switch";

describe("Switch", () => {
  it("reports the next checked state", () => {
    const handleChange = vi.fn();

    render(<Switch checked={false} label="Enable auto-save" onCheckedChange={handleChange} />);

    const switchButton = screen.getByRole("switch", { name: "Enable auto-save" });
    expect(switchButton).toHaveAttribute("aria-checked", "false");

    fireEvent.click(switchButton);
    expect(handleChange).toHaveBeenCalledWith(true);
  });
});
