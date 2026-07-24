import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { vi } from "vitest";
import { CompactNameDialog } from "./CompactNameDialog";

function deferred() {
  let resolve!: () => undefined;
  const promise = new Promise<unknown>((resolvePromise) => {
    resolve = () => {
      resolvePromise(undefined);
      return undefined;
    };
  });
  return { promise, resolve };
}

function rejectedDeferred() {
  let reject!: (reason: unknown) => undefined;
  const promise = new Promise<unknown>((_resolvePromise, rejectPromise) => {
    reject = (reason: unknown) => {
      rejectPromise(reason);
      return undefined;
    };
  });
  return { promise, reject };
}

const defaultProps = {
  cancelLabel: "Cancel",
  errorMessage: "The file operation failed.",
  initialValue: "",
  onCancel: vi.fn(),
  onSubmit: vi.fn().mockResolvedValue(undefined),
  submitLabel: "Create",
  title: "New file name"
};

describe("CompactNameDialog", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("autofocuses and selects the initial value, trims submission, and uses mobile-safe targets", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <CompactNameDialog
        {...defaultProps}
        initialValue="  Draft.md  "
        onSubmit={onSubmit}
      />
    );

    const dialog = screen.getByRole("dialog", { name: "New file name" });
    const input = screen.getByRole("textbox", { name: "New file name" });
    expect(dialog.parentElement).toHaveClass(
      "pt-[var(--compact-safe-area-top)]",
      "pb-[var(--compact-safe-area-bottom)]"
    );
    await waitFor(() => expect(document.activeElement).toBe(input));
    expect(input).toHaveValue("  Draft.md  ");
    expect((input as HTMLInputElement).selectionStart).toBe(0);
    expect((input as HTMLInputElement).selectionEnd).toBe("  Draft.md  ".length);
    screen.getAllByRole("button").forEach((button) => {
      expect(button).toHaveClass("min-h-11", "min-w-11");
    });

    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledWith("Draft.md"));
  });

  it("disables submission while the trimmed name is empty", () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<CompactNameDialog {...defaultProps} onSubmit={onSubmit} />);

    const submit = screen.getByRole("button", { name: "Create" });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: "New file name" }), {
      target: { value: "   " }
    });
    expect(submit).toBeDisabled();
    fireEvent.click(submit);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("submits with Enter once and disables every action while pending", async () => {
    const pending = deferred();
    const onCancel = vi.fn();
    const onSubmit = vi.fn(() => pending.promise);
    render(
      <CompactNameDialog
        {...defaultProps}
        initialValue="Draft"
        onCancel={onCancel}
        onSubmit={onSubmit}
      />
    );

    const input = screen.getByRole("textbox", { name: "New file name" });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onSubmit).toHaveBeenCalledWith("Draft");
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(input).not.toBeDisabled();
    expect(input).toHaveAttribute("readonly");
    fireEvent.keyDown(input, { key: "Tab" });
    expect(document.activeElement).toBe(input);
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onCancel).not.toHaveBeenCalled();

    await act(async () => {
      pending.resolve();
      await pending.promise;
    });
  });

  it("closes with Escape while a dialog button is focused", async () => {
    const onCancel = vi.fn();
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>Open dialog</button>
          {open ? (
            <CompactNameDialog
              {...defaultProps}
              initialValue="Draft"
              onCancel={() => {
                onCancel();
                setOpen(false);
              }}
            />
          ) : null}
        </>
      );
    }

    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open dialog" });
    trigger.focus();
    fireEvent.click(trigger);
    const cancel = screen.getByRole("button", { name: "Cancel" });
    cancel.focus();

    fireEvent.keyDown(cancel, { key: "Escape" });

    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog", { name: "New file name" })).not.toBeInTheDocument();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("keeps the input focused while pending and editable after rejection", async () => {
    const pending = rejectedDeferred();
    render(
      <CompactNameDialog
        {...defaultProps}
        initialValue="Draft"
        onSubmit={() => pending.promise}
      />
    );

    const input = screen.getByRole("textbox", { name: "New file name" });
    await waitFor(() => expect(document.activeElement).toBe(input));
    fireEvent.keyDown(input, { key: "Enter" });

    expect(input).toHaveAttribute("readonly");
    expect(input).not.toBeDisabled();
    expect(document.activeElement).toBe(input);
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();

    await act(async () => {
      pending.reject(new Error("operation failed"));
      await Promise.resolve();
    });

    expect(await screen.findByRole("alert")).toHaveTextContent("The file operation failed.");
    expect(input).not.toHaveAttribute("readonly");
    expect(input).not.toBeDisabled();
    expect(document.activeElement).toBe(input);
  });

  it("traps Tab and Shift+Tab at the first and last dialog controls", async () => {
    render(
      <>
        <button type="button">Before dialog</button>
        <CompactNameDialog {...defaultProps} initialValue="Draft" />
        <button type="button">After dialog</button>
      </>
    );

    const input = screen.getByRole("textbox", { name: "New file name" });
    const submit = screen.getByRole("button", { name: "Create" });
    const before = screen.getByRole("button", { name: "Before dialog" });
    const after = screen.getByRole("button", { name: "After dialog" });
    await waitFor(() => expect(document.activeElement).toBe(input));

    fireEvent.keyDown(input, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(submit);
    expect(document.activeElement).not.toBe(before);

    fireEvent.keyDown(submit, { key: "Tab" });
    expect(document.activeElement).toBe(input);
    expect(document.activeElement).not.toBe(after);
  });

  it("cancels with Escape and restores the element focused before the dialog", async () => {
    const onCancel = vi.fn();
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>Open dialog</button>
          {open ? (
            <CompactNameDialog
              {...defaultProps}
              onCancel={() => {
                onCancel();
                setOpen(false);
              }}
            />
          ) : null}
        </>
      );
    }

    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open dialog" });
    trigger.focus();
    fireEvent.click(trigger);

    const input = screen.getByRole("textbox", { name: "New file name" });
    await waitFor(() => expect(document.activeElement).toBe(input));
    fireEvent.keyDown(input, { key: "Escape" });

    expect(onCancel).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("keeps the dialog open and shows only the supplied safe error after rejection", async () => {
    const onSubmit = vi.fn().mockRejectedValue(
      new Error("token=super-secret failed at /Users/example/private-note.md")
    );
    render(
      <CompactNameDialog
        {...defaultProps}
        errorMessage="The file operation failed."
        initialValue="Draft"
        onSubmit={onSubmit}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("The file operation failed.");
    expect(screen.queryByText(/super-secret|private-note/u)).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "New file name" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create" })).toBeEnabled();
  });
});
