import { FilePlus2, RefreshCw } from "lucide-react";
import { useState } from "react";
import { t, type AppLanguage } from "@markra/shared";
import { CompactNameDialog, compactNameOperationErrorMessage } from "./CompactNameDialog";

type CompactWelcomeStateProps = {
  language: AppLanguage;
  applicationSyncAvailable: boolean;
  onConfigureSync: () => unknown;
  onNewDocument: (fileName: string) => Promise<boolean>;
};

const compactTargetClass = "min-h-11 min-w-11";

export function CompactWelcomeState({
  language,
  applicationSyncAvailable,
  onConfigureSync,
  onNewDocument
}: CompactWelcomeStateProps) {
  const [nameDialogOpen, setNameDialogOpen] = useState(false);

  return (
    <section
      className="flex h-full min-h-0 flex-col items-center justify-center gap-5 px-8 pb-[var(--compact-bottom-inset)] text-center"
      aria-labelledby="compact-welcome-title"
    >
      <div className="grid gap-2">
        <h2 className="text-xl font-semibold text-(--text-heading)" id="compact-welcome-title">
          {t(language, "compact.welcome.title")}
        </h2>
        <p className="m-0 max-w-sm text-sm leading-6 text-(--text-secondary)">
          {t(language, "compact.welcome.description")}
        </p>
      </div>
      <div className="grid w-full max-w-xs gap-2">
        <button
          className={`${compactTargetClass} inline-flex items-center justify-center gap-2 rounded-lg bg-(--accent) px-4 text-sm font-medium text-white`}
          type="button"
          onClick={() => setNameDialogOpen(true)}
        >
          <FilePlus2 aria-hidden="true" size={19} />
          {t(language, "compact.welcome.newDocument")}
        </button>
        {applicationSyncAvailable ? (
          <button
            className={`${compactTargetClass} inline-flex items-center justify-center gap-2 rounded-lg border border-(--border-default) px-4 text-sm font-medium`}
            type="button"
            onClick={onConfigureSync}
          >
            <RefreshCw aria-hidden="true" size={18} />
            {t(language, "compact.sync.configure")}
          </button>
        ) : (
          <p className="m-0 text-xs leading-5 text-(--text-secondary)">
            {t(language, "compact.welcome.localOnly")}
          </p>
        )}
      </div>
      {nameDialogOpen ? (
        <CompactNameDialog
          cancelLabel={t(language, "compact.files.cancel")}
          errorMessage={(operationError) => compactNameOperationErrorMessage(operationError, {
            duplicate: t(language, "compact.files.nameExists"),
            fallback: t(language, "compact.files.operationFailed"),
            invalid: t(language, "compact.files.nameInvalid")
          })}
          initialValue=""
          submitLabel={t(language, "compact.files.create")}
          title={t(language, "compact.files.newFileName")}
          onCancel={() => setNameDialogOpen(false)}
          onSubmit={async (fileName) => {
            const created = await onNewDocument(fileName);
            if (!created) throw new Error("Document creation failed");
            setNameDialogOpen(false);
          }}
        />
      ) : null}
    </section>
  );
}
