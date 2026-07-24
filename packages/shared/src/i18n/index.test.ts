import { describe, expect, it } from "vitest";
import { supportedLanguages, t } from "./index";
import deMessages from "./locales/de";
import enMessages, { syncEnMessages } from "./locales/en";
import esMessages from "./locales/es";
import frMessages from "./locales/fr";
import itMessages from "./locales/it";
import jaMessages from "./locales/ja";
import koMessages from "./locales/ko";
import ptBrMessages from "./locales/pt-BR";
import ruMessages from "./locales/ru";
import zhCnMessages from "./locales/zh-CN";
import zhTwMessages from "./locales/zh-TW";
import type { AppLanguage, I18nKey, LocaleMessages } from "./locales/types";

const nonEnglishLocaleMessages: Record<Exclude<AppLanguage, "en">, LocaleMessages> = {
  de: deMessages,
  es: esMessages,
  fr: frMessages,
  it: itMessages,
  ja: jaMessages,
  ko: koMessages,
  "pt-BR": ptBrMessages,
  ru: ruMessages,
  "zh-CN": zhCnMessages,
  "zh-TW": zhTwMessages
};

const localeMessages: Record<AppLanguage, LocaleMessages> = {
  en: enMessages,
  ...nonEnglishLocaleMessages
};

const expectedProductNames: Record<AppLanguage, string> = {
  en: "QingYu",
  "zh-CN": "轻语",
  "zh-TW": "輕語",
  ja: "QingYu",
  ko: "QingYu",
  fr: "QingYu",
  de: "QingYu",
  es: "QingYu",
  "pt-BR": "QingYu",
  it: "QingYu",
  ru: "QingYu"
};

const sourceKeys = Object.keys(enMessages) as I18nKey[];

function untranslatedKeys(messages: LocaleMessages) {
  return sourceKeys.filter((key) => {
    const message = messages[key];

    return typeof message !== "string" || message.trim().length === 0;
  });
}

describe("i18n", () => {
  it("ships common app languages with English as the first default", () => {
    expect(supportedLanguages.map((language) => language.code)).toEqual([
      "en",
      "zh-CN",
      "zh-TW",
      "ja",
      "ko",
      "fr",
      "de",
      "es",
      "pt-BR",
      "it",
      "ru"
    ]);
  });

  it("falls back to the key when no translation exists", () => {
    expect(t("ru", "missing.key")).toBe("missing.key");
  });

  it("ships non-empty English labels for every translation key", () => {
    expect(untranslatedKeys(enMessages)).toEqual([]);
  });

  it("ships every English translation key in every supported locale", () => {
    for (const [language, messages] of Object.entries(nonEnglishLocaleMessages)) {
      expect(untranslatedKeys(messages), `${language} should define every English i18n key`).toEqual([]);
    }
  });

  it("ships the complete MCP settings and audit vocabulary in every locale", () => {
    const requiredKeys = [
      "settings.categories.mcp",
      "settings.mcp.summary",
      "settings.mcp.enabled",
      "settings.mcp.endpoint",
      "settings.mcp.workspace.current",
      "settings.mcp.permission.documentsRead",
      "settings.mcp.permission.syncRun",
      "settings.mcp.policy.confirmation",
      "settings.mcp.policy.dryRun",
      "settings.mcp.policy.deletion",
      "settings.mcp.policy.recycleBinCleanup",
      "settings.mcp.policy.recycleBinCleanup.never",
      "settings.mcp.policy.recycleBinCleanup.days7",
      "settings.mcp.policy.recycleBinCleanup.days30",
      "settings.mcp.policy.recycleBinCleanup.days90",
      "settings.mcp.policy.syncAfterWrite",
      "settings.mcp.policy.syncExecution",
      "settings.mcp.health.running",
      "settings.mcp.audit.time",
      "settings.mcp.audit.duration",
      "settings.mcp.audit.empty",
      "settings.mcp.audit.clear",
      "settings.mcp.error.revisionConflict",
      "settings.mcp.error.save",
      "settings.mcp.error.start"
    ] as I18nKey[];

    for (const [language, messages] of Object.entries(localeMessages)) {
      for (const key of requiredKeys) {
        expect(messages[key], `${language}:${key}`).toBeTypeOf("string");
        expect(messages[key]?.trim(), `${language}:${key}`).not.toBe("");
      }
    }
  });

  it("ships reviewed English and Simplified Chinese Compact editor copy", () => {
    const keys = [
      "compact.editor.files",
      "compact.editor.more",
      "compact.save.saved",
      "compact.save.dirty",
      "compact.save.saving",
      "compact.sync.configure",
      "compact.welcome.title",
      "compact.welcome.newDocument"
    ] as I18nKey[];

    expect(keys.map((key) => t("en", key))).toEqual([
      "Files",
      "More",
      "Saved",
      "Unsaved",
      "Saving",
      "Configure Sync",
      "Start writing",
      "New Document"
    ]);
    expect(keys.map((key) => t("zh-CN", key))).toEqual([
      "文件",
      "更多",
      "已保存",
      "未保存",
      "保存中",
      "配置同步",
      "开始写作",
      "新建文档"
    ]);
  });

  it("describes standalone onboarding and named current-notebook synchronization", () => {
    expect(enMessages).not.toHaveProperty("onboarding.action.openFolder");
    expect(zhCnMessages).not.toHaveProperty("onboarding.action.openFolder");
    expect(t("en", "onboarding.deferred.description")).toBe(
      "You can still open a standalone Markdown file without choosing a notebook directory."
    );
    expect(t("zh-CN", "onboarding.deferred.description")).toBe(
      "你仍可打开独立的 Markdown 文件，而不选择笔记目录。"
    );
    expect(t("en", "onboarding.external.description")).not.toMatch(/external folder/i);
    expect(t("zh-CN", "onboarding.external.description")).not.toMatch(/外部目录|外部文件夹/u);
    expect(t("en", "settings.sync.summaryDescription")).toContain(
      "notes/<directory-name>/"
    );
    expect(t("zh-CN", "settings.sync.summaryDescription")).toContain("notes/<目录名>/");
  });

  it("provides concise localized copy for the transient sync error toast", () => {
    const keys = [
      "settings.sync.toastIncomplete",
      "settings.sync.toastRetry",
      "settings.sync.toastRetrying"
    ] as I18nKey[];

    expect(keys.map((key) => t("en", key))).toEqual([
      "Sync did not complete",
      "Retry",
      "Syncing…"
    ]);
    expect(keys.map((key) => t("zh-CN", key))).toEqual([
      "同步未完成",
      "重试",
      "正在同步…"
    ]);
  });

  it("describes directory CLI arguments as notebook switches in every locale", () => {
    const staleCliCopy = /open folders and Markdown|打开文件夹和 Markdown|開啟資料夾和 Markdown/i;

    for (const [language, messages] of Object.entries(localeMessages)) {
      expect(
        messages["settings.shellCommand.descriptionMissing"],
        `${language}:settings.shellCommand.descriptionMissing`
      ).not.toMatch(staleCliCopy);
    }
    expect(t("en", "settings.shellCommand.descriptionMissing")).toBe(
      "Install the markra command to switch notebook directories or open Markdown files from Terminal."
    );
    expect(t("zh-CN", "settings.shellCommand.descriptionMissing")).toBe(
      "安装 markra 命令后，可从终端切换笔记目录或打开 Markdown 文件。"
    );
  });

  it("uses reviewed English as the fallback copy for remaining Compact keys in other locales", () => {
    const localizedCompactKeys = new Set<I18nKey>([
      "compact.files.cancel",
      "compact.files.create",
      "compact.files.nameExists",
      "compact.files.nameInvalid",
      "compact.files.newFileName",
      "compact.files.newFolderName",
      "compact.files.operationFailed",
      "compact.files.rename",
      "compact.settings.manageNotebooks",
      "compact.settings.switchNotebookDirectory"
    ]);
    const compactKeys = sourceKeys.filter((key) =>
      key.startsWith("compact.") && !localizedCompactKeys.has(key)
    );

    for (const [language, messages] of Object.entries(nonEnglishLocaleMessages)) {
      if (language === "zh-CN") continue;
      for (const key of compactKeys) {
        expect(messages[key], `${language}:${key} should use Compact fallback copy`).toBe(enMessages[key]);
      }
    }
  });

  it("ships localized Compact name-dialog actions and errors in every non-English locale", () => {
    const localizedNameDialogKeys = [
      "compact.files.cancel",
      "compact.files.create",
      "compact.files.nameExists",
      "compact.files.nameInvalid",
      "compact.files.newFileName",
      "compact.files.newFolderName",
      "compact.files.operationFailed",
      "compact.files.rename"
    ] as I18nKey[];

    for (const [language, messages] of Object.entries(nonEnglishLocaleMessages)) {
      for (const key of localizedNameDialogKeys) {
        expect(messages[key], `${language}:${key} should be localized`).not.toBe(enMessages[key]);
      }
    }
  });

  it("ships Simplified Chinese resource settings copy and explicit English fallback elsewhere", () => {
    const resourceKeys = sourceKeys.filter((key) =>
      key === "settings.categories.resources" || key.startsWith("settings.resources.")
    );

    expect(t("zh-CN", "settings.categories.resources")).toBe("资源");
    expect(t("zh-CN", "settings.resources.unused")).toBe("未引用资源");
    expect(t("zh-CN", "settings.resources.missing")).toBe("丢失资源");

    for (const [language, messages] of Object.entries(nonEnglishLocaleMessages)) {
      if (language === "zh-CN") continue;
      for (const key of resourceKeys) {
        expect(messages[key], `${language}:${key} should use resource settings fallback copy`).toBe(enMessages[key]);
      }
    }
  });

  it("keeps every sync catalog application-scoped and free of project configuration files", () => {
    const legacyProjectSyncWording = /\.qingyu\/config\.json|project(?:'s)? (?:folder|configuration)|项目同步配置|專案同步設定|プロジェクト|프로젝트|конфигурац(?:ия|ии) проекта/i;
    for (const [language, messages] of Object.entries({ en: enMessages, ...nonEnglishLocaleMessages })) {
      for (const [key, value] of Object.entries(messages)) {
        if (
          !key.startsWith("settings.sync.") &&
          !key.startsWith("compact.sync.") &&
          key !== "compact.error.sync"
        ) continue;
        expect(value, `${language}:${key} should describe application sync`).not.toMatch(legacyProjectSyncWording);
      }
    }
  });

  it("uses the reviewed English sync catalog for locales awaiting application-sync translations", () => {
    const localizedTraditionalChineseKeys = new Set([
      "settings.sync.summaryDescription",
      "settings.sync.malformedDescription",
      "settings.sync.recoveryDescription",
      "settings.sync.remotePathDescription",
      "settings.sync.remotePathPlaceholder",
      "settings.sync.selectCloudNotebookDescription",
      "settings.sync.s3RequestTimeout",
      "settings.sync.s3RequestTimeoutDescription",
      "settings.sync.seconds",
      "settings.sync.s3AddressingStyle",
      "settings.sync.s3AddressingStyleDescription",
      "settings.sync.s3AddressingStyle.auto",
      "settings.sync.s3AddressingStyle.path",
      "settings.sync.s3AddressingStyle.virtualHosted",
      "settings.sync.s3TlsVerification",
      "settings.sync.s3TlsVerificationDescription",
      "settings.sync.s3TlsVerification.verify",
      "settings.sync.s3TlsVerification.skip"
    ]);
    for (const [language, messages] of Object.entries(nonEnglishLocaleMessages)) {
      if (language === "zh-CN") continue;
      for (const key of Object.keys(syncEnMessages) as I18nKey[]) {
        if (language === "zh-TW" && localizedTraditionalChineseKeys.has(key)) continue;
        expect(messages[key], `${language}:${key} should use the reviewed sync fallback`)
          .toBe(enMessages[key]);
      }
    }
  });

  it("labels the launch trigger as an application launch", () => {
    expect(t("en", "settings.sync.trigger.appLaunch")).toBe("Application launched");
    expect(t("zh-CN", "settings.sync.trigger.appLaunch")).toBe("启动应用");
  });

  it("uses the localized product name throughout every message catalog", () => {
    const legacyDisplayName = ["Mar", "kra"].join("");
    const legacyPattern = new RegExp(`\\b${legacyDisplayName}\\b`);
    const nonlocalizedPattern = /\bQingYu\b/;

    for (const [language, messages] of Object.entries(localeMessages) as [AppLanguage, LocaleMessages][]) {
      const productName = expectedProductNames[language];
      const values = Object.values(messages).filter((value): value is string => typeof value === "string");

      expect(messages["menu.hide"], language).toContain(productName);
      expect(messages["menu.quit"], language).toContain(productName);
      expect(values.filter((value) => legacyPattern.test(value)), language).toEqual([]);
      if (language === "zh-CN" || language === "zh-TW") {
        const localizedProductName = expectedProductNames[language];
        const hanAdjacentSpacingPattern = new RegExp(
          `[\\u3400-\\u9fff] ${localizedProductName}|${localizedProductName} [\\u3400-\\u9fff]`
        );

        expect(values.filter((value) => nonlocalizedPattern.test(value)), language).toEqual([]);
        expect(values.filter((value) => hanAdjacentSpacingPattern.test(value)), language).toEqual([]);
      }
    }
  });
});
