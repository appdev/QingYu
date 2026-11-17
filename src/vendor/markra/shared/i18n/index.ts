import deMessages from "./locales/de.ts";
import enMessages from "./locales/en.ts";
import esMessages from "./locales/es.ts";
import frMessages from "./locales/fr.ts";
import itMessages from "./locales/it.ts";
import jaMessages from "./locales/ja.ts";
import koMessages from "./locales/ko.ts";
import ptBrMessages from "./locales/pt-BR.ts";
import ruMessages from "./locales/ru.ts";
import zhCnMessages from "./locales/zh-CN.ts";
import zhTwMessages from "./locales/zh-TW.ts";
import type { AppLanguage, I18nKey, LocaleMessages } from "./locales/types.ts";

export type { AppLanguage, I18nKey };

export const supportedLanguages: Array<{
  code: AppLanguage;
  label: string;
}> = [
  { code: "en", label: "English" },
  { code: "zh-CN", label: "简体中文" },
  { code: "zh-TW", label: "繁體中文" },
  { code: "ja", label: "日本語" },
  { code: "ko", label: "한국어" },
  { code: "fr", label: "Français" },
  { code: "de", label: "Deutsch" },
  { code: "es", label: "Español" },
  { code: "pt-BR", label: "Português (Brasil)" },
  { code: "it", label: "Italiano" },
  { code: "ru", label: "Русский" }
];

const aiTranslationLanguageNames: Record<AppLanguage, string> = {
  de: "German",
  en: "English",
  es: "Spanish",
  fr: "French",
  it: "Italian",
  ja: "Japanese",
  ko: "Korean",
  "pt-BR": "Brazilian Portuguese",
  ru: "Russian",
  "zh-CN": "Simplified Chinese",
  "zh-TW": "Traditional Chinese"
};

const dictionaries: Record<AppLanguage, LocaleMessages> = {
  en: enMessages,
  "zh-CN": zhCnMessages,
  "zh-TW": zhTwMessages,
  ja: jaMessages,
  ko: koMessages,
  fr: frMessages,
  de: deMessages,
  es: esMessages,
  "pt-BR": ptBrMessages,
  it: itMessages,
  ru: ruMessages
};

export function isAppLanguage(value: unknown): value is AppLanguage {
  return supportedLanguages.some((language) => language.code === value);
}

export function aiTranslationLanguageName(language: AppLanguage | null | undefined) {
  if (!language) return "English";

  return aiTranslationLanguageNames[language] ?? "English";
}

export function t(language: AppLanguage, key: string) {
  const i18nKey = key as I18nKey;

  return dictionaries[language][i18nKey] ?? enMessages[i18nKey] ?? key;
}
