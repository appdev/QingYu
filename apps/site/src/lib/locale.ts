import type { SiteLocale } from "../content";

export const localeStorageKey = "qingyu.site.locale";

type LocaleStorage = Pick<Storage, "getItem" | "setItem">;

export function readStoredLocale(storage: Pick<LocaleStorage, "getItem">): SiteLocale {
  try {
    return storage.getItem(localeStorageKey) === "en" ? "en" : "zh-CN";
  } catch {
    return "zh-CN";
  }
}

export function writeStoredLocale(storage: Pick<LocaleStorage, "setItem">, locale: SiteLocale) {
  try {
    storage.setItem(localeStorageKey, locale);
    return true;
  } catch {
    return false;
  }
}
