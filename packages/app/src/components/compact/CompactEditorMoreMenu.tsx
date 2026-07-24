import { FileClock, RefreshCw, Save, Search, Settings } from "lucide-react";
import { t, type AppLanguage } from "@markra/shared";

type CompactEditorMoreMenuProps = {
  documentAvailable: boolean;
  language: AppLanguage;
  applicationSyncAvailable: boolean;
  syncConfigured: boolean;
  onClose: () => unknown;
  onConfigureSync: () => unknown;
  onFind: () => unknown;
  onHistory: () => unknown;
  onSave: () => unknown;
  onSettings: () => unknown;
  onSyncNow: () => unknown;
};

const menuItemClass = "flex min-h-11 w-full items-center gap-3 rounded-lg px-3 text-left text-sm disabled:opacity-45";

export function CompactEditorMoreMenu({
  documentAvailable,
  language,
  applicationSyncAvailable,
  syncConfigured,
  onClose,
  onConfigureSync,
  onFind,
  onHistory,
  onSave,
  onSettings,
  onSyncNow
}: CompactEditorMoreMenuProps) {
  const run = (action: () => unknown) => {
    onClose();
    action();
  };

  return (
    <div
      aria-label={t(language, "compact.editor.more")}
      className="absolute top-full right-2 z-30 min-w-48 rounded-xl border border-(--border-default) bg-(--bg-primary) p-1.5 shadow-xl"
      role="menu"
    >
      <button
        className={menuItemClass}
        disabled={!documentAvailable}
        role="menuitem"
        type="button"
        onClick={() => run(onSave)}
      >
        <Save aria-hidden="true" size={18} />
        {t(language, "compact.editor.save")}
      </button>
      <button
        className={menuItemClass}
        disabled={!documentAvailable}
        role="menuitem"
        type="button"
        onClick={() => run(onFind)}
      >
        <Search aria-hidden="true" size={18} />
        {t(language, "compact.editor.find")}
      </button>
      <button
        className={menuItemClass}
        disabled={!documentAvailable}
        role="menuitem"
        type="button"
        onClick={() => run(onHistory)}
      >
        <FileClock aria-hidden="true" size={18} />
        {t(language, "compact.editor.history")}
      </button>
      {applicationSyncAvailable ? (
        <button
          className={menuItemClass}
          role="menuitem"
          type="button"
          onClick={() => run(syncConfigured ? onSyncNow : onConfigureSync)}
        >
          <RefreshCw aria-hidden="true" size={18} />
          {t(language, syncConfigured ? "compact.sync.now" : "compact.sync.configure")}
        </button>
      ) : null}
      <button
        className={menuItemClass}
        role="menuitem"
        type="button"
        onClick={() => run(onSettings)}
      >
        <Settings aria-hidden="true" size={18} />
        {t(language, "compact.editor.settings")}
      </button>
    </div>
  );
}
