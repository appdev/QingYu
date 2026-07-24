import { invoke } from "@tauri-apps/api/core";
import { listen, type Event } from "@tauri-apps/api/event";
import { subscribeToMobileSystemBack } from "./mobile-back";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn()
}));

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

describe("mobile system Back adapter", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
    mockedListen.mockReset();
    mockedInvoke.mockResolvedValue(undefined);
  });

  it.each([
    [true, true],
    [false, false]
  ])("acknowledges a completed handler result %s", async (handlerResult, consumed) => {
    let mobileBack: ((event: Event<unknown>) => unknown) | undefined;
    const cleanup = vi.fn();
    mockedListen.mockImplementation(async (event, handler) => {
      expect(event).toBe("qingyu://mobile-back-requested");
      mobileBack = handler as (event: Event<unknown>) => unknown;
      return cleanup;
    });
    const handler = vi.fn(async () => handlerResult);

    const unsubscribe = await subscribeToMobileSystemBack(handler);
    await mobileBack?.({ event: "qingyu://mobile-back-requested", id: 1, payload: null } as Event<unknown>);

    expect(handler).toHaveBeenCalledTimes(1);
    expect(mockedInvoke).toHaveBeenCalledWith("complete_mobile_back", { consumed });

    await unsubscribe();
    expect(cleanup).toHaveBeenCalledTimes(1);
  });

  it("fails closed when navigation rejects so Android cannot exit accidentally", async () => {
    const navigationError = new Error("sync form could not flush");
    let mobileBack: ((event: Event<unknown>) => unknown) | undefined;
    mockedListen.mockImplementation(async (_event, handler) => {
      mobileBack = handler as (event: Event<unknown>) => unknown;
      return () => undefined;
    });

    await subscribeToMobileSystemBack(async () => {
      throw navigationError;
    });
    await expect(mobileBack?.({
      event: "qingyu://mobile-back-requested",
      id: 2,
      payload: null
    } as Event<unknown>)).resolves.toBeUndefined();

    expect(mockedInvoke).toHaveBeenCalledWith("complete_mobile_back", { consumed: true });
  });
});
