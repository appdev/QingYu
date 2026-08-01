import {
  createSettingsStoreHarness,
  resetSettingsStoreRuntime,
  setupSettingsStoreHarness
} from "../../test/settings-store";
import { getStoredExportSettings, saveStoredExportSettings } from "./app-settings";
import { normalizeExportSettings } from "./export-settings";

const settingsStore = createSettingsStoreHarness();
const { loadStore: mockedLoadStore, store } = settingsStore;
const appConfig = settingsStore.appConfig;

describe("export settings", () => {
  beforeEach(() => {
    setupSettingsStoreHarness(settingsStore);
  });

  afterEach(() => {
    resetSettingsStoreRuntime();
  });

  it("loads the default export settings", async () => {
    store.get.mockResolvedValue(undefined);

    await expect(getStoredExportSettings()).resolves.toEqual({
      fontFamily: null,
      pandocArgs: "",
      pandocPath: "",
      pdfAuthor: "",
      pdfFooter: "",
      pdfHeader: "",
      pdfHeightMm: 297,
      pdfMarginMm: 18,
      pdfMarginPreset: "default",
      pdfPageBreakOnH1: false,
      pdfPageSize: "default",
      pdfWidthMm: 210
    });

    expect(store.get).toHaveBeenCalledWith("exportSettings");
    expect(mockedLoadStore).toHaveBeenCalledWith("settings.json", { autoSave: false, defaults: {} });
    expect(mockedLoadStore).not.toHaveBeenCalledWith("local-state.json", expect.anything());
  });

  it("normalizes persisted export settings", () => {
    expect(normalizeExportSettings({
      fontFamily: " Example Serif ",
      pandocArgs: " --toc --metadata title=\"Draft\" ",
      pandocPath: " /opt/homebrew/bin/pandoc ",
      pdfAuthor: " Ada Lovelace ",
      pdfFooter: "Page footer",
      pdfHeader: "Draft header",
      pdfHeightMm: 279,
      pdfMarginMm: 24,
      pdfMarginPreset: "custom",
      pdfPageBreakOnH1: true,
      pdfPageSize: "letter",
      pdfWidthMm: 216
    })).toEqual({
      fontFamily: "Example Serif",
      pandocArgs: "--toc --metadata title=\"Draft\"",
      pandocPath: "/opt/homebrew/bin/pandoc",
      pdfAuthor: "Ada Lovelace",
      pdfFooter: "Page footer",
      pdfHeader: "Draft header",
      pdfHeightMm: 279,
      pdfMarginMm: 24,
      pdfMarginPreset: "custom",
      pdfPageBreakOnH1: true,
      pdfPageSize: "letter",
      pdfWidthMm: 216
    });
    expect(normalizeExportSettings({
      fontFamily: 42,
      pandocArgs: "x".repeat(1500),
      pandocPath: "x".repeat(700),
      pdfHeightMm: 9999,
      pdfMarginMm: 999,
      pdfMarginPreset: "custom",
      pdfPageSize: "custom",
      pdfWidthMm: 10
    })).toEqual({
      fontFamily: null,
      pandocArgs: "x".repeat(1000),
      pandocPath: "x".repeat(500),
      pdfAuthor: "",
      pdfFooter: "",
      pdfHeader: "",
      pdfHeightMm: 2000,
      pdfMarginMm: 60,
      pdfMarginPreset: "custom",
      pdfPageBreakOnH1: false,
      pdfPageSize: "custom",
      pdfWidthMm: 50
    });
    expect(normalizeExportSettings({ pdfMarginMm: 24 })).toEqual({
      fontFamily: null,
      pandocArgs: "",
      pandocPath: "",
      pdfAuthor: "",
      pdfFooter: "",
      pdfHeader: "",
      pdfHeightMm: 297,
      pdfMarginMm: 24,
      pdfMarginPreset: "custom",
      pdfPageBreakOnH1: false,
      pdfPageSize: "default",
      pdfWidthMm: 210
    });
  });

  it("persists normalized export settings", async () => {
    await saveStoredExportSettings({
      fontFamily: " Example Serif ",
      pandocArgs: " --toc ",
      pandocPath: " /usr/local/bin/pandoc ",
      pdfAuthor: "Ada",
      pdfFooter: "Footer",
      pdfHeader: "Header",
      pdfHeightMm: 279,
      pdfMarginMm: 72,
      pdfMarginPreset: "custom",
      pdfPageBreakOnH1: true,
      pdfPageSize: "letter",
      pdfWidthMm: 216
    });

    expect(store.set).toHaveBeenCalledWith("exportSettings", {
      fontFamily: "Example Serif",
      pandocArgs: "--toc",
      pdfAuthor: "Ada",
      pdfFooter: "Footer",
      pdfHeader: "Header",
      pdfHeightMm: 279,
      pdfMarginMm: 60,
      pdfMarginPreset: "custom",
      pdfPageBreakOnH1: true,
      pdfPageSize: "letter",
      pdfWidthMm: 216
    });
    expect(appConfig.patchState).toHaveBeenCalledWith([{
      path: "/usr/local/bin/pandoc",
      type: "set-pandoc-path"
    }]);
    expect(mockedLoadStore).toHaveBeenCalledWith("settings.json", { autoSave: false, defaults: {} });
    expect(mockedLoadStore).not.toHaveBeenCalledWith("local-state.json", expect.anything());
    expect(store.save).toHaveBeenCalledTimes(1);
  });
});
