import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";
import { showNativeAppAbout, showNativePandocSetup } from "./dialog";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  message: vi.fn()
}));

const mockedInvoke = vi.mocked(invoke);
const mockedMessage = vi.mocked(message);

describe("native dialogs", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
    mockedMessage.mockReset();
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
