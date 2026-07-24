import { fireEvent, render, screen } from "@testing-library/react";

import { Button } from "./Button";
import { Modal } from "./Modal";

describe("Modal", () => {
  it("renders a dialog with header, body, footer, and close action", () => {
    const handleClose = vi.fn();

    render(
      <Modal
        title="Custom headers"
        closeLabel="Close custom headers"
        closeIcon={<span aria-hidden="true">x</span>}
        footer={<Button variant="primary">Save</Button>}
        onClose={handleClose}
      >
        <p>Editor body</p>
      </Modal>
    );

    expect(screen.getByRole("dialog", { name: "Custom headers" })).toBeInTheDocument();
    expect(screen.getByText("Editor body")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Close custom headers" }));
    expect(handleClose).toHaveBeenCalledTimes(1);
  });
});
