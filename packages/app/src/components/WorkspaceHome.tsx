import {
  FilePlus2,
  FileText,
  Files,
  FolderSync,
  RefreshCw,
  Search,
  Settings,
  type LucideIcon
} from "lucide-react";
import { t, type AppLanguage } from "@markra/shared";

export type WorkspaceHomeProps = {
  language: AppLanguage;
  presentation: "desktop" | "compact";
  actions: {
    createDocument: () => unknown;
    openDocument?: () => unknown;
    quickOpen?: () => unknown;
    showFiles?: () => unknown;
    openSettings?: () => unknown;
    configureSync?: () => unknown;
    switchWorkspace?: () => unknown;
  };
  shortcuts?: Partial<Record<
    "createDocument" | "quickOpen" | "openSettings" | "showFiles",
    string
  >>;
};

type WorkspaceHomeAction = {
  callback: (() => unknown) | undefined;
  icon: LucideIcon;
  key: keyof WorkspaceHomeProps["actions"];
  labelKey: string;
  shortcutKey?: keyof NonNullable<WorkspaceHomeProps["shortcuts"]>;
};

export function WorkspaceHome({
  actions,
  language,
  presentation,
  shortcuts
}: WorkspaceHomeProps) {
  const actionItems: WorkspaceHomeAction[] = [
    {
      callback: actions.createDocument,
      icon: FilePlus2,
      key: "createDocument",
      labelKey: "workspaceHome.createDocument",
      shortcutKey: "createDocument"
    },
    {
      callback: actions.openDocument,
      icon: FileText,
      key: "openDocument",
      labelKey: "workspaceHome.openDocument"
    },
    {
      callback: actions.quickOpen,
      icon: Search,
      key: "quickOpen",
      labelKey: "workspaceHome.quickOpen",
      shortcutKey: "quickOpen"
    },
    {
      callback: actions.showFiles,
      icon: Files,
      key: "showFiles",
      labelKey: "workspaceHome.showFiles",
      shortcutKey: "showFiles"
    },
    {
      callback: actions.openSettings,
      icon: Settings,
      key: "openSettings",
      labelKey: "workspaceHome.openSettings",
      shortcutKey: "openSettings"
    },
    {
      callback: actions.configureSync,
      icon: RefreshCw,
      key: "configureSync",
      labelKey: "workspaceHome.configureSync"
    },
    {
      callback: actions.switchWorkspace,
      icon: FolderSync,
      key: "switchWorkspace",
      labelKey: "workspaceHome.switchWorkspace"
    }
  ];
  const compact = presentation === "compact";

  return (
    <section
      className={`flex h-full min-h-0 w-full overflow-y-auto bg-(--bg-primary) text-(--text-heading) ${
        compact ? "items-start px-5 py-8" : "items-center px-8 py-14 sm:px-12"
      }`}
      data-presentation={presentation}
      aria-labelledby="workspace-home-title"
    >
      <div className={`mx-auto grid w-full ${compact ? "max-w-sm gap-7" : "max-w-lg gap-10"}`}>
        <header className={compact ? "grid gap-3" : "grid gap-4"}>
          <div
            className="flex items-center gap-2 text-[11px] leading-4 font-[650] tracking-[0.16em] text-(--text-secondary) uppercase"
            aria-hidden="true"
          >
            <span className="size-1.5 rounded-full bg-(--accent) opacity-65" />
            QingYu
          </div>
          <div className="grid gap-2">
            <h1
              className={`m-0 font-[680] tracking-[-0.02em] text-(--text-heading) ${
                compact ? "text-[24px] leading-8" : "text-[28px] leading-9"
              }`}
              id="workspace-home-title"
            >
              {t(language, "workspaceHome.title")}
            </h1>
            <p className="m-0 max-w-md text-[13px] leading-6 font-[450] text-(--text-secondary)">
              {t(language, "workspaceHome.description")}
            </p>
          </div>
        </header>

        <div className="grid gap-1.5">
          {actionItems.map(({ callback, icon: Icon, key, labelKey, shortcutKey }) => {
            if (!callback) return null;

            const shortcut = shortcutKey ? shortcuts?.[shortcutKey] : undefined;

            return (
              <button
                className={`group flex w-full cursor-pointer items-center gap-3 rounded-lg border border-transparent bg-transparent px-3 text-left text-[13px] leading-5 font-[560] text-(--text-heading) transition-[background-color,border-color,color] duration-150 ease-out hover:border-(--border-default) hover:bg-[color-mix(in_srgb,var(--accent)_6%,var(--bg-primary))] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) ${
                  compact ? "min-h-11 min-w-11" : "min-h-10"
                }`}
                key={key}
                type="button"
                onClick={() => {
                  callback();
                }}
              >
                <span className="flex size-8 shrink-0 items-center justify-center rounded-md border border-(--border-default) text-(--text-secondary) transition-colors duration-150 group-hover:text-(--accent)">
                  <Icon aria-hidden="true" size={17} strokeWidth={1.7} />
                </span>
                <span className="min-w-0 flex-1">{t(language, labelKey)}</span>
                {shortcut ? (
                  <kbd
                    className="shrink-0 rounded border border-(--border-default) px-1.5 py-0.5 font-sans text-[10px] leading-4 font-[560] text-(--text-secondary)"
                    aria-hidden="true"
                  >
                    {shortcut}
                  </kbd>
                ) : null}
              </button>
            );
          })}
        </div>
      </div>
    </section>
  );
}
