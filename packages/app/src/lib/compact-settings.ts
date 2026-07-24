import type { CompactSettingsCategory } from "../hooks/useCompactNavigation";

export type CompactSettingsCapabilities = {
  applicationSync: boolean;
  mcpPolicy: boolean;
};

export function compactSettingsCategories({
  applicationSync,
  mcpPolicy
}: CompactSettingsCapabilities): CompactSettingsCategory[] {
  const categories: CompactSettingsCategory[] = ["general"];
  if (mcpPolicy) categories.push("mcp");
  categories.push("storage");
  if (applicationSync) categories.push("sync");
  categories.push("appearance", "editor");
  return categories;
}
