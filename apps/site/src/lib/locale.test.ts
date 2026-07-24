import { localeStorageKey, readStoredLocale, writeStoredLocale } from "./locale";

describe("site locale storage", () => {
  it("defaults to Chinese and accepts only known locale values", () => {
    const storage = new Map<string, string>();
    const adapter = {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value)
    };

    expect(readStoredLocale(adapter)).toBe("zh-CN");
    storage.set(localeStorageKey, "fr");
    expect(readStoredLocale(adapter)).toBe("zh-CN");
    expect(writeStoredLocale(adapter, "en")).toBe(true);
    expect(readStoredLocale(adapter)).toBe("en");
  });

  it("survives unavailable storage", () => {
    const storage = {
      getItem: () => { throw new Error("blocked"); },
      setItem: () => { throw new Error("blocked"); }
    };

    expect(readStoredLocale(storage)).toBe("zh-CN");
    expect(writeStoredLocale(storage, "en")).toBe(false);
  });
});
