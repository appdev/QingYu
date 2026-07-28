import { render, screen } from "@testing-library/react";
import type { SyncConflictRecord } from "../../lib/sync-config";
import { SyncConflictHistoryDialog } from "./SyncConflictHistoryDialog";

const conflict: SyncConflictRecord = {
  conflictId: "00000000-0000-4000-8000-0000000000b1",
  occurredAt: "2026-07-28T10:00:00Z",
  relativePath: "folder/note.md",
  repositoryId: "00000000-0000-4000-8000-0000000000b2",
  resolution: "keep-local"
};

const versions = {
  conflict,
  local: { byteSize: 10, text: "local\ntext" },
  remote: { byteSize: 11, text: "remote\ntext" }
};

describe("SyncConflictHistoryDialog", () => {
  it("shows both actual versions as read-only history without resolution controls", async () => {
    const onClose = vi.fn();
    render(
      <SyncConflictHistoryDialog
        conflict={conflict}
        language="en"
        onClose={onClose}
        onRead={vi.fn(async () => versions)}
      />
    );
    expect(screen.getByRole("dialog")).toHaveAttribute("aria-modal", "false");
    expect(await screen.findByText((_, element) => (
      element?.tagName === "PRE" && element.textContent === "local\ntext"
    ))).toBeVisible();
    expect(screen.getByText((_, element) => (
      element?.tagName === "PRE" && element.textContent === "remote\ntext"
    ))).toBeVisible();
    expect(screen.queryByRole("button", { name: "Keep local" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Use remote" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Keep both" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Relative path for the remote copy")).not.toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("distinguishes a deleted local file from a binary or oversized version", async () => {
    render(
      <SyncConflictHistoryDialog
        conflict={conflict}
        language="en"
        onClose={vi.fn()}
        onRead={vi.fn(async () => ({ ...versions, local: null }))}
      />
    );

    expect(await screen.findByText("The local file no longer exists.")).toBeVisible();
    expect(screen.queryByText("Preview unavailable for a binary or large file."))
      .not.toBeInTheDocument();
  });
});
