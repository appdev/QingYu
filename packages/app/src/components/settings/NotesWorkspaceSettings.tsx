import { FolderOpen, RotateCcw } from "lucide-react";
import type { PrimaryWorkspaceStatus } from "../../hooks/usePrimaryWorkspace";
import {
  SettingsButton,
  SettingsCallout,
  SettingsRow,
  SettingsSection
} from "./SettingsControls";
import type { SettingsTranslate } from "./translate";

export type NotesWorkspaceSettingsProps = {
  onChoose: () => unknown;
  onResetOnboarding: () => unknown;
  root: string | null;
  status: PrimaryWorkspaceStatus;
  translate: SettingsTranslate;
};

export function NotesWorkspaceSettings({
  onChoose,
  onResetOnboarding,
  root,
  status,
  translate
}: NotesWorkspaceSettingsProps) {
  const unavailable = status === "recovery" || status === "error";
  const busy = status === "loading";
  const chooseLabel = translate("settings.notesWorkspace.switchDirectory");

  return (
    <>
      <SettingsSection
        label={translate("settings.notesWorkspace.section")}
        intro={unavailable ? (
          <SettingsCallout
            description={translate("settings.notesWorkspace.unavailableDescription")}
            icon={FolderOpen}
            title={translate("settings.notesWorkspace.unavailable")}
          />
        ) : undefined}
      >
        <SettingsRow
          title={translate("settings.notesWorkspace.path")}
          description={translate("settings.notesWorkspace.pathDescription")}
          action={
            <span className="max-w-80 break-all text-right text-[12px] leading-5 font-[560] text-(--text-heading)">
              {root ?? translate("settings.notesWorkspace.notConfigured")}
            </span>
          }
        />
        <SettingsRow
          title={translate("settings.notesWorkspace.changeTitle")}
          description={translate("settings.notesWorkspace.noMoveDescription")}
          action={
            <SettingsButton disabled={busy} label={chooseLabel} onClick={onChoose}>
              <FolderOpen aria-hidden="true" size={13} />
              {chooseLabel}
            </SettingsButton>
          }
        />
      </SettingsSection>

      <SettingsSection label={translate("settings.notesWorkspace.onboardingSection")}>
        <SettingsRow
          title={translate("settings.notesWorkspace.resetTitle")}
          description={translate("settings.notesWorkspace.resetDescription")}
          action={
            <SettingsButton
              disabled={busy}
              label={translate("settings.notesWorkspace.reset")}
              onClick={onResetOnboarding}
            >
              <RotateCcw aria-hidden="true" size={13} />
              {translate("settings.notesWorkspace.reset")}
            </SettingsButton>
          }
        />
      </SettingsSection>
    </>
  );
}
