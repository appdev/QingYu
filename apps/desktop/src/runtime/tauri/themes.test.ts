import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import {
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
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openPath: vi.fn() }));

const mockedInvoke = vi.mocked(invoke);
const mockedConvertFileSrc = vi.mocked(convertFileSrc);
const mockedOpen = vi.mocked(open);
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
      ["list_themes", { refresh: false }],
      ["prepare_theme_activation", { id: "nord", expectedFingerprint: "fingerprint" }],
      ["commit_theme_activation", { token: "commit-token" }],
      ["cancel_theme_activation", { token: "cancel-token" }],
      ["release_theme_activation", undefined],
      ["delete_theme", { id: "nord", expectedFingerprint: "fingerprint" }]
    ]);
  });

  it("marks only explicit catalog refreshes as filesystem rescans", async () => {
    await listNativeThemes();
    await listNativeThemes(true);

    expect(mockedInvoke.mock.calls).toEqual([
      ["list_themes", { refresh: false }],
      ["list_themes", { refresh: true }]
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

  it("converts the stylesheet directory and appends an encoded fingerprint query", async () => {
    mockedConvertFileSrc.mockReturnValue("http://asset.local");
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
    expect(mockedConvertFileSrc).toHaveBeenCalledWith("/app/themes/drake ayu");
  });

  it("keeps relative resource URLs inside the converted theme directory", async () => {
    mockedConvertFileSrc.mockImplementation((path) =>
      `asset://localhost/${encodeURIComponent(path)}`
    );
    mockedInvoke.mockResolvedValue({
      fingerprint: "drake-fingerprint",
      id: "drake-ayu",
      source: { kind: "stylesheet", path: "/app/themes/drake ayu/theme.css" },
      token: "resource-token"
    });

    const payload = await prepareNativeThemeActivation("drake-ayu", "drake-fingerprint");
    if (payload.source.kind !== "stylesheet") throw new Error("expected stylesheet payload");

    expect(new URL("./assets/fonts/JetBrainsMono-Regular.woff2", payload.source.href).href).toBe(
      "asset://localhost/%2Fapp%2Fthemes%2Fdrake%20ayu/assets/fonts/JetBrainsMono-Regular.woff2"
    );
  });

  it.each([
    {
      name: "drive-letter",
      path: "C:\\Users\\Ying Chen\\AppData\\Roaming\\QingYu\\themes\\drake-ayu\\theme.css",
      expectedDirectory: "C:\\Users\\Ying Chen\\AppData\\Roaming\\QingYu\\themes\\drake-ayu",
      expectedHref: "http://asset.localhost/C%3A%5CUsers%5CYing%20Chen%5CAppData%5CRoaming%5CQingYu%5Cthemes%5Cdrake-ayu/theme.css?fingerprint=windows-fingerprint"
    },
    {
      name: "UNC",
      path: "\\\\theme-server\\shared themes\\drake-ayu\\theme.css",
      expectedDirectory: "\\\\theme-server\\shared themes\\drake-ayu",
      expectedHref: "http://asset.localhost/%5C%5Ctheme-server%5Cshared%20themes%5Cdrake-ayu/theme.css?fingerprint=windows-fingerprint"
    }
  ])("preserves the $name directory with Tauri's Windows convertFileSrc shape", async ({
    expectedDirectory,
    expectedHref,
    path
  }) => {
    mockedConvertFileSrc.mockImplementation((filePath, protocol = "asset") =>
      `http://${protocol}.localhost/${encodeURIComponent(filePath)}`
    );
    mockedInvoke.mockResolvedValue({
      fingerprint: "windows-fingerprint",
      id: "drake-ayu",
      source: { kind: "stylesheet", path },
      token: "resource-token"
    });

    await expect(prepareNativeThemeActivation("drake-ayu", "windows-fingerprint")).resolves.toMatchObject({
      source: {
        kind: "stylesheet",
        href: expectedHref
      }
    });
    expect(mockedConvertFileSrc).toHaveBeenCalledWith(expectedDirectory);
  });

  it("preserves an existing asset query and fragment when adding the fingerprint", async () => {
    mockedConvertFileSrc.mockReturnValue("asset://localhost/lease?scope=lease#face");
    mockedInvoke.mockResolvedValue({
      fingerprint: "drake fingerprint",
      id: "drake-ayu",
      source: { kind: "stylesheet", path: "/app/themes/drake-ayu/theme.css" },
      token: "resource-token"
    });

    await expect(prepareNativeThemeActivation("drake-ayu", "drake fingerprint")).resolves.toMatchObject({
      source: {
        kind: "stylesheet",
        href: "asset://localhost/lease/theme.css?scope=lease&fingerprint=drake%20fingerprint#face"
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

});
