import { fireEvent, render, screen } from "@testing-library/react";

import { Textarea } from "./Textarea";

describe("Textarea", () => {
  it("preserves native textarea change behavior", () => {
    const handleChange = vi.fn();

    render(<Textarea aria-label="Headers JSON" value="{}" onChange={handleChange} />);

    fireEvent.change(screen.getByRole("textbox", { name: "Headers JSON" }), { target: { value: "{\"x\":1}" } });
    expect(handleChange).toHaveBeenCalledTimes(1);
  });
});
