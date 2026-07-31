import { act, startTransition, Suspense } from "react";
import { createRoot, type Root } from "react-dom/client";

import {
  createDesktopStartupWorkspaceController,
  DesktopStartupWorkspace
} from "./desktop-startup-workspace";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true;

type AuthoritativeStartupStatus =
  | "unselected"
  | "invalid"
  | "unavailable"
  | "resolving"
  | "unsupported-version"
  | "starting"
  | "ready"
  | "failed";

type WorkspaceProps = {
  retryWorkspace: () => Promise<unknown>;
  selectWorkspace: () => Promise<string | null>;
  startWorkspace: (workspacePath: string) => Promise<unknown>;
  startupStatus: AuthoritativeStartupStatus;
};

type WorkspaceInput = Pick<WorkspaceProps, "selectWorkspace" | "startWorkspace"> &
  Partial<Pick<WorkspaceProps, "retryWorkspace" | "startupStatus">>;

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

  it("uses only the dedicated retry callback for an authoritative failed startup", () => {
    const retryRequest = deferred<unknown>();
    const retryWorkspace = vi.fn(() => retryRequest.promise);
    const selectWorkspace = vi.fn(async () => null);
    const startWorkspace = vi.fn(async () => undefined);
    const container = renderWorkspace({
      retryWorkspace,
      selectWorkspace,
      startWorkspace,
      startupStatus: "failed"
    });

    const retry = buttonNamed(container, "Retry");
    act(() => {
      retry.click();
      retry.click();
    });

    expect(retryWorkspace).toHaveBeenCalledTimes(1);
    expect(selectWorkspace).not.toHaveBeenCalled();
    expect(startWorkspace).not.toHaveBeenCalled();
    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "retrying"
    );
    expect(container.querySelectorAll("button")).toHaveLength(0);
  });

  it("returns to a safe failed shell when the dedicated retry rejects", async () => {
    const container = renderWorkspace({
      retryWorkspace: vi.fn(async () => {
        throw new Error("sensitive retry detail");
      }),
      selectWorkspace: vi.fn(async () => null),
      startWorkspace: vi.fn(async () => undefined),
      startupStatus: "failed"
    });

    await act(async () => buttonNamed(container, "Retry").click());

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "failed"
    );
    expect(container).not.toHaveTextContent("sensitive retry detail");
    expect(buttonNamed(container, "Retry")).toBeEnabled();
    expect(container.querySelectorAll("button")).toHaveLength(1);
  });

  it("follows an authoritative starting to failed transition after child startup", async () => {
    const retryWorkspace = vi.fn(async () => undefined);
    const selectWorkspace = vi.fn(async () => "/Users/example/Notes");
    const startWorkspace = vi.fn(async () => undefined);
    const mounted = mountWorkspace({
      retryWorkspace,
      selectWorkspace,
      startWorkspace,
      startupStatus: "unselected"
    });

    await act(async () => buttonNamed(mounted.container, "Choose directory").click());
    expect(mounted.container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "starting"
    );
    mounted.render({
      retryWorkspace,
      selectWorkspace,
      startWorkspace,
      startupStatus: "starting"
    });
    mounted.render({
      retryWorkspace,
      selectWorkspace,
      startWorkspace,
      startupStatus: "failed"
    });

    expect(mounted.container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "failed"
    );
    expect(buttonNamed(mounted.container, "Retry")).toBeEnabled();
    expect(mounted.container.querySelectorAll("button")).toHaveLength(1);
    expect(selectWorkspace).toHaveBeenCalledTimes(1);
    expect(startWorkspace).toHaveBeenCalledWith("/Users/example/Notes");
  });

  it("allows workspace selection for an authoritative invalid startup", () => {
    const container = renderWorkspace({
      selectWorkspace: vi.fn(async () => null),
      startWorkspace: vi.fn(async () => undefined),
      startupStatus: "invalid"
    });

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "unselected"
    );
    expect(buttonNamed(container, "Choose directory")).toBeEnabled();
    expect(container.querySelectorAll("button")).toHaveLength(1);
  });

  it("keeps an authoritative unavailable startup retry-only", () => {
    const container = renderWorkspace({
      selectWorkspace: vi.fn(async () => null),
      startWorkspace: vi.fn(async () => undefined),
      startupStatus: "unavailable"
    });

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "failed"
    );
    expect(buttonNamed(container, "Retry")).toBeEnabled();
    expect(container.querySelectorAll("button")).toHaveLength(1);
  });

  it("keeps an authoritative resolving startup busy without actions", () => {
    const retryWorkspace = vi.fn(async () => undefined);
    const selectWorkspace = vi.fn(async () => null);
    const startWorkspace = vi.fn(async () => undefined);
    const container = renderWorkspace({
      retryWorkspace,
      selectWorkspace,
      startWorkspace,
      startupStatus: "resolving"
    });

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "resolving"
    );
    expect(container.querySelector("[role='status']")).toHaveTextContent(
      "Resolving your workspace…"
    );
    expect(container.querySelector("button")).toBeNull();
    expect(retryWorkspace).not.toHaveBeenCalled();
    expect(selectWorkspace).not.toHaveBeenCalled();
    expect(startWorkspace).not.toHaveBeenCalled();
  });

  it("maps an unsupported Kernel version to an actionless upgrade requirement", () => {
    const retryWorkspace = vi.fn(async () => undefined);
    const selectWorkspace = vi.fn(async () => null);
    const startWorkspace = vi.fn(async () => undefined);
    const container = renderWorkspace({
      retryWorkspace,
      selectWorkspace,
      startWorkspace,
      startupStatus: "unsupported-version"
    });

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "upgrade-required"
    );
    expect(container.querySelector("h1")).toHaveTextContent("Update QingYu to continue");
    expect(container.querySelector("button")).toBeNull();
    expect(container.querySelector("[role='status']")).toBeNull();
    expect(retryWorkspace).not.toHaveBeenCalled();
    expect(selectWorkspace).not.toHaveBeenCalled();
    expect(startWorkspace).not.toHaveBeenCalled();
  });

  it("rejects direct selection while an authoritative failure requires retry", async () => {
    const selectWorkspace = vi.fn(async () => null);
    const controller = createDesktopStartupWorkspaceController({
      retryWorkspace: vi.fn(async () => undefined),
      selectWorkspace,
      startWorkspace: vi.fn(async () => undefined),
      startupStatus: "failed"
    });

    await controller.select();

    expect(selectWorkspace).not.toHaveBeenCalled();
    expect(controller.getSnapshot()).toEqual({
      failure: "startup",
      status: "failed",
      workspacePath: null
    });
  });

  it("keeps ready passive while the parent owns shell replacement", () => {
    const retryWorkspace = vi.fn(async () => undefined);
    const selectWorkspace = vi.fn(async () => null);
    const startWorkspace = vi.fn(async () => undefined);
    const container = renderWorkspace({
      retryWorkspace,
      selectWorkspace,
      startWorkspace,
      startupStatus: "ready"
    });

    expect(container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "starting"
    );
    expect(container.querySelector("button")).toBeNull();
    expect(retryWorkspace).not.toHaveBeenCalled();
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

  it.each(["unselected", "invalid"] as const)(
    "allows selection again when startup rejects before %s is persisted",
    async (startupStatus) => {
      const container = renderWorkspace({
        selectWorkspace: vi.fn(async () => "/Users/example/Notes"),
        startWorkspace: vi.fn(async () => {
          throw new Error("sensitive native startup detail");
        }),
        startupStatus
      });

      await act(async () => buttonNamed(container, "Choose directory").click());

      expect(container.querySelector("main")).toHaveAttribute(
        "data-desktop-startup-workspace",
        "failed"
      );
      expect(container.querySelector("[role='alert']")).toHaveTextContent(
        "The directory selector could not open."
      );
      expect(container).not.toHaveTextContent("sensitive native startup detail");
      expect(buttonNamed(container, "Choose directory")).toBeEnabled();
      expect(container.querySelectorAll("button")).toHaveLength(1);
    }
  );

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

  it("keeps selection state and uses current callbacks after a committed callback-only rerender", async () => {
    const selection = deferred<string | null>();
    const initialStartup = vi.fn(async () => undefined);
    const currentSelection = vi.fn(async () => null);
    const currentStartup = vi.fn(async () => undefined);
    const mounted = mountWorkspace({
      selectWorkspace: vi.fn(() => selection.promise),
      startWorkspace: initialStartup
    });

    act(() => buttonNamed(mounted.container, "Choose directory").click());
    mounted.render({
      selectWorkspace: currentSelection,
      startWorkspace: currentStartup
    });
    expect(mounted.container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "selecting"
    );
    await act(async () => selection.resolve("/Users/example/Notes"));

    expect(initialStartup).not.toHaveBeenCalled();
    expect(currentSelection).not.toHaveBeenCalled();
    expect(currentStartup).toHaveBeenCalledWith("/Users/example/Notes");
    expect(mounted.container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "starting"
    );
  });

  it("does not leak callbacks from an abandoned concurrent render", async () => {
    const selection = deferred<string | null>();
    const committedStartup = vi.fn(async () => undefined);
    const abandonedStartup = vi.fn(async () => undefined);
    const neverCommitted = new Promise<never>(() => undefined);
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    mountedRoots.push({ container, root });

    act(() => root.render(
      <Suspense fallback={<p>Waiting</p>}>
        <DesktopStartupWorkspace
          retryWorkspace={vi.fn(async () => undefined)}
          selectWorkspace={vi.fn(() => selection.promise)}
          startWorkspace={committedStartup}
          startupStatus="unselected"
        />
      </Suspense>
    ));
    act(() => buttonNamed(container, "Choose directory").click());
    act(() => {
      startTransition(() => root.render(
        <Suspense fallback={<p>Waiting</p>}>
          <DesktopStartupWorkspace
            retryWorkspace={vi.fn(async () => undefined)}
            selectWorkspace={vi.fn(async () => null)}
            startWorkspace={abandonedStartup}
            startupStatus="unselected"
          />
          <SuspendForever pending={neverCommitted} />
        </Suspense>
      ));
    });

    await act(async () => selection.resolve("/Users/example/Notes"));

    expect(committedStartup).toHaveBeenCalledWith("/Users/example/Notes");
    expect(abandonedStartup).not.toHaveBeenCalled();
  });

  it.each([
    { shellStatus: "resolving", startupStatus: "resolving" },
    { shellStatus: "starting", startupStatus: "starting" },
    { shellStatus: "starting", startupStatus: "ready" },
    { shellStatus: "upgrade-required", startupStatus: "unsupported-version" }
  ] as const)(
    "fails closed when a committed startup becomes $startupStatus",
    async ({ shellStatus, startupStatus }) => {
      const startup = deferred<unknown>();
      const retryWorkspace = vi.fn(async () => undefined);
      const selectWorkspace = vi.fn(async () => "/Users/example/Notes");
      const startWorkspace = vi.fn(() => startup.promise);
      const mounted = mountWorkspace({
        retryWorkspace,
        selectWorkspace,
        startWorkspace,
        startupStatus: "unselected"
      });

      await act(async () => buttonNamed(mounted.container, "Choose directory").click());
      expect(startWorkspace).toHaveBeenCalledWith("/Users/example/Notes");
      mounted.render({
        retryWorkspace,
        selectWorkspace,
        startWorkspace,
        startupStatus
      });
      await act(async () => {
        startup.reject(new Error("sensitive stale detail"));
        await Promise.resolve();
      });

      expect(mounted.container.querySelector("main")).toHaveAttribute(
        "data-desktop-startup-workspace",
        shellStatus
      );
      expect(mounted.container.querySelector("button")).toBeNull();
      expect(mounted.container).not.toHaveTextContent("sensitive stale detail");
      expect(retryWorkspace).not.toHaveBeenCalled();
    }
  );

  it("ignores a selection result superseded by an authoritative status", async () => {
    const selection = deferred<string | null>();
    const retryWorkspace = vi.fn(async () => undefined);
    const selectWorkspace = vi.fn(() => selection.promise);
    const startWorkspace = vi.fn(async () => undefined);
    const mounted = mountWorkspace({
      retryWorkspace,
      selectWorkspace,
      startWorkspace,
      startupStatus: "unselected"
    });

    act(() => buttonNamed(mounted.container, "Choose directory").click());
    mounted.render({
      retryWorkspace,
      selectWorkspace,
      startWorkspace,
      startupStatus: "failed"
    });
    await act(async () => selection.resolve("/Users/example/Stale"));

    expect(startWorkspace).not.toHaveBeenCalled();
    expect(mounted.container.querySelector("main")).toHaveAttribute(
      "data-desktop-startup-workspace",
      "failed"
    );
    expect(buttonNamed(mounted.container, "Retry")).toBeEnabled();
  });

  it("ignores a selection result after the startup shell unmounts", async () => {
    const selection = deferred<string | null>();
    const startWorkspace = vi.fn(async () => undefined);
    const mounted = mountWorkspace({
      selectWorkspace: vi.fn(() => selection.promise),
      startWorkspace
    });

    act(() => buttonNamed(mounted.container, "Choose directory").click());
    mounted.unmount();
    selection.resolve("/Users/example/Stale");
    await Promise.resolve();

    expect(startWorkspace).not.toHaveBeenCalled();
  });

  it("does not restart selection after the host accepts the startup request", async () => {
    const selectWorkspace = vi.fn(async () => "/Users/example/Notes");
    const startWorkspace = vi.fn(async () => undefined);
    const controller = createDesktopStartupWorkspaceController({
      retryWorkspace: vi.fn(async () => undefined),
      selectWorkspace,
      startWorkspace,
      startupStatus: "unselected"
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

  it("keeps an in-flight selection alive across subscription reconnects", async () => {
    const selection = deferred<string | null>();
    const startWorkspace = vi.fn(async () => undefined);
    const controller = createDesktopStartupWorkspaceController({
      retryWorkspace: vi.fn(async () => undefined),
      selectWorkspace: vi.fn(() => selection.promise),
      startWorkspace,
      startupStatus: "unselected"
    });
    const disconnect = controller.subscribe(vi.fn());
    const request = controller.select();

    disconnect();
    const disconnectAgain = controller.subscribe(vi.fn());
    selection.resolve("/Users/example/Notes");
    await request;

    expect(startWorkspace).toHaveBeenCalledWith("/Users/example/Notes");
    expect(controller.getSnapshot()).toEqual({
      status: "starting",
      workspacePath: "/Users/example/Notes"
    });
    disconnectAgain();
  });

  function renderWorkspace(props: WorkspaceInput) {
    return mountWorkspace(props).container;
  }

  function mountWorkspace(input: WorkspaceInput) {
    const defaults = {
      retryWorkspace: vi.fn(async () => undefined),
      startupStatus: "unselected" as const
    };
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    mountedRoots.push({ container, root });
    const render = (nextInput: WorkspaceInput) => act(() => root.render(
      <DesktopStartupWorkspace {...defaults} {...nextInput} />
    ));
    const unmount = () => {
      const mountedIndex = mountedRoots.findIndex((mounted) => mounted.root === root);
      if (mountedIndex >= 0) mountedRoots.splice(mountedIndex, 1);
      act(() => root.unmount());
      container.remove();
    };
    render(input);
    return { container, render, unmount };
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
  let reject!: (reason?: unknown) => unknown;
  let resolve!: (value: T) => unknown;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    reject = promiseReject;
    resolve = promiseResolve;
  });
  return { promise, reject, resolve };
}

function SuspendForever({ pending }: { pending: Promise<never> }): never {
  throw pending;
}
