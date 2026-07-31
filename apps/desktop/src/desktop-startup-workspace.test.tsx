import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

import {
  createDesktopStartupWorkspaceController,
  DesktopStartupWorkspace
} from "./desktop-startup-workspace";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true;

describe("desktop startup workspace", () => {
  const mountedRoots: Array<{ container: HTMLDivElement; root: Root }> = [];

  afterEach(() => {
    for (const mounted of mountedRoots.splice(0)) {
      act(() => mounted.root.unmount());
      mounted.container.remove();
    }
  });

  it("renders the local workspace shell before a directory is selected", () => {
    const selectWorkspace = vi.fn(async () => null);
    const startWorkspace = vi.fn(async () => undefined);

    const container = renderWorkspace({ selectWorkspace, startWorkspace });

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "unselected"
    );
    expect(container.querySelector("h1")).toHaveTextContent("Choose your workspace");
    expect(buttonNamed(container, "Choose directory")).toBeEnabled();
    expect(selectWorkspace).not.toHaveBeenCalled();
    expect(startWorkspace).not.toHaveBeenCalled();
  });

  it("starts only one directory selection while the selector is pending", () => {
    const selection = deferred<string | null>();
    const selectWorkspace = vi.fn(() => selection.promise);
    const container = renderWorkspace({
      selectWorkspace,
      startWorkspace: vi.fn(async () => undefined)
    });
    const chooseDirectory = buttonNamed(container, "Choose directory");

    act(() => {
      chooseDirectory.click();
      chooseDirectory.click();
    });

    expect(selectWorkspace).toHaveBeenCalledTimes(1);
    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "selecting"
    );
    expect(buttonNamed(container, "Choosing directory…")).toBeDisabled();
  });

  it("returns to the unselected shell when directory selection is canceled", async () => {
    const selection = deferred<string | null>();
    const startWorkspace = vi.fn(async () => undefined);
    const container = renderWorkspace({
      selectWorkspace: vi.fn(() => selection.promise),
      startWorkspace
    });

    act(() => buttonNamed(container, "Choose directory").click());
    await act(async () => selection.resolve(null));

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "unselected"
    );
    expect(buttonNamed(container, "Choose directory")).toBeEnabled();
    expect(startWorkspace).not.toHaveBeenCalled();
  });

  it("requests host startup for the selected path and keeps the shell in starting", async () => {
    const startup = deferred<unknown>();
    const startWorkspace = vi.fn(() => startup.promise);
    const container = renderWorkspace({
      selectWorkspace: vi.fn(async () => "/Users/example/Notes"),
      startWorkspace
    });

    await act(async () => buttonNamed(container, "Choose directory").click());

    expect(startWorkspace).toHaveBeenCalledTimes(1);
    expect(startWorkspace).toHaveBeenCalledWith("/Users/example/Notes");
    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "starting"
    );
    expect(container.querySelector("[role='status']")).toHaveTextContent(
      "Starting the native Kernel for your workspace…"
    );
    expect(container.querySelector("button")).toBeNull();
  });

  it("shows a safe failed shell when the host rejects workspace startup", async () => {
    const container = renderWorkspace({
      selectWorkspace: vi.fn(async () => "/Users/example/Notes"),
      startWorkspace: vi.fn(async () => {
        throw new Error("sensitive native startup detail");
      })
    });

    await act(async () => buttonNamed(container, "Choose directory").click());

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "failed"
    );
    expect(container.querySelector("[role='alert']")).toHaveTextContent(
      "The native Kernel could not start for this workspace."
    );
    expect(container).not.toHaveTextContent("sensitive native startup detail");
    expect(buttonNamed(container, "Retry")).toBeEnabled();
    expect(buttonNamed(container, "Choose another directory")).toBeEnabled();
  });

  it("offers selection again without a dead retry after the selector rejects", async () => {
    const container = renderWorkspace({
      selectWorkspace: vi.fn(async () => {
        throw new Error("sensitive selector detail");
      }),
      startWorkspace: vi.fn(async () => undefined)
    });

    await act(async () => buttonNamed(container, "Choose directory").click());

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "failed"
    );
    expect(container).not.toHaveTextContent("sensitive selector detail");
    expect(container.querySelectorAll("button")).toHaveLength(1);
    expect(buttonNamed(container, "Choose directory")).toBeEnabled();
  });

  it("retries the retained workspace once while a retry is pending", async () => {
    const retryStartup = deferred<unknown>();
    const startWorkspace = vi.fn()
      .mockRejectedValueOnce(new Error("first startup failed"))
      .mockImplementationOnce(() => retryStartup.promise);
    const selectWorkspace = vi.fn(async () => "/Users/example/Notes");
    const container = renderWorkspace({ selectWorkspace, startWorkspace });

    await act(async () => buttonNamed(container, "Choose directory").click());
    const retry = buttonNamed(container, "Retry");
    act(() => {
      retry.click();
      retry.click();
    });

    expect(selectWorkspace).toHaveBeenCalledTimes(1);
    expect(startWorkspace).toHaveBeenCalledTimes(2);
    expect(startWorkspace).toHaveBeenLastCalledWith("/Users/example/Notes");
    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "retrying"
    );
    expect(container.querySelector("[role='status']")).toHaveTextContent(
      "Retrying native Kernel startup…"
    );
  });

  it("ignores a stale selection result after the shell receives new callbacks", async () => {
    const staleSelection = deferred<string | null>();
    const staleStartup = vi.fn(async () => undefined);
    const mounted = mountWorkspace({
      selectWorkspace: vi.fn(() => staleSelection.promise),
      startWorkspace: staleStartup
    });

    act(() => buttonNamed(mounted.container, "Choose directory").click());
    mounted.render({
      selectWorkspace: vi.fn(async () => null),
      startWorkspace: vi.fn(async () => undefined)
    });
    act(() => buttonNamed(mounted.container, "Choose directory").click());
    await act(async () => staleSelection.resolve("/Users/example/Stale"));

    expect(staleStartup).not.toHaveBeenCalled();
    expect(mounted.container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "unselected"
    );
  });

  it("does not restart selection after the host accepts the startup request", async () => {
    const selectWorkspace = vi.fn(async () => "/Users/example/Notes");
    const startWorkspace = vi.fn(async () => undefined);
    const controller = createDesktopStartupWorkspaceController({
      selectWorkspace,
      startWorkspace
    });

    await controller.select();
    await controller.select();

    expect(controller.getSnapshot()).toEqual({
      status: "starting",
      workspacePath: "/Users/example/Notes"
    });
    expect(selectWorkspace).toHaveBeenCalledTimes(1);
    expect(startWorkspace).toHaveBeenCalledTimes(1);
  });

  function renderWorkspace({
    selectWorkspace,
    startWorkspace
  }: {
    selectWorkspace: () => Promise<string | null>;
    startWorkspace: (workspacePath: string) => Promise<unknown>;
  }) {
    return mountWorkspace({ selectWorkspace, startWorkspace }).container;
  }

  function mountWorkspace(props: {
    selectWorkspace: () => Promise<string | null>;
    startWorkspace: (workspacePath: string) => Promise<unknown>;
  }) {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    mountedRoots.push({ container, root });
    const render = (nextProps: typeof props) => act(() => root.render(
      <DesktopStartupWorkspace {...nextProps} />
    ));
    render(props);
    return { container, render };
  }
});

function buttonNamed(container: HTMLElement, name: string) {
  const button = [...container.querySelectorAll("button")].find(
    (candidate) => candidate.textContent?.trim() === name
  );
  if (!button) throw new Error(`Button not found: ${name}`);
  return button;
}

function deferred<T>() {
  let resolve!: (value: T) => unknown;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}
