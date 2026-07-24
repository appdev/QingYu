import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { desktopRuntime } from "../desktop";
import { mobileRuntime } from "../mobile";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
  listen: vi.fn()
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn()
}));

vi.mock("@tauri-apps/plugin-fs", () => ({
  readFile: vi.fn()
}));

vi.mock("@tauri-apps/plugin-log", () => ({
  error: vi.fn(),
  info: vi.fn(),
  warn: vi.fn()
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn()
}));

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: vi.fn(() => "macos"),
  version: vi.fn(() => "26.5.1")
}));

vi.mock("@tauri-apps/plugin-store", () => ({
  load: vi.fn()
}));

const mockedInvoke = vi.mocked(invoke);
const mockedOpenUrl = vi.mocked(openUrl);

describe("official external URL opener", () => {
  beforeEach(() => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {}
    });
    mockedInvoke.mockReset();
    mockedOpenUrl.mockReset();
    mockedOpenUrl.mockResolvedValue(undefined);
  });

  afterEach(() => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    vi.restoreAllMocks();
  });

  it("keeps the validated browser fallback for non-Tauri development", async () => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    const browserOpen = vi.spyOn(window, "open").mockImplementation(() => null);

    await desktopRuntime.window.openExternalUrl(" https://example.test/browser ");

    expect(browserOpen).toHaveBeenCalledWith(
      "https://example.test/browser",
      "_blank",
      "noopener,noreferrer"
    );
    expect(mockedOpenUrl).not.toHaveBeenCalled();
  });

  it("does not expose browser fallback failure details", async () => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    vi.spyOn(window, "open").mockImplementation(() => {
      throw new Error(
        "Browser failed for https://example.test/open?access_token=private-token at /Users/example/private.md credential=private-password Authorization Bearer private-header"
      );
    });

    await expect(desktopRuntime.window.openExternalUrl("https://example.test/browser"))
      .rejects.toThrow("The system could not open the link.");

    expect(mockedOpenUrl).not.toHaveBeenCalled();
  });

  it.each([
    ["desktop", desktopRuntime],
    ["mobile", mobileRuntime]
  ] as const)("opens parsed HTTP and HTTPS URLs through the %s runtime", async (_name, runtime) => {
    await runtime.window.openExternalUrl("  https://example.test/guide?q=mobile#start  ");
    await runtime.window.openExternalUrl("http://example.test/help");

    expect(mockedOpenUrl.mock.calls).toEqual([
      ["https://example.test/guide?q=mobile#start"],
      ["http://example.test/help"]
    ]);
    expect(mockedInvoke).not.toHaveBeenCalledWith("open_external_url", expect.anything());
  });

  it.each([
    "file:///Users/example/private.md",
    "javascript:alert(1)",
    "mailto:hello@example.test",
    "tel:+15551234567",
    "qingyu://settings",
    "https://user:password@example.test/private"
  ])("rejects unsupported or credential-bearing URL %s before calling the plugin", async (url) => {
    await expect(desktopRuntime.window.openExternalUrl(url)).rejects.toThrow(/unsupported|invalid|credentials/iu);

    expect(mockedOpenUrl).not.toHaveBeenCalled();
  });

  it("maps unknown native opener details to a fixed safe failure", async () => {
    const unsafeFailure = new Error(
      "Browser failed for https://example.test/open?access_token=private-token at /Users/example/private.md credential=private-password Authorization Bearer private-header"
    );
    mockedOpenUrl.mockRejectedValue(unsafeFailure);
    mockedInvoke.mockRejectedValue(unsafeFailure);

    const failure = await desktopRuntime.window.openExternalUrl("https://example.test/open")
      .then(() => null, (error: unknown) => error);
    const message = failure instanceof Error ? failure.message : String(failure);

    expect(message).toBe("The system could not open the link.");
    expect(message).not.toContain("https://example.test");
    expect(message).not.toContain("private-token");
    expect(message).not.toContain("/Users/example/private.md");
    expect(message).not.toContain("private-password");
    expect(message).not.toContain("Authorization");
    expect(message).not.toContain("private-header");
  });
});
