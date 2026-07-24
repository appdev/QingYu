import { useLayoutEffect, useState } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { RemoteNotebookCatalogEntry } from "../../runtime";
import { RemoteNotebookDialog } from "./RemoteNotebookDialog";

function entry(
  name: string,
  options: Partial<Omit<RemoteNotebookCatalogEntry, "name">> = {}
): RemoteNotebookCatalogEntry {
  return {
    available: true,
    disabledReason: null,
    name,
    ...options
  };
}

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

const defaultProps = {
  entries: [
    entry("Archive"),
    entry("晨间札记"),
    entry("不可用", { available: false, disabledReason: "notebook-name-unavailable" }),
    entry("服务不可用", { available: false, disabledReason: "internal-provider-403" })
  ],
  error: null,
  loading: false,
  onCancel: vi.fn(),
  onRefresh: vi.fn(async () => undefined),
  onRestore: vi.fn(async () => undefined)
};

describe("RemoteNotebookDialog", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("shows only safe notebook names and restores exactly one enabled selection", async () => {
    const onRestore = vi.fn(async () => undefined);
    render(<RemoteNotebookDialog {...defaultProps} onRestore={onRestore} />);

    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("radio", { name: "Archive" })).toBeEnabled();
    expect(within(dialog).getByRole("radio", { name: "晨间札记" })).toBeEnabled();
    expect(within(dialog).getByRole("radio", { name: "不可用" })).toBeDisabled();
    expect(within(dialog).getByText("This notebook name cannot be used.")).toBeVisible();
    expect(within(dialog).getByText("This cloud notebook is unavailable.")).toBeVisible();
    expect(dialog).not.toHaveTextContent("notebook-name-unavailable");
    expect(dialog).not.toHaveTextContent("internal-provider-403");
    expect(dialog).not.toHaveTextContent(/https?:\/\/|access.?key|password|secret/iu);

    const restore = within(dialog).getByRole("button", { name: /restore/iu });
    expect(restore).toBeDisabled();
    fireEvent.click(within(dialog).getByRole("radio", { name: "晨间札记" }));

    expect(within(dialog).getByRole("radio", { name: "Archive" })).not.toBeChecked();
    expect(within(dialog).getByRole("radio", { name: "晨间札记" })).toBeChecked();
    expect(restore).toBeEnabled();
    expect(within(dialog).getByText(/same-name local folder.*reuse.*merge/iu)).toBeVisible();

    fireEvent.click(restore);
    await waitFor(() => expect(onRestore).toHaveBeenCalledWith("晨间札记"));
    expect(onRestore).toHaveBeenCalledTimes(1);
  });

  it("marks the current notebook directory and prevents restoring it again", async () => {
    const onRestore = vi.fn(async () => undefined);
    render(
      <RemoteNotebookDialog
        {...defaultProps}
        currentNotebookName="Archive"
        entries={[entry("Archive"), entry("晨间札记")]}
        onRestore={onRestore}
      />
    );

    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("radio", { name: "Archive" })).toBeDisabled();
    expect(within(dialog).getByText("Current notebook directory")).toBeVisible();

    fireEvent.click(within(dialog).getByRole("radio", { name: "晨间札记" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Restore" }));

    await waitFor(() => expect(onRestore).toHaveBeenCalledWith("晨间札记"));
    expect(onRestore).not.toHaveBeenCalledWith("Archive");
  });

  it("allows the current remote directory to be chosen during first-sync discovery", async () => {
    const onRestore = vi.fn(async () => undefined);
    render(
      <RemoteNotebookDialog
        {...defaultProps}
        allowCurrentNotebookSelection
        currentNotebookName="Archive"
        entries={[entry("Archive")]}
        onRestore={onRestore}
      />
    );

    fireEvent.click(screen.getByRole("radio", { name: "Archive" }));
    fireEvent.click(screen.getByRole("button", { name: "Sync now" }));

    await waitFor(() => expect(onRestore).toHaveBeenCalledWith("Archive"));
  });

  it.each(["current", "unavailable"] as const)(
    "blocks a selected notebook synchronously when it becomes %s",
    async (blockedBy) => {
      const onRestore = vi.fn(async () => undefined);
      const observedBeforePassiveEffect = vi.fn();

      function BlockingHarness() {
        const [blocked, setBlocked] = useState(false);
        useLayoutEffect(() => {
          if (!blocked) return;
          const restoreButton = Array.from(document.querySelectorAll("button"))
            .find((button) => button.textContent === "Restore") as HTMLButtonElement | undefined;
          observedBeforePassiveEffect({
            mergeWarningVisible: document.body.textContent?.includes(
              "A same-name local folder will be reused"
            ),
            restoreDisabled: restoreButton?.disabled
          });
          if (restoreButton) {
            restoreButton.disabled = false;
            restoreButton.click();
          }
        }, [blocked]);

        return (
          <>
            <button type="button" onClick={() => setBlocked(true)}>Block selection</button>
            <RemoteNotebookDialog
              {...defaultProps}
              currentNotebookName={blocked && blockedBy === "current" ? "B" : null}
              entries={[
                entry("A"),
                entry("B", {
                  available: !blocked || blockedBy !== "unavailable",
                  disabledReason: blocked && blockedBy === "unavailable"
                    ? "notebook-name-unavailable"
                    : null
                })
              ]}
              onRestore={onRestore}
            />
          </>
        );
      }

      render(<BlockingHarness />);
      fireEvent.click(screen.getByRole("radio", { name: "B" }));
      expect(screen.getByText(/same-name local folder/iu)).toBeVisible();

      fireEvent.click(screen.getByRole("button", { name: "Block selection" }));

      expect(observedBeforePassiveEffect).toHaveBeenCalledWith({
        mergeWarningVisible: false,
        restoreDisabled: true
      });
      await waitFor(() => expect(onRestore).not.toHaveBeenCalled());
    }
  );

  it("renders loading, empty, and retryable error states without exposing a restore action", async () => {
    const onRefresh = vi.fn(async () => undefined);
    const { rerender } = render(
      <RemoteNotebookDialog
        {...defaultProps}
        entries={[]}
        loading
        onRefresh={onRefresh}
      />
    );

    expect(screen.getByRole("status")).toHaveTextContent(/loading/iu);
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /restore/iu })).toBeDisabled();

    rerender(
      <RemoteNotebookDialog
        {...defaultProps}
        entries={[]}
        error={null}
        loading={false}
        onRefresh={onRefresh}
      />
    );
    expect(screen.getByText(/no cloud notebooks/iu)).toBeVisible();

    rerender(
      <RemoteNotebookDialog
        {...defaultProps}
        entries={[]}
        error="The cloud notebook list could not be loaded."
        loading={false}
        onRefresh={onRefresh}
      />
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "The cloud notebook list could not be loaded."
    );
    fireEvent.click(screen.getByRole("button", { name: /try again|refresh/iu }));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledOnce());
  });

  it("locks dismissal and selection during bootstrap, then supports Escape dismissal", async () => {
    const pending = deferred();
    const onCancel = vi.fn();
    const onRestore = vi.fn(() => pending.promise);
    render(
      <RemoteNotebookDialog
        {...defaultProps}
        entries={[entry("Archive"), entry("晨间札记")]}
        onCancel={onCancel}
        onRestore={onRestore}
      />
    );

    fireEvent.click(screen.getByRole("radio", { name: "Archive" }));
    fireEvent.click(screen.getByRole("button", { name: /restore/iu }));

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("status")).toHaveTextContent(/restoring|preparing/iu);
    expect(screen.getByRole("radio", { name: "Archive" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /cancel|close/iu })).toBeDisabled();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onCancel).not.toHaveBeenCalled();

    await act(async () => {
      pending.resolve();
      await pending.promise;
    });

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("contains Tab and Shift+Tab focus within the dialog", () => {
    render(
      <>
        <button type="button">Before dialog</button>
        <RemoteNotebookDialog {...defaultProps} entries={[entry("Archive")]} />
        <button type="button">After dialog</button>
      </>
    );

    const dialog = screen.getByRole("dialog");
    const cancel = screen.getByRole("button", { name: "Cancel" });
    expect(document.activeElement).toBe(dialog);
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(cancel);

    const first = screen.getByRole("radio", { name: "Archive" });
    fireEvent.click(first);
    const last = screen.getByRole("button", { name: "Restore" });

    first.focus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);
    expect(document.activeElement).not.toBe(screen.getByRole("button", { name: "Before dialog" }));

    last.focus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(document.activeElement).toBe(first);
    expect(document.activeElement).not.toBe(screen.getByRole("button", { name: "After dialog" }));
  });

  it("keeps Tab and Shift+Tab on the dialog container while every control is busy", async () => {
    const pending = deferred();
    render(
      <>
        <button type="button">Outside dialog</button>
        <RemoteNotebookDialog
          {...defaultProps}
          entries={[entry("Archive")]}
          onRestore={() => pending.promise}
        />
      </>
    );
    fireEvent.click(screen.getByRole("radio", { name: "Archive" }));
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    const dialog = screen.getByRole("dialog");
    const outside = screen.getByRole("button", { name: "Outside dialog" });

    outside.focus();
    expect(fireEvent.keyDown(dialog, { key: "Tab" })).toBe(false);
    expect(document.activeElement).toBe(dialog);

    outside.focus();
    expect(fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true })).toBe(false);
    expect(document.activeElement).toBe(dialog);

    await act(async () => {
      pending.resolve();
      await pending.promise;
    });
  });
});
