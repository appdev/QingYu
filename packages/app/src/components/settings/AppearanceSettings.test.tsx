import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { translate } from "../../test/settings-components";
import { protectedThemeDescriptors, type ThemeDescriptor } from "../../lib/themes/theme-catalog";
import { AppearanceSettings } from "./AppearanceSettings";

const nord: ThemeDescriptor = {
  appearance: "dark",
  author: "Arctic Studio",
  fileName: "nord.css",
  fingerprint: "a".repeat(64),
  id: "nord",
  name: "Nord",
  preview: { accent: "#88c0d0", background: "#2e3440", panel: "#3b4252", text: "#eceff4" },
  source: "third-party",
  storageKind: "inlineCss"
};

const sepia: ThemeDescriptor = {
  appearance: "light",
  fileName: "sepia.css",
  fingerprint: "b".repeat(64),
  id: "sepia",
  name: "Sepia",
  preview: { accent: "#8b5e34", background: "#fbf0d9", panel: "#f3e3c4", text: "#3b2f22" },
  source: "third-party",
  storageKind: "inlineCss"
};

type ThemeController = ComponentProps<typeof AppearanceSettings>["themeController"];

function createThemeController(overrides: Partial<ThemeController> = {}): ThemeController {
  const lightThemes = [protectedThemeDescriptors[0], sepia] as ThemeDescriptor[];
  const darkThemes = [protectedThemeDescriptors[1], nord] as ThemeDescriptor[];

  return {
    activeTheme: sepia,
    appearanceMode: "light",
    darkTheme: "nord",
    lightTheme: "sepia",
    selectAppearanceMode: vi.fn(),
    selectTheme: vi.fn(),
    themeError: null,
    catalog: {
      capabilities: { canDelete: true, canImport: true, canOpenDirectory: true },
      darkThemes,
      deleteTheme: vi.fn(async () => undefined),
      error: null,
      importTheme: vi.fn(async () => null),
      invalidFiles: [],
      lightThemes,
      loading: false,
      openDirectory: vi.fn(async () => undefined),
      refresh: vi.fn(async () => undefined),
      replaceTheme: vi.fn(async () => nord),
      themes: [...lightThemes, ...darkThemes]
    },
    ...overrides
  };
}

describe("AppearanceSettings", () => {
  it("renders independent unequal light and dark catalogs and selects immediately", () => {
    const controller = createThemeController();
    controller.catalog.darkThemes = [protectedThemeDescriptors[1], nord, { ...nord, id: "midnight", name: "Midnight" }];

    render(<AppearanceSettings themeController={controller} translate={translate} />);

    const appearanceMode = screen.getByRole("radiogroup", { name: "Appearance mode" });
    const lightPalette = screen.getByRole("radiogroup", { name: "Light palette" });
    const darkPalette = screen.getByRole("radiogroup", { name: "Dark palette" });
    expect(within(lightPalette).getAllByRole("radio")).toHaveLength(2);
    expect(within(darkPalette).getAllByRole("radio")).toHaveLength(3);
    expect(within(lightPalette).getByRole("radio", { name: "Sepia" })).toHaveAttribute("aria-checked", "true");
    expect(within(darkPalette).getByRole("radio", { name: "Nord" })).toHaveAttribute("aria-checked", "true");

    fireEvent.click(within(appearanceMode).getByRole("radio", { name: "Dark" }));
    fireEvent.click(within(darkPalette).getByRole("radio", { name: "Midnight" }));

    expect(controller.selectAppearanceMode).toHaveBeenCalledWith("dark");
    expect(controller.selectTheme).toHaveBeenCalledWith(expect.objectContaining({ id: "midnight" }));
  });

  it("exposes the supported desktop theme file actions", async () => {
    const controller = createThemeController();
    render(<AppearanceSettings themeController={controller} translate={translate} />);

    await waitFor(() => expect(controller.catalog.refresh).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: "Import theme" }));
    await waitFor(() => expect(controller.catalog.importTheme).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh themes" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Refresh themes" }));
    await waitFor(() => expect(controller.catalog.refresh).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByRole("button", { name: "Open theme folder" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Open theme folder" }));

    await waitFor(() => {
      expect(controller.catalog.openDirectory).toHaveBeenCalledTimes(1);
    });
  });

  it("offers typed duplicate imports for replacement", async () => {
    const controller = createThemeController();
    controller.catalog.importTheme = vi.fn(async () => ({
      candidate: nord,
      existing: nord,
      kind: "conflict" as const,
      sourcePath: "/tmp/nord.css"
    }));
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<AppearanceSettings themeController={controller} translate={translate} />);

    fireEvent.click(screen.getByRole("button", { name: "Import theme" }));

    await waitFor(() => {
      expect(controller.catalog.replaceTheme).toHaveBeenCalledWith("/tmp/nord.css", nord.fingerprint);
    });
  });

  it("requires confirmation before deleting a third-party theme", async () => {
    const controller = createThemeController();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<AppearanceSettings themeController={controller} translate={translate} />);

    fireEvent.click(screen.getByRole("button", { name: "Delete theme: Nord" }));

    await waitFor(() => expect(controller.catalog.deleteTheme).toHaveBeenCalledWith(nord));
    expect(confirm).toHaveBeenCalledWith("Delete theme “Nord”?");
  });

  it("keeps refresh and selection on mobile while hiding desktop file actions", () => {
    const controller = createThemeController();
    controller.catalog.capabilities = {
      canDelete: true,
      canImport: false,
      canOpenDirectory: false
    };
    render(<AppearanceSettings themeController={controller} translate={translate} />);

    expect(screen.getByRole("button", { name: "Refresh themes" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete theme: Nord" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Import theme" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open theme folder" })).not.toBeInTheDocument();
  });
});
