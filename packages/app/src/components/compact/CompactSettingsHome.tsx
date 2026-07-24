import {
  ArrowLeft,
  Cable,
  ChevronRight,
  Cloud,
  HardDrive,
  Languages,
  Palette,
  Type,
  type LucideIcon
} from "lucide-react";
import { t, type I18nKey } from "@markra/shared";
import type {
  CompactNavigation,
  CompactSettingsCategory
} from "../../hooks/useCompactNavigation";
import { compactSettingsCategories } from "../../lib/compact-settings";
import type { CompactAppController } from "./types";

type CompactSettingsHomeProps = {
  controller: CompactAppController;
  navigation: CompactNavigation;
};

type CompactSettingsGroup = {
  categories: CompactSettingsCategory[];
  labelKey: I18nKey;
};

const targetClass = "min-h-11 min-w-11";

const categoryDetails: Record<CompactSettingsCategory, {
  icon: LucideIcon;
  labelKey: I18nKey;
}> = {
  appearance: { icon: Palette, labelKey: "compact.settings.appearance" },
  editor: { icon: Type, labelKey: "compact.settings.editor" },
  general: { icon: Languages, labelKey: "compact.settings.general" },
  mcp: { icon: Cable, labelKey: "compact.settings.mcp" },
  storage: { icon: HardDrive, labelKey: "compact.settings.storage" },
  sync: { icon: Cloud, labelKey: "compact.settings.sync" }
};

const settingsGroups: CompactSettingsGroup[] = [
  { categories: ["general", "mcp"], labelKey: "compact.settings.group.app" },
  { categories: ["storage", "sync"], labelKey: "compact.settings.group.workspace" },
  {
    categories: ["appearance", "editor"],
    labelKey: "compact.settings.group.editing"
  }
];

export function CompactSettingsHome({
  controller,
  navigation
}: CompactSettingsHomeProps) {
  const language = controller.language ?? "en";
  const visibleCategories = compactSettingsCategories(controller.capabilities);
  const visibleCategorySet = new Set(visibleCategories);
  const back = () => navigation.pop().catch(() => {});
  const openCategory = (category: CompactSettingsCategory) => {
    if (category === "sync") return navigation.push({ kind: "sync-status" });
    return navigation.push({ kind: "settings-detail", category });
  };

  return (
    <section
      aria-label={t(language, "compact.settings.title")}
      className="absolute inset-0 flex h-full min-h-0 min-w-0 flex-col overflow-x-hidden bg-(--bg-primary)"
      data-testid="compact-settings-home"
    >
      <header className="flex shrink-0 items-center gap-2 border-b border-(--border-subtle) px-2 pt-[var(--compact-safe-area-top)]">
        <button
          aria-label={t(language, "compact.navigation.back")}
          className={`${targetClass} inline-flex items-center justify-center rounded-lg`}
          type="button"
          onClick={back}
        >
          <ArrowLeft aria-hidden="true" size={20} />
        </button>
        <h1 className="m-0 min-w-0 flex-1 truncate text-base font-semibold">
          {t(language, "compact.settings.title")}
        </h1>
      </header>
      <div
        className="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden px-4 py-5 pb-[calc(1.25rem+var(--compact-bottom-inset))]"
        data-compact-scroll="vertical"
        data-compact-settings-scroll
      >
        <div className="mx-auto grid w-full min-w-0 max-w-lg gap-6">
          {settingsGroups.map((group) => {
            const categories = group.categories.filter((category) => visibleCategorySet.has(category));
            if (categories.length === 0) return null;

            return (
              <section className="min-w-0" key={group.labelKey}>
                <h2 className="m-0 mb-2 text-xs font-semibold text-(--text-secondary)">
                  {t(language, group.labelKey)}
                </h2>
                <div className="min-w-0 overflow-hidden rounded-xl border border-(--border-subtle) bg-(--bg-secondary)">
                  {categories.map((category) => {
                    const details = categoryDetails[category];
                    const Icon = details.icon;
                    return (
                      <button
                        className={`${targetClass} flex w-full min-w-0 items-center gap-3 border-0 border-b border-(--border-subtle) bg-transparent px-3 py-2 text-left last:border-b-0`}
                        data-compact-settings-category={category}
                        key={category}
                        type="button"
                        onClick={() => openCategory(category)}
                      >
                        <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-(--bg-primary) text-(--text-secondary)">
                          <Icon aria-hidden="true" size={18} />
                        </span>
                        <span className="min-w-0 flex-1 truncate text-sm font-medium text-(--text-heading)">
                          {t(language, details.labelKey)}
                        </span>
                        <ChevronRight aria-hidden="true" className="shrink-0 text-(--text-secondary)" size={18} />
                      </button>
                    );
                  })}
                </div>
              </section>
            );
          })}
        </div>
      </div>
    </section>
  );
}
