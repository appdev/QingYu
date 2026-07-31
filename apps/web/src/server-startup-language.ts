export type ServerStartupLanguage = "en" | "zh-CN";

export function resolveServerStartupLanguage(
  search: string,
  preferredLanguages: readonly string[],
): ServerStartupLanguage {
  const requested = new URLSearchParams(search).get("startupLanguage");
  if (requested?.toLowerCase().startsWith("zh")) return "zh-CN";
  if (requested?.toLowerCase().startsWith("en")) return "en";
  return preferredLanguages.some((language) => language.toLowerCase().startsWith("zh"))
    ? "zh-CN"
    : "en";
}
