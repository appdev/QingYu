import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { SyncConflictRecord } from "../../lib/sync-config";
import { SyncConflictDialog } from "./SyncConflictDialog";

const conflict: SyncConflictRecord = {
  conflictId: "00000000-0000-4000-8000-0000000000b1",
  occurredAt: "2026-07-28T10:00:00Z",
  relativePath: "folder/note.md",
  repositoryId: "00000000-0000-4000-8000-0000000000b2",
  resolution: null
};

const versions = {
  conflict,
  local: { byteSize: 10, text: "local\ntext" },
  remote: { byteSize: 11, text: "remote\ntext" }
};

describe("SyncConflictDialog", () => {
  beforeEach(() => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  afterEach(() => vi.restoreAllMocks());

  it("loads both versions and dispatches each exact explicit resolution", async () => {
    const onResolve = vi.fn(async () => undefined);
    const onClose = vi.fn();
    render(
      <SyncConflictDialog
        conflict={conflict}
        language="en"
        onClose={onClose}
        onRead={vi.fn(async () => versions)}
        onResolve={onResolve}
      />
    );
    expect(screen.getByRole("dialog")).toHaveAttribute("aria-modal", "false");
    expect(await screen.findByText((_, element) => (
      element?.tagName === "PRE" && element.textContent === "local\ntext"
    ))).toBeVisible();
    expect(screen.getByText((_, element) => (
      element?.tagName === "PRE" && element.textContent === "remote\ntext"
    ))).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Keep local" }));
    await waitFor(() => expect(onResolve).toHaveBeenCalledWith(conflict, { kind: "keep-local" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("keeps the panel open on failure and validates the keep-both relative path", async () => {
    const onResolve = vi.fn(async () => Promise.reject(new Error("failed")));
    const onClose = vi.fn();
    render(
      <SyncConflictDialog
        conflict={conflict}
        language="en"
        onClose={onClose}
        onRead={vi.fn(async () => versions)}
        onResolve={onResolve}
      />
    );
    await screen.findByText((_, element) => (
      element?.tagName === "PRE" && element.textContent === "local\ntext"
    ));
    const path = screen.getByLabelText("Relative path for the remote copy");
    fireEvent.change(path, { target: { value: "../outside.md" } });
    expect(screen.getByRole("button", { name: "Keep both" })).toBeDisabled();
    fireEvent.change(path, { target: { value: "folder/note.remote.md" } });
    fireEvent.click(screen.getByRole("button", { name: "Keep both" }));

    await waitFor(() => expect(onResolve).toHaveBeenCalledWith(conflict, {
      destinationRelativePath: "folder/note.remote.md",
      kind: "keep-both"
    }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be resolved");
    expect(onClose).not.toHaveBeenCalled();
  });
});
