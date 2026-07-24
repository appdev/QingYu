import { act, renderHook, waitFor } from "@testing-library/react";
import { defaultEditorPreferences, getStoredEditorPreferences } from "../lib/settings/app-settings";
import { listenAppEditorPreferencesChanged } from "../lib/settings/settings-events";
import { useEditorPreferences } from "./useEditorPreferences";

vi.mock("../lib/settings/app-settings", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/settings/app-settings")>(),
  getStoredEditorPreferences: vi.fn()
}));

vi.mock("../lib/settings/settings-events", () => ({
  listenAppEditorPreferencesChanged: vi.fn()
}));

const mockedGetStoredEditorPreferences = vi.mocked(getStoredEditorPreferences);
const mockedListenAppEditorPreferencesChanged = vi.mocked(listenAppEditorPreferencesChanged);

describe("useEditorPreferences", () => {
  beforeEach(() => {
    mockedGetStoredEditorPreferences.mockReset();
    mockedListenAppEditorPreferencesChanged.mockReset();
    mockedListenAppEditorPreferencesChanged.mockResolvedValue(() => {});
  });

  it("loads editor preferences and reacts to cross-window preference changes", async () => {
    let onPreferencesChanged: Parameters<typeof listenAppEditorPreferencesChanged>[0] | null = null;
    mockedGetStoredEditorPreferences.mockResolvedValue({
      ...defaultEditorPreferences,
      autoRevealActiveFile: true,
      bodyFontSize: 16,
      tableColumnWidthMode: "even"
    });
    mockedListenAppEditorPreferencesChanged.mockImplementation(async (listener) => {
      onPreferencesChanged = listener;
      return () => {};
    });

    const { result } = renderHook(() => useEditorPreferences());

    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.loading).toBe(false));
    act(() => {
      onPreferencesChanged?.({
        ...defaultEditorPreferences,
        bodyFontSize: 18,
        contentWidth: "wide",
        showWordCount: false
      });
    });

    expect(result.current.preferences.bodyFontSize).toBe(18);
    expect(result.current.preferences.contentWidth).toBe("wide");
    expect(result.current.preferences.showWordCount).toBe(false);
  });

  it("keeps a live preference update when the initial stored preferences resolve later", async () => {
    let onPreferencesChanged: Parameters<typeof listenAppEditorPreferencesChanged>[0] | null = null;
    let resolveStoredPreferences: (preferences: Awaited<ReturnType<typeof getStoredEditorPreferences>>) => unknown = () => {};
    mockedGetStoredEditorPreferences.mockReturnValue(new Promise((resolve) => {
      resolveStoredPreferences = resolve;
    }));
    mockedListenAppEditorPreferencesChanged.mockImplementation(async (listener) => {
      onPreferencesChanged = listener;
      return () => {};
    });

    const { result } = renderHook(() => useEditorPreferences());

    act(() => {
      onPreferencesChanged?.({
        ...defaultEditorPreferences,
        bodyFontSize: 18,
        showWordCount: false
      });
    });
    expect(result.current.preferences.bodyFontSize).toBe(18);
    expect(result.current.preferences.showWordCount).toBe(false);

    await act(async () => {
      resolveStoredPreferences({
        ...defaultEditorPreferences,
        bodyFontSize: 16,
        showWordCount: true
      });
    });

    expect(result.current.loading).toBe(false);
    expect(result.current.preferences.bodyFontSize).toBe(18);
    expect(result.current.preferences.showWordCount).toBe(false);
  });
});
