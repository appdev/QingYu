import { render, screen } from "@testing-library/react";

import { Badge } from "./Badge";

describe("Badge", () => {
  it("renders non-interactive status text", () => {
    render(<Badge aria-label="Reasoning">R</Badge>);

    const badge = screen.getByLabelText("Reasoning");
    expect(badge).toHaveClass("inline-flex");
    expect(badge).toHaveTextContent("R");
  });
});
