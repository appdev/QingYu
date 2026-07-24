import { fireEvent, render, screen } from "@testing-library/react";

import { TextInput } from "./TextInput";

describe("TextInput", () => {
  it("preserves native input change behavior", () => {
    const handleChange = vi.fn();

    render(<TextInput aria-label="File name" value="notes" onChange={handleChange} />);

    fireEvent.change(screen.getByRole("textbox", { name: "File name" }), { target: { value: "updated" } });
    expect(handleChange).toHaveBeenCalledTimes(1);
  });
});
