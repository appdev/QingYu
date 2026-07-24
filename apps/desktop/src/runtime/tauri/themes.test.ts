import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  confirmNativeThemeActivation,
  commitNativeThemeActivation,
  cancelNativeThemeActivation,
  deleteNativeTheme,
  importNativeTheme,
  listNativeThemes,
  openNativeThemeDirectory,
  prepareNativeThemeActivation,
  releaseNativeThemeActivation,
  replaceNativeTheme
} from "./themes";

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: vi.fn(), invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(), open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openPath: vi.fn() }));

const mockedInvoke = vi.mocked(invoke);
const mockedConvertFileSrc = vi.mocked(convertFileSrc);
const mockedOpen = vi.mocked(open);
const mockedAsk = vi.mocked(ask);
const mockedOpenPath = vi.mocked(openPath);

describe("native theme runtime", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedConvertFileSrc.mockImplementation((path) => `asset://${path}`);
  });

  it("maps catalog reads, activation transitions, and deletes to semantic native commands", async () => {
    mockedInvoke.mockImplementation(async (command) => command === "prepare_theme_activation"
      ? {
          fingerprint: "fingerprint",
          id: "nord",
          source: { kind: "inline", css: ":root {}" },
          token: "prepare-token"
        }
      : undefined);

    await listNativeThemes();
    await prepareNativeThemeActivation("nord", "fingerprint");
    await commitNativeThemeActivation("commit-token");
    await cancelNativeThemeActivation("cancel-token");
    await releaseNativeThemeActivation();
    await deleteNativeTheme("nord", "fingerprint");

    expect(mockedInvoke.mock.calls.map(([command, args]) => [command, args])).toEqual([
      ["list_themes", undefined],
      ["prepare_theme_activation", { id: "nord", expectedFingerprint: "fingerprint" }],
      ["commit_theme_activation", { token: "commit-token" }],
      ["cancel_theme_activation", { token: "cancel-token" }],
      ["release_theme_activation", undefined],
      ["delete_theme", { id: "nord", expectedFingerprint: "fingerprint" }]
    ]);
  });

  it("keeps inline activation payloads inline", async () => {
    mockedInvoke.mockResolvedValue({
      fingerprint: "nord-fingerprint",
      id: "nord",
      source: { kind: "inline", css: ":root { --theme-accent: blue; }" },
      token: "inline-token"
    });

    await expect(prepareNativeThemeActivation("nord", "nord-fingerprint")).resolves.toEqual({
      fingerprint: "nord-fingerprint",
      id: "nord",
      source: { kind: "inline", css: ":root { --theme-accent: blue; }" },
      token: "inline-token"
    });
    expect(mockedConvertFileSrc).not.toHaveBeenCalled();
  });

  it("converts stylesheet paths and appends an encoded fingerprint query", async () => {
    mockedConvertFileSrc.mockReturnValue("http://asset.local/theme.css");
    mockedInvoke.mockResolvedValue({
      fingerprint: "drake fingerprint/?&",
      id: "drake-ayu",
      source: { kind: "stylesheet", path: "/app/themes/drake ayu/theme.css" },
      token: "resource-token"
    });

    await expect(prepareNativeThemeActivation("drake-ayu", "drake fingerprint/?&")).resolves.toEqual({
      fingerprint: "drake fingerprint/?&",
      id: "drake-ayu",
      source: {
        kind: "stylesheet",
        href: "http://asset.local/theme.css?fingerprint=drake%20fingerprint%2F%3F%26"
      },
      token: "resource-token"
    });
    expect(mockedConvertFileSrc).toHaveBeenCalledWith("/app/themes/drake ayu/theme.css");
  });

  it("preserves an existing asset query and fragment when adding the fingerprint", async () => {
    mockedConvertFileSrc.mockReturnValue("asset://localhost/theme.css?scope=lease#face");
    mockedInvoke.mockResolvedValue({
      fingerprint: "drake fingerprint",
      id: "drake-ayu",
      source: { kind: "stylesheet", path: "/app/themes/drake-ayu/theme.css" },
      token: "resource-token"
    });

    await expect(prepareNativeThemeActivation("drake-ayu", "drake fingerprint")).resolves.toMatchObject({
      source: {
        kind: "stylesheet",
        href: "asset://localhost/theme.css?scope=lease&fingerprint=drake%20fingerprint#face"
      }
    });
  });

  it("uses the native picker for import and treats cancel as a non-error", async () => {
    mockedOpen.mockResolvedValueOnce(null).mockResolvedValueOnce("/tmp/nord.css");
    mockedInvoke.mockResolvedValue(undefined);

    await expect(importNativeTheme()).resolves.toBeNull();
    await importNativeTheme();

    expect(mockedInvoke).toHaveBeenCalledWith("import_theme_file", { sourcePath: "/tmp/nord.css" });
    expect(mockedOpen).toHaveBeenCalledWith({
      directory: false,
      filters: [{ extensions: ["css", "theme"], name: "Theme" }],
      multiple: false
    });
  });

  it("replaces from the conflict source and opens the owned directory", async () => {
    mockedInvoke
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce("/app/themes");

    await replaceNativeTheme("/tmp/new.css", "old-fingerprint");
    await openNativeThemeDirectory();

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "replace_theme_file", {
      expectedFingerprint: "old-fingerprint",
      sourcePath: "/tmp/new.css"
    });
    expect(mockedOpenPath).toHaveBeenCalledWith("/app/themes");
  });

  it("fails closed when the native activation dialog rejects", async () => {
    mockedAsk.mockRejectedValue(new Error("dialog unavailable"));

    await expect(confirmNativeThemeActivation("Nord")).resolves.toBe(false);
  });
});
