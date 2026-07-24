/* Hallmark · pre-emit critique: P5 H5 E4 S5 R5 V4 */

import {
  CloudDownload,
  FileText,
  FolderPlus,
  LoaderCircle,
  RefreshCw
} from "lucide-react";
import { t, type AppLanguage } from "@markra/shared";
import { Button } from "@markra/ui";
import type { PrimaryWorkspaceStatus } from "../../hooks/usePrimaryWorkspace";

export type WelcomeScreenProps = {
  error: string | null;
  formFactor: "desktop" | "mobile";
  language: AppLanguage;
  onChooseDesktopRoot: () => Promise<unknown>;
  onCreateMobileRoot: () => Promise<unknown>;
  onDeferDesktopSetup: () => Promise<unknown>;
  onOpenExternalFile: () => Promise<unknown>;
  onRestoreFromCloud?: () => Promise<unknown>;
  onRetry: () => Promise<unknown>;
  status: PrimaryWorkspaceStatus;
};

type DesktopWelcomeProps = Omit<
  WelcomeScreenProps,
  "error" | "formFactor" | "onCreateMobileRoot"
>;

function DesktopExternalActions({
  language,
  onOpenExternalFile
}: Pick<DesktopWelcomeProps, "language" | "onOpenExternalFile">) {
  const label = (key: string) => t(language, key);

  return (
    <section className="welcome-screen__external" aria-labelledby="welcome-external-title">
      <div>
        <h2 id="welcome-external-title">{label("onboarding.external.title")}</h2>
        <p>{label("onboarding.external.description")}</p>
      </div>
      <div className="welcome-screen__external-actions">
        <Button
          className="welcome-screen__action"
          variant="secondary"
          onClick={onOpenExternalFile}
        >
          <FileText aria-hidden="true" size={17} strokeWidth={1.7} />
          {label("onboarding.action.openFile")}
        </Button>
      </div>
    </section>
  );
}

function DesktopWelcome({
  language,
  onChooseDesktopRoot,
  onDeferDesktopSetup,
  onOpenExternalFile,
  onRestoreFromCloud,
  onRetry,
  status
}: DesktopWelcomeProps) {
  const label = (key: string) => t(language, key);
  const brandName = label("onboarding.brand.name");
  const loading = status === "loading";
  const recovery = status === "recovery";
  const error = status === "error";
  const deferred = status === "deferred";

  return (
    <main className="welcome-screen welcome-screen--desktop" data-form-factor="desktop">
      <aside className="welcome-screen__identity" aria-label={brandName}>
        <p className="welcome-screen__wordmark">{brandName}</p>
        <p className="welcome-screen__slogan">{label("onboarding.brand.slogan")}</p>
        <p className="welcome-screen__promise">{label("onboarding.brand.promise")}</p>
      </aside>

      <section
        className="welcome-screen__desktop-task"
        aria-label={loading ? label("onboarding.loading") : undefined}
        aria-labelledby={loading ? undefined : "welcome-title"}
      >
        <div className="welcome-screen__desktop-content">
          {loading ? (
            <div className="welcome-screen__status" role="status">
              <LoaderCircle aria-hidden="true" className="welcome-screen__spinner" size={20} />
              <span>{label("onboarding.loading")}</span>
            </div>
          ) : (
            <>
              <div className="welcome-screen__task-copy">
                <h1 id="welcome-title">
                  {recovery
                    ? label("onboarding.recovery.title")
                    : error
                      ? label("onboarding.error.title")
                      : deferred
                        ? label("onboarding.deferred.title")
                        : label("onboarding.desktop.title")}
                </h1>
                <p>
                  {recovery
                    ? label("onboarding.recovery.description")
                    : error
                      ? label("onboarding.error.description")
                      : deferred
                        ? label("onboarding.deferred.description")
                        : label("onboarding.desktop.description")}
                </p>
              </div>

              <div className="welcome-screen__primary-actions">
                {recovery || error ? (
                  <Button
                    className="welcome-screen__action"
                    variant="primary"
                    onClick={onRetry}
                  >
                    <RefreshCw aria-hidden="true" size={17} strokeWidth={1.8} />
                    {label("onboarding.action.retry")}
                  </Button>
                ) : null}
                <Button
                  className="welcome-screen__action"
                  variant={recovery || error ? "secondary" : "primary"}
                  onClick={onChooseDesktopRoot}
                >
                  <FolderPlus aria-hidden="true" size={17} strokeWidth={1.7} />
                  {recovery || error
                    ? label("onboarding.action.chooseOtherDirectory")
                    : label("onboarding.action.chooseLocalDirectory")}
                </Button>
                {onRestoreFromCloud ? (
                  <Button
                    className="welcome-screen__action"
                    variant="secondary"
                    onClick={onRestoreFromCloud}
                  >
                    <CloudDownload aria-hidden="true" size={17} strokeWidth={1.7} />
                    {label("onboarding.action.restoreFromCloud")}
                  </Button>
                ) : null}
                {!recovery && !error && !deferred ? (
                  <Button
                    className="welcome-screen__action welcome-screen__defer"
                    style={{ color: "var(--text-primary)" }}
                    variant="ghost"
                    onClick={onDeferDesktopSetup}
                  >
                    {label("onboarding.action.defer")}
                  </Button>
                ) : null}
              </div>

              <DesktopExternalActions
                language={language}
                onOpenExternalFile={onOpenExternalFile}
              />
            </>
          )}
        </div>
      </section>
    </main>
  );
}

type MobileWelcomeProps = Pick<
  WelcomeScreenProps,
  "error" | "language" | "onCreateMobileRoot" | "onRetry" | "status"
>;

function MobileWelcome({ error, language, onCreateMobileRoot, onRetry, status }: MobileWelcomeProps) {
  const label = (key: string) => t(language, key);
  const brandName = label("onboarding.brand.name");
  const loading = status === "loading";
  const failure = status === "error" || status === "recovery";

  return (
    <main className="welcome-screen welcome-screen--mobile" data-form-factor="mobile">
      <section className="welcome-screen__mobile-copy" aria-labelledby="welcome-title">
        <p className="welcome-screen__slogan">{label("onboarding.brand.slogan")}</p>
        <p className="welcome-screen__wordmark">{brandName}</p>
        <h1 id="welcome-title">
          {failure ? label("onboarding.error.title") : label("onboarding.mobile.title")}
        </h1>
        <p>
          {failure
            ? label("onboarding.error.mobileDescription")
            : label("onboarding.mobile.description")}
        </p>
        {failure && error ? <p role="alert">{error}</p> : null}
      </section>

      {loading ? (
        <div className="welcome-screen__status" role="status">
          <LoaderCircle aria-hidden="true" className="welcome-screen__spinner" size={20} />
          <span>{label("onboarding.loading")}</span>
        </div>
      ) : (
        <div className="welcome-screen__mobile-action">
          <Button
            className="welcome-screen__mobile-button"
            variant="primary"
            onClick={failure ? onRetry : onCreateMobileRoot}
          >
            {failure ? (
              <RefreshCw aria-hidden="true" size={18} strokeWidth={1.8} />
            ) : null}
            {failure
              ? label("onboarding.action.retry")
              : label("onboarding.action.createMobile")}
          </Button>
        </div>
      )}
    </main>
  );
}

export function WelcomeScreen(props: WelcomeScreenProps) {
  if (props.status === "ready") return null;

  if (props.formFactor === "mobile") {
    return (
      <MobileWelcome
        error={props.error}
        language={props.language}
        status={props.status}
        onCreateMobileRoot={props.onCreateMobileRoot}
        onRetry={props.onRetry}
      />
    );
  }

  return (
    <DesktopWelcome
      language={props.language}
      status={props.status}
      onChooseDesktopRoot={props.onChooseDesktopRoot}
      onDeferDesktopSetup={props.onDeferDesktopSetup}
      onOpenExternalFile={props.onOpenExternalFile}
      onRestoreFromCloud={props.onRestoreFromCloud}
      onRetry={props.onRetry}
    />
  );
}
