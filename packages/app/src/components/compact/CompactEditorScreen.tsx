import { AlertCircle, Check, Circle, Files, LoaderCircle, MoreHorizontal } from "lucide-react";
import { useState } from "react";
import { t } from "@markra/shared";
import type { CompactNavigation, CompactOverlayPage } from "../../hooks/useCompactNavigation";
import { CompactEditorToolbar } from "./CompactEditorToolbar";
import { CompactEditorMoreMenu } from "./CompactEditorMoreMenu";
import { CompactWelcomeState } from "./CompactWelcomeState";
import type { CompactAppController, CompactSaveState } from "./types";

type CompactEditorScreenProps = {
  controller: CompactAppController;
  navigation: CompactNavigation;
};

const compactTargetClass = "min-h-11 min-w-11";

function saveStateContent(language: CompactAppController["language"], saveState: CompactSaveState) {
  if (saveState.status === "error") {
    return {
      icon: <AlertCircle aria-hidden="true" className="shrink-0" size={12} />,
      label: t(language, "compact.save.error")
    };
  }
  if (saveState.status === "saving") {
    return {
      icon: <LoaderCircle aria-hidden="true" className="shrink-0 animate-spin motion-reduce:animate-none" size={12} />,
      label: t(language, "compact.save.saving")
    };
  }
  if (saveState.status === "dirty") {
    return {
      icon: <Circle aria-hidden="true" className="shrink-0 fill-current" size={8} />,
      label: t(language, "compact.save.dirty")
    };
  }
  return {
    icon: <Check aria-hidden="true" className="shrink-0" size={12} />,
    label: t(language, "compact.save.saved")
  };
}

export function CompactEditorScreen({ controller, navigation }: CompactEditorScreenProps) {
  const [moreOpen, setMoreOpen] = useState(false);
  const language = controller.language ?? "en";
  const documentAvailable = controller.document.document.open;
  const syncConfigured = controller.workspace.syncConfigDocument?.readiness === "ready"
    && controller.workspace.syncConfigDocument.config.enabled;
  const saveContent = saveStateContent(language, controller.saveState);
  const pushAfterFlush = async (page: CompactOverlayPage) => {
    try {
      await controller.saveState.flush("navigation");
    } catch {
      // Draft persistence remains available and the persistent save error stays visible.
    }
    return navigation.push(page);
  };
  const openFiles = () => pushAfterFlush({ kind: "files" });
  const openSettings = () => pushAfterFlush({ kind: "settings" });
  const openSyncStatus = () => pushAfterFlush({ kind: "sync-status" });

  return (
    <section className="relative flex h-full min-h-0 flex-col" aria-label={t(language, "compact.editor.screen")}>
      <header className="relative z-20 grid shrink-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center border-b border-(--border-subtle) px-1 pt-[var(--compact-safe-area-top)]">
        <button
          aria-label={t(language, "compact.editor.files")}
          className={`${compactTargetClass} inline-flex items-center justify-center rounded-lg`}
          type="button"
          onClick={openFiles}
        >
          <Files aria-hidden="true" size={20} />
        </button>
        <div className="min-w-0 px-2 text-center">
          <h1 className="m-0 truncate text-sm font-semibold text-(--text-heading)">
            {controller.document.document.name}
          </h1>
          {documentAvailable ? (
            <p
              aria-live="polite"
              className={`m-0 flex min-w-0 items-center justify-center gap-1 truncate text-[11px] ${
                controller.saveState.status === "error" ? "text-(--status-error)" : "text-(--text-secondary)"
              }`}
              role="status"
            >
              {saveContent.icon}
              <span className="truncate">{saveContent.label}</span>
            </p>
          ) : null}
        </div>
        <button
          aria-expanded={moreOpen}
          aria-haspopup="menu"
          aria-label={t(language, "compact.editor.more")}
          className={`${compactTargetClass} inline-flex items-center justify-center rounded-lg`}
          type="button"
          onClick={() => setMoreOpen((open) => !open)}
        >
          <MoreHorizontal aria-hidden="true" size={21} />
        </button>
        {moreOpen ? (
          <CompactEditorMoreMenu
            documentAvailable={documentAvailable}
            language={language}
            applicationSyncAvailable={controller.capabilities.applicationSync}
            syncConfigured={syncConfigured}
            onClose={() => setMoreOpen(false)}
            onConfigureSync={openSyncStatus}
            onFind={controller.actions.openDocumentSearch}
            onHistory={controller.actions.openDocumentHistory}
            onSave={controller.actions.saveDocument}
            onSettings={openSettings}
            onSyncNow={controller.actions.runApplicationSyncNow}
          />
        ) : null}
      </header>
      {documentAvailable && controller.saveState.status === "error" ? (
        <div
          className="relative z-10 flex shrink-0 items-center gap-2 border-b border-(--status-error) bg-(--bg-secondary) px-3 py-1 text-sm text-(--status-error)"
          role="alert"
        >
          <span className="min-w-0 flex-1 whitespace-normal break-words">
            {controller.saveState.error ?? t(language, "compact.save.error")}
          </span>
          <button
            className="min-h-11 min-w-11 shrink-0 rounded-lg px-2 font-medium text-(--accent)"
            type="button"
            onClick={() => controller.saveState.retry().catch(() => {})}
          >
            {t(language, "compact.save.retry")}
          </button>
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-hidden">
        {documentAvailable ? controller.editor.host : (
          <CompactWelcomeState
            language={language}
            applicationSyncAvailable={controller.capabilities.applicationSync}
            onConfigureSync={openSyncStatus}
            onNewDocument={controller.document.createBlankDocument}
          />
        )}
      </div>
      {documentAvailable && navigation.page.kind === "editor" ? (
        <CompactEditorToolbar
          disabled={controller.editor.readOnly}
          editor={controller.editor}
          imageImport={controller.capabilities.imageImport}
          language={language}
          trueMobile={controller.capabilities.trueMobile}
        />
      ) : null}
    </section>
  );
}
