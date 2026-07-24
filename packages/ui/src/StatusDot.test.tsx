import { render, screen } from "@testing-library/react";

import { StatusDot } from "./StatusDot";

describe("StatusDot", () => {
  it("renders active and inactive status markers", () => {
    render(
      <>
        <StatusDot aria-label="Enabled" tone="active" />
        <StatusDot aria-label="Disabled" tone="inactive" />
      </>
    );

    expect(screen.getByLabelText("Enabled")).toHaveClass("bg-(--accent)");
    expect(screen.getByLabelText("Disabled")).toHaveClass("bg-(--border-strong)");
  });
});
