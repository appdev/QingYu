import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { RemoteNotebookCatalogEntry } from "../../runtime";
import { MobileNotebookDialog } from "./MobileNotebookDialog";

function remoteEntry(
  name: string,
  available = true,
  disabledReason: string | null = null
): RemoteNotebookCatalogEntry {
  return { available, disabledReason, name };
}

const defaultProps = {
  error: null,
  loading: false,
  localNames: ["Archive", "随笔"],
  onCancel: vi.fn(),
  onCreate: vi.fn(async (_name: string) => undefined),
  onRefresh: vi.fn(async () => undefined),
  onRestore: vi.fn(async (_name: string) => undefined),
  onSwitch: vi.fn(async (_name: string) => undefined),
  remoteEntries: [remoteEntry("云端札记")]
};

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

describe("MobileNotebookDialog", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("creates one validated named notebook without exposing a filesystem picker", async () => {
    const onCreate = vi.fn(async (_name: string) => undefined);
    render(<MobileNotebookDialog {...defaultProps} onCreate={onCreate} />);

    const dialog = screen.getByRole("dialog");
    expect(dialog).not.toHaveTextContent(/choose folder|filesystem|path/iu);
    const input = within(dialog).getByRole("textbox", { name: /notebook name/iu });
    const create = within(dialog).getByRole("button", { name: /^create$/iu });
    expect(create).toBeDisabled();

    fireEvent.change(input, { target: { value: "bad/name" } });
    expect(within(dialog).getByRole("alert")).toHaveTextContent(/cannot include|invalid/iu);
    expect(create).toBeDisabled();

    fireEvent.change(input, { target: { value: "  我的笔记  " } });
    expect(within(dialog).queryByRole("alert")).not.toBeInTheDocument();
    expect(create).toBeEnabled();
    fireEvent.click(create);

    await waitFor(() => expect(onCreate).toHaveBeenCalledWith("  我的笔记  "));
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it.each([".QINGYU", ".MARKRA-SYNC", ".markra-sync-stage-interrupted"])(
    "rejects protected local notebook name %s before creation",
    (protectedName) => {
      const onCreate = vi.fn(async (_name: string) => undefined);
      render(<MobileNotebookDialog {...defaultProps} onCreate={onCreate} />);

      const dialog = screen.getByRole("dialog");
      fireEvent.change(within(dialog).getByRole("textbox", { name: /notebook name/iu }), {
        target: { value: protectedName }
      });

      expect(within(dialog).getByRole("alert")).toBeVisible();
      expect(within(dialog).getByRole("button", { name: /^create$/iu })).toBeDisabled();
      expect(onCreate).not.toHaveBeenCalled();
    }
  );

  it("lists managed children and switches only the selected exact notebook name", async () => {
    const onSwitch = vi.fn(async (_name: string) => undefined);
    render(<MobileNotebookDialog {...defaultProps} onSwitch={onSwitch} />);

    const localList = screen.getByRole("list", { name: /on this device|local notebooks/iu });
    expect(within(localList).getByRole("button", { name: "Archive" })).toBeVisible();
    expect(within(localList).getByRole("button", { name: "随笔" })).toBeVisible();

    fireEvent.click(within(localList).getByRole("button", { name: "随笔" }));
    await waitFor(() => expect(onSwitch).toHaveBeenCalledWith("随笔"));
    expect(onSwitch).toHaveBeenCalledTimes(1);
  });

  it("restores one enabled remote child into the managed workspace and warns before same-name merge", async () => {
    const onRestore = vi.fn(async (_name: string) => undefined);
    render(
      <MobileNotebookDialog
        {...defaultProps}
        localNames={["Archive", "云端札记"]}
        onRestore={onRestore}
        remoteEntries={[
          remoteEntry("云端札记"),
          remoteEntry("不可用", false, "notebook-name-unavailable"),
          remoteEntry("服务不可用", false, "internal-provider-403")
        ]}
      />
    );

    const remoteList = screen.getByRole("list", { name: /cloud notebooks/iu });
    expect(within(remoteList).getByRole("button", { name: "不可用" })).toBeDisabled();
    expect(within(remoteList).getByText("This notebook name cannot be used.")).toBeVisible();
    expect(within(remoteList).getByText("This cloud notebook is unavailable.")).toBeVisible();
    expect(remoteList).not.toHaveTextContent("notebook-name-unavailable");
    expect(remoteList).not.toHaveTextContent("internal-provider-403");
    fireEvent.click(within(remoteList).getByRole("button", { name: "云端札记" }));
    expect(screen.getByText(/already exists.*merge|same-name.*merge/iu)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: /^restore$/iu }));

    await waitFor(() => expect(onRestore).toHaveBeenCalledWith("云端札记"));
    expect(onRestore).toHaveBeenCalledTimes(1);
    expect(onRestore).not.toHaveBeenCalledWith("Archive");
  });

  it("provides explicit loading, empty, retry, and Escape dismissal states", async () => {
    const onCancel = vi.fn();
    const onRefresh = vi.fn(async () => undefined);
    const { rerender } = render(
      <MobileNotebookDialog
        {...defaultProps}
        localNames={[]}
        loading
        onCancel={onCancel}
        onRefresh={onRefresh}
        remoteEntries={[]}
      />
    );

    expect(screen.getByRole("status")).toHaveTextContent(/loading/iu);
    expect(screen.getAllByRole("button").every((button) => button.classList.contains("min-h-11")))
      .toBe(true);

    rerender(
      <MobileNotebookDialog
        {...defaultProps}
        error="Cloud notebooks are unavailable."
        loading={false}
        localNames={[]}
        onCancel={onCancel}
        onRefresh={onRefresh}
        remoteEntries={[]}
      />
    );
    expect(screen.getByText(/no notebooks on this device/iu)).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent("Cloud notebooks are unavailable.");
    fireEvent.click(screen.getByRole("button", { name: /try again|refresh/iu }));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledOnce());

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("contains Tab and Shift+Tab focus within the sheet", () => {
    render(
      <>
        <button type="button">Before sheet</button>
        <MobileNotebookDialog {...defaultProps} />
        <button type="button">After sheet</button>
      </>
    );

    const first = screen.getByRole("button", { name: "Cancel" });
    fireEvent.click(screen.getByRole("button", { name: "云端札记" }));
    const last = screen.getByRole("button", { name: "Restore" });

    first.focus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);
    expect(document.activeElement).not.toBe(screen.getByRole("button", { name: "Before sheet" }));

    last.focus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(document.activeElement).toBe(first);
    expect(document.activeElement).not.toBe(screen.getByRole("button", { name: "After sheet" }));
  });

  it("keeps Tab and Shift+Tab on the sheet container while every control is busy", async () => {
    const pending = deferred();
    render(
      <>
        <button type="button">Outside sheet</button>
        <MobileNotebookDialog
          {...defaultProps}
          onRestore={() => pending.promise}
        />
      </>
    );
    fireEvent.click(screen.getByRole("button", { name: "云端札记" }));
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    const dialog = screen.getByRole("dialog");
    const outside = screen.getByRole("button", { name: "Outside sheet" });

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
