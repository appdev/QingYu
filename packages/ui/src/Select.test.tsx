import { fireEvent, render, screen } from "@testing-library/react";

import { Select } from "./Select";

describe("Select", () => {
  it("preserves native select change behavior", () => {
    const handleChange = vi.fn();

    render(
      <Select aria-label="Theme" value="light" onChange={handleChange}>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </Select>
    );

    fireEvent.change(screen.getByRole("combobox", { name: "Theme" }), { target: { value: "dark" } });
    expect(handleChange).toHaveBeenCalledTimes(1);
  });
});
