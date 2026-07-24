import { render, screen } from "@testing-library/react";

import { Field } from "./Field";
import { TextInput } from "./TextInput";

describe("Field", () => {
  it("connects labels, descriptions, and errors to the control", () => {
    render(
      <Field label="Folder path" description="Storage location" error="Invalid path">
        <TextInput />
      </Field>
    );

    expect(screen.getByLabelText("Folder path")).toHaveAccessibleDescription("Storage location Invalid path");
    expect(screen.getByText("Invalid path")).toHaveAttribute("role", "alert");
  });
});
