import { render, screen } from "@testing-library/react";
import type { SyncConflictRecord } from "../../lib/sync-config";
import { SyncConflictHistoryDialog } from "./SyncConflictHistoryDialog";

const conflict: SyncConflictRecord = {
  conflictId: "00000000-0000-4000-8000-0000000000b1",
  copyError: null,
  copyPath: null,
  copyStatus: "not-requested",
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

  it("shows a generated Markdown conflict copy path", async () => {
    render(
      <SyncConflictHistoryDialog
        conflict={{
          ...conflict,
          copyStatus: "generated",
          copyPath: "folder/note-Conflicted-20260804-153000.md",
          copyError: null
        }}
        language="en"
        onClose={vi.fn()}
        onRead={vi.fn(async () => ({
          ...versions,
          conflict: {
            ...conflict,
            copyStatus: "generated" as const,
            copyPath: "folder/note-Conflicted-20260804-153000.md",
            copyError: null
          }
        }))}
      />
    );

    expect(await screen.findByText("Conflict copy created")).toBeVisible();
    expect(screen.getByText("folder/note-Conflicted-20260804-153000.md")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Use remote" })).not.toBeInTheDocument();
  });

  it("shows skipped and failed copy states without resolution controls", async () => {
    const { rerender } = render(
      <SyncConflictHistoryDialog
        conflict={{ ...conflict, copyStatus: "skipped", copyPath: null, copyError: null }}
        language="en"
        onClose={vi.fn()}
        onRead={vi.fn(async () => ({
          ...versions,
          conflict: {
            ...conflict,
            copyStatus: "skipped" as const,
            copyPath: null,
            copyError: null
          }
        }))}
      />
    );

    expect(await screen.findByText("Only Markdown conflicts create visible copies.")).toBeVisible();

    rerender(
      <SyncConflictHistoryDialog
        conflict={{
          ...conflict,
          conflictId: "00000000-0000-4000-8000-0000000000c9",
          copyStatus: "failed",
          copyPath: null,
          copyError: "dejavu-working-tree-changed"
        }}
        language="en"
        onClose={vi.fn()}
        onRead={vi.fn(async () => ({
          ...versions,
          conflict: {
            ...conflict,
            copyStatus: "failed" as const,
            copyPath: null,
            copyError: "dejavu-working-tree-changed"
          }
        }))}
      />
    );

    expect(await screen.findByText("Conflict copy could not be created.")).toBeVisible();
    expect(screen.getByText("dejavu-working-tree-changed")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Keep both" })).not.toBeInTheDocument();
  });
});
