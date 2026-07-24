import {
  listenPrimaryWorkspaceChanged,
  notifyPrimaryWorkspaceChanged
} from "./primary-workspace-events";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests
} from "../../runtime";

describe("primary workspace application events", () => {
  const emit = vi.fn();
  const listen = vi.fn();

  beforeEach(() => {
    emit.mockReset();
    listen.mockReset();
    configureAppRuntime({
      ...createDefaultAppRuntime(),
      events: {
        emit,
        isAvailable: () => true,
        listen
      }
    });
  });

  afterEach(() => resetAppRuntimeForTests());

  it("ignores self notifications and forwards another controller generation", async () => {
    const cleanup = vi.fn();
    const changed = vi.fn();
    listen.mockResolvedValue(cleanup);

    const stop = await listenPrimaryWorkspaceChanged("settings-instance", changed);
    const listener = listen.mock.calls[0]?.[1];

    await notifyPrimaryWorkspaceChanged({ generation: 3, sourceId: "settings-instance" });
    listener?.({ payload: { generation: 3, sourceId: "settings-instance" } });
    listener?.({ payload: { generation: 7, sourceId: "main-instance" } });
    listener?.({ payload: { generation: -1, sourceId: "other" } });
    stop();

    expect(emit).toHaveBeenCalledWith("qingyu://primary-workspace-changed", {
      generation: 3,
      sourceId: "settings-instance"
    });
    expect(changed).toHaveBeenCalledOnce();
    expect(changed).toHaveBeenCalledWith({ generation: 7, sourceId: "main-instance" });
    expect(cleanup).toHaveBeenCalledOnce();
  });
});
