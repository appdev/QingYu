import { invoke } from "@tauri-apps/api/core";
import { confirm, message } from "@tauri-apps/plugin-dialog";
import { confirmNativeAction, showNativeAppAbout, showNativePandocSetup } from "./dialog";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  message: vi.fn()
}));

const mockedConfirm = vi.mocked(confirm);
const mockedInvoke = vi.mocked(invoke);
const mockedMessage = vi.mocked(message);

describe("native dialogs", () => {
  beforeEach(() => {
    mockedConfirm.mockReset();
    mockedInvoke.mockReset();
    mockedMessage.mockReset();
  });

  it("uses the explicit native warning confirmation", async () => {
    mockedConfirm.mockResolvedValueOnce(true);

    await expect(confirmNativeAction("Change the global key?")).resolves.toBe(true);

    expect(mockedConfirm).toHaveBeenCalledWith("Change the global key?", {
      kind: "warning",
      title: "QingYu"
    });
  });

  it("maps the native Pandoc setup dialog buttons to app actions", async () => {
    mockedMessage.mockResolvedValueOnce("Install Pandoc").mockResolvedValueOnce("Set Pandoc path").mockResolvedValueOnce("Cancel");

    const labels = {
      cancelLabel: "Cancel",
      installLabel: "Install Pandoc",
      message: "Install Pandoc to continue exporting.",
      setPathLabel: "Set Pandoc path",
      title: "Pandoc required"
    };

    await expect(showNativePandocSetup(labels)).resolves.toBe("install");
    await expect(showNativePandocSetup(labels)).resolves.toBe("setPath");
    await expect(showNativePandocSetup(labels)).resolves.toBe("cancel");

    expect(mockedMessage).toHaveBeenCalledWith("Install Pandoc to continue exporting.", {
      buttons: {
        cancel: "Cancel",
        no: "Set Pandoc path",
        yes: "Install Pandoc"
      },
      kind: "warning",
      title: "Pandoc required"
    });
  });

  it("opens the system-native app about panel through Rust", async () => {
    await showNativeAppAbout();

    expect(mockedInvoke).toHaveBeenCalledWith("show_native_app_about");
  });
});
