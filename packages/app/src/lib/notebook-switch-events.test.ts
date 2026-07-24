import {
  emitNotebookSwitchRequested,
  listenNotebookSwitchRequested,
  requestPrimaryNotebookSwitch
} from "./notebook-switch-events";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests
} from "../runtime";

describe("notebook switch requests", () => {
  afterEach(() => resetAppRuntimeForTests());

  it("preserves an exact native path and rejects malformed payloads", async () => {
    const runtime = createDefaultAppRuntime();
    let listener: ((event: { payload: unknown }) => unknown) | null = null;
    configureAppRuntime({
      ...runtime,
      events: {
        emit: async (_name, payload) => {
          listener?.({ payload });
        },
        isAvailable: () => true,
        listen: async (_name, next) => {
          listener = next as (event: { payload: unknown }) => unknown;
          return () => {
            listener = null;
          };
        }
      }
    });
    const received = vi.fn();
    await listenNotebookSwitchRequested(received);

    await emitNotebookSwitchRequested({ path: "/Notes/Name ", source: "native-open" });
    (listener as ((event: { payload: unknown }) => unknown) | null)?.({
      payload: { path: 42, source: "native-open" }
    });

    expect(received).toHaveBeenCalledOnce();
    expect(received).toHaveBeenCalledWith({ path: "/Notes/Name ", source: "native-open" });
  });

  it("selects a folder locally and uses the durable desktop request when no main-window listener exists", async () => {
    const runtime = createDefaultAppRuntime();
    const emit = vi.fn(async () => undefined);
    const requestPrimaryNotebookSwitchRuntime = vi.fn(async () => undefined);
    configureAppRuntime({
      ...runtime,
      events: { ...runtime.events, emit },
      files: {
        ...runtime.files,
        openMarkdownFolder: async () => ({ name: "Notes ", path: "/Notes/Name " }),
        requestPrimaryNotebookSwitch: requestPrimaryNotebookSwitchRuntime
      }
    });

    await expect(requestPrimaryNotebookSwitch({ source: "file-menu" })).resolves.toBe(true);

    expect(requestPrimaryNotebookSwitchRuntime).toHaveBeenCalledWith("/Notes/Name ");
    expect(emit).not.toHaveBeenCalled();
  });
});
