import {
  createDefaultAppRuntime,
  type AppSettingsGroup,
  type AppSettingsRuntime,
  type KernelDomainPort,
  type KernelSettingEntrySnapshot,
  type KernelSettingKey,
  type KernelSettingValue,
  type KernelSettingsSnapshot,
} from "../index";

export function createKernelSettingsRuntime(
  kernel: KernelDomainPort,
): AppSettingsRuntime {
  const local = createDefaultAppRuntime().settings;
  const localGroupValues = new Map<AppSettingsGroup, unknown>();

  const readGroup = async <TValue>(group: AppSettingsGroup) => {
    const remote = mapGroup(await kernel.settings.read(), group);
    const localValue = localGroupValues.get(group);
    if (isRecord(localValue) && isRecord(remote)) {
      return { ...localValue, ...remote } as TValue;
    }
    return (remote ?? localValue) as TValue | undefined;
  };

  const writeGroup = async (group: AppSettingsGroup, value: unknown) => {
    const current = await kernel.settings.read();
    const values = groupEntries(group, value);
    await kernel.settings.patch({ expectedRevision: current.revision, values });
    localGroupValues.set(group, cloneSetting(value));
    return undefined;
  };

  return {
    ...local,
    readGroup,
    replacePortable: async (settings) => {
      const current = await kernel.settings.read();
      const portable = requireRecord(settings);
      const groups: Array<[AppSettingsGroup, unknown]> = [
        ["appearance", {
          appearanceMode: portable.appearanceMode,
          darkTheme: portable.darkTheme,
          lightTheme: portable.lightTheme,
        }],
        ["customThemeCss", portable.customThemeCss],
        ["language", portable.language],
        ["editorPreferences", portable.editorPreferences],
        ["fileIgnoreSettings", portable.fileIgnoreSettings],
        ["exportSettings", portable.exportSettings],
      ];
      const values = groups.flatMap(([group, value]) => groupEntries(group, value));
      await kernel.settings.patch({ expectedRevision: current.revision, values });
      groups.forEach(([group, value]) => localGroupValues.set(group, cloneSetting(value)));
      return undefined;
    },
    writeGroup,
  };
}

function mapGroup(snapshot: KernelSettingsSnapshot, group: AppSettingsGroup): unknown {
  const values = new Map(snapshot.values.map((entry) => [entry.key, settingValue(entry.value)]));
  switch (group) {
    case "appearance":
      return {
        appearanceMode: values.get("appearance.mode"),
        darkTheme: values.get("appearance.darkTheme"),
        lightTheme: values.get("appearance.lightTheme"),
      };
    case "customThemeCss":
      return {
        dark: values.get("theme.customCss.dark"),
        light: values.get("theme.customCss.light"),
      };
    case "language": return values.get("language");
    case "editorPreferences":
      return compactRecord({
        bodyFontSize: values.get("editor.bodyFontSize"),
        contentWidth: values.get("editor.contentWidth"),
        contentWidthPx: values.get("editor.contentWidthPx"),
        editorFontFamily: values.get("editor.fontFamily"),
        lineHeight: values.get("editor.lineHeight"),
        paragraphSpacingPx: values.get("editor.paragraphSpacingPx"),
        showWordCount: values.get("editor.showWordCount"),
        viewMode: values.get("editor.viewMode"),
        wrapCodeBlocks: values.get("editor.wrapCodeBlocks"),
      });
    case "fileIgnoreSettings":
      return compactRecord({ rules: values.get("files.ignoreRules") });
    case "exportSettings":
      return compactRecord({
        fontFamily: values.get("export.fontFamily"),
        pdfAuthor: values.get("export.pdfAuthor"),
        pdfFooter: values.get("export.pdfFooter"),
        pdfHeader: values.get("export.pdfHeader"),
        pdfHeightMm: values.get("export.pdfHeightMm"),
        pdfMarginMm: values.get("export.pdfMarginMm"),
        pdfMarginPreset: values.get("export.pdfMarginPreset"),
        pdfPageBreakOnH1: values.get("export.pdfPageBreakOnH1"),
        pdfPageSize: values.get("export.pdfPageSize"),
        pdfWidthMm: values.get("export.pdfWidthMm"),
      });
  }
}

function groupEntries(
  group: AppSettingsGroup,
  value: unknown,
): KernelSettingEntrySnapshot[] {
  if (group === "language") {
    return [entry("language", stringValue(value))];
  }
  const record = requireRecord(value);
  switch (group) {
    case "appearance":
      return [
        entry("appearance.mode", stringValue(record.appearanceMode)),
        entry("appearance.lightTheme", stringValue(record.lightTheme)),
        entry("appearance.darkTheme", stringValue(record.darkTheme)),
      ];
    case "customThemeCss":
      return [
        entry("theme.customCss.light", stringValue(record.light)),
        entry("theme.customCss.dark", stringValue(record.dark)),
      ];
    case "editorPreferences":
      return compactEntries([
        optionalEntry("editor.bodyFontSize", record.bodyFontSize, integerValue),
        optionalEntry("editor.contentWidth", record.contentWidth, stringValue),
        optionalEntry("editor.contentWidthPx", record.contentWidthPx, nullableIntegerValue),
        optionalEntry("editor.fontFamily", record.editorFontFamily, fontFamilyValue),
        optionalEntry("editor.lineHeight", record.lineHeight, numberValue),
        optionalEntry("editor.paragraphSpacingPx", record.paragraphSpacingPx, integerValue),
        optionalEntry("editor.showWordCount", record.showWordCount, booleanValue),
        optionalEntry("editor.wrapCodeBlocks", record.wrapCodeBlocks, booleanValue),
        optionalEntry("editor.viewMode", record.viewMode, stringValue),
      ]);
    case "fileIgnoreSettings":
      return [entry("files.ignoreRules", stringValue(record.rules))];
    case "exportSettings":
      return compactEntries([
        optionalEntry("export.fontFamily", record.fontFamily, nullableStringValue),
        optionalEntry("export.pdfAuthor", record.pdfAuthor, stringValue),
        optionalEntry("export.pdfFooter", record.pdfFooter, stringValue),
        optionalEntry("export.pdfHeader", record.pdfHeader, stringValue),
        optionalEntry("export.pdfHeightMm", record.pdfHeightMm, integerValue),
        optionalEntry("export.pdfWidthMm", record.pdfWidthMm, integerValue),
        optionalEntry("export.pdfMarginMm", record.pdfMarginMm, integerValue),
        optionalEntry("export.pdfMarginPreset", record.pdfMarginPreset, stringValue),
        optionalEntry("export.pdfPageBreakOnH1", record.pdfPageBreakOnH1, booleanValue),
        optionalEntry("export.pdfPageSize", record.pdfPageSize, stringValue),
      ]);
  }
}

function entry(key: KernelSettingKey, value: KernelSettingValue): KernelSettingEntrySnapshot {
  return { key, value };
}

function optionalEntry(
  key: KernelSettingKey,
  value: unknown,
  map: (candidate: unknown) => KernelSettingValue,
) {
  return value === undefined ? null : entry(key, map(value));
}

function compactEntries(
  entries: Array<KernelSettingEntrySnapshot | null>,
): KernelSettingEntrySnapshot[] {
  const compacted = entries.filter((entry): entry is KernelSettingEntrySnapshot => entry !== null);
  if (compacted.length === 0) throw new Error("No portable Server settings were provided.");
  return compacted;
}

function settingValue(value: KernelSettingValue): unknown {
  if (value.type === "font-family") return { ...value.value };
  return value.value;
}

function stringValue(value: unknown): KernelSettingValue {
  if (typeof value !== "string") throw new Error("The Server setting must be a string.");
  return { type: "string", value };
}

function booleanValue(value: unknown): KernelSettingValue {
  if (typeof value !== "boolean") throw new Error("The Server setting must be a boolean.");
  return { type: "boolean", value };
}

function integerValue(value: unknown): KernelSettingValue {
  if (!Number.isSafeInteger(value)) throw new Error("The Server setting must be an integer.");
  return { type: "integer", value: value as number };
}

function numberValue(value: unknown): KernelSettingValue {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error("The Server setting must be finite.");
  }
  return { type: "number", value };
}

function nullableIntegerValue(value: unknown): KernelSettingValue {
  if (value !== null && !Number.isSafeInteger(value)) {
    throw new Error("The Server setting must be a nullable integer.");
  }
  return { type: "nullable-integer", value: value as number | null };
}

function nullableStringValue(value: unknown): KernelSettingValue {
  if (value !== null && typeof value !== "string") {
    throw new Error("The Server setting must be a nullable string.");
  }
  return { type: "nullable-string", value };
}

function fontFamilyValue(value: unknown): KernelSettingValue {
  const family = requireRecord(value);
  if (family.source === "theme" && family.family === null) {
    return { type: "font-family", value: { family: null, source: "theme" } };
  }
  if (family.source === "system" && typeof family.family === "string") {
    return { type: "font-family", value: { family: family.family, source: "system" } };
  }
  throw new Error("The Server font setting is invalid.");
}

function requireRecord(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) throw new Error("The Server setting group is invalid.");
  return value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function compactRecord(value: Record<string, unknown>) {
  return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== undefined));
}

function cloneSetting<T>(value: T): T {
  if (typeof structuredClone === "function") return structuredClone(value);
  return JSON.parse(JSON.stringify(value)) as T;
}
