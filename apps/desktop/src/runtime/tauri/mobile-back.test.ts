import { invoke } from "@tauri-apps/api/core";
import { subscribeToMobileSystemBack } from "./mobile-back";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

const mockedInvoke = vi.mocked(invoke);
const mobileBackRequestedEvent = "qingyu://mobile-back-requested";

function dispatchMobileBack() {
  window.dispatchEvent(new Event(mobileBackRequestedEvent));
}

describe("mobile system Back adapter", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
    mockedInvoke.mockImplementation(async (command) => (
      command === "begin_mobile_back" ? true : undefined
    ));
  });

  it.each([
    [true, true],
    [false, false]
  ])("acquires native authority before acknowledging handler result %s", async (handlerResult, consumed) => {
    const handler = vi.fn(async () => handlerResult);

    const unsubscribe = await subscribeToMobileSystemBack(handler);
    dispatchMobileBack();

    await vi.waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith(
      "complete_mobile_back",
      { consumed }
    ));
    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "begin_mobile_back");
    expect(handler).toHaveBeenCalledTimes(1);
    expect(mockedInvoke.mock.invocationCallOrder[0]).toBeLessThan(handler.mock.invocationCallOrder[0]);

    await unsubscribe();
    dispatchMobileBack();
    await Promise.resolve();
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it("fails closed when navigation rejects so Android cannot exit accidentally", async () => {
    const navigationError = new Error("sync form could not flush");
    const unsubscribe = await subscribeToMobileSystemBack(async () => {
      throw navigationError;
    });
    dispatchMobileBack();

    await vi.waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith(
      "complete_mobile_back",
      { consumed: true }
    ));
    await unsubscribe();
  });

  it("ignores forged or duplicate Back delivery without native authority", async () => {
    let beginCalls = 0;
    mockedInvoke.mockImplementation(async (command) => {
      if (command !== "begin_mobile_back") return undefined;
      beginCalls += 1;
      return beginCalls === 1;
    });
    let releaseHandler!: (value: boolean) => unknown;
    const handler = vi.fn(() => new Promise<boolean>((resolve) => {
      releaseHandler = resolve;
    }));

    const unsubscribe = await subscribeToMobileSystemBack(handler);
    dispatchMobileBack();
    dispatchMobileBack();

    await vi.waitFor(() => expect(mockedInvoke).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(handler).toHaveBeenCalledTimes(1));
    releaseHandler(true);
    await vi.waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith(
      "complete_mobile_back",
      { consumed: true }
    ));
    await unsubscribe();
  });
});
