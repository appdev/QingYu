import { FakeIndexedDbFactory } from "../../test/web-runtime-fakes";
import { createWebRuntime } from "..";

describe("web dialog runtime", () => {
  it("provides retained browser dialog fallbacks", async () => {
    const runtime = createWebRuntime({
      indexedDB: new FakeIndexedDbFactory().indexedDB
    });

    await expect(runtime.dialog.showPandocSetup({
      cancelLabel: "Cancel",
      installLabel: "Install",
      message: "Pandoc is unavailable.",
      setPathLabel: "Set path",
      title: "Pandoc"
    })).resolves.toBe("cancel");
    await expect(runtime.dialog.showAppAbout()).resolves.toBeUndefined();
  });

  it("confirms browser actions through the runtime adapter", async () => {
    const runtime = createWebRuntime({
      indexedDB: new FakeIndexedDbFactory().indexedDB
    });
    const confirm = vi.spyOn(globalThis, "confirm").mockReturnValue(true);

    await expect(runtime.dialog.confirm("Change the global key?")).resolves.toBe(true);

    expect(confirm).toHaveBeenCalledWith("Change the global key?");
  });
});
