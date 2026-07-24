import { useCallback, useEffect, useState } from "react";
import { t, type I18nKey } from "@markra/shared";
import { Button } from "@markra/ui";
import { getAppRuntime, type AppMcpRuntime } from "../../runtime";
import {
  isMcpRevisionConflict,
  type McpAuditEntry,
  type McpConfig,
  type McpPermissions,
  type McpRecycleBinRetentionDays,
  type McpServerHealth
} from "../../lib/mcp";
import { useMcpSettings } from "../../hooks/useMcpSettings";
import { McpClientConfiguration } from "./McpClientConfiguration";
import { SettingsRow, SettingsSection, SettingsSelect } from "./SettingsControls";

type Translate = (key: I18nKey) => string;
type ConfirmAction = (message: string) => boolean | Promise<boolean>;

const auditPageSize = 100;
const englishTranslate: Translate = (key) => t("en", key);

const permissionLabels: Array<[keyof McpPermissions, I18nKey]> = [
  ["documentsRead", "settings.mcp.permission.documentsRead"],
  ["documentsWrite", "settings.mcp.permission.documentsWrite"],
  ["documentsMove", "settings.mcp.permission.documentsMove"],
  ["documentsDelete", "settings.mcp.permission.documentsDelete"],
  ["settingsRead", "settings.mcp.permission.settingsRead"],
  ["settingsWrite", "settings.mcp.permission.settingsWrite"],
  ["syncRead", "settings.mcp.permission.syncRead"],
  ["syncWrite", "settings.mcp.permission.syncWrite"],
  ["syncCredentialsWrite", "settings.mcp.permission.syncCredentialsWrite"],
  ["syncRun", "settings.mcp.permission.syncRun"]
];

export function McpSettings({
  compact = false,
  confirmAction,
  runtime = getAppRuntime().mcp,
  translate = englishTranslate,
  writeClipboard
}: {
  compact?: boolean;
  confirmAction?: ConfirmAction;
  runtime?: AppMcpRuntime;
  translate?: Translate;
  writeClipboard?: (text: string) => Promise<unknown>;
}) {
  const state = useMcpSettings(runtime);
  const [actionError, setActionError] = useState<string | null>(null);
  const [auditEntries, setAuditEntries] = useState<McpAuditEntry[]>([]);
  const [auditOffset, setAuditOffset] = useState(0);
  const snapshot = state.snapshot;
  const compactControlClass = compact ? "min-h-11 min-w-11" : undefined;

  const loadAudit = useCallback(async (offset: number) => {
    if (!runtime.localServiceAvailable) return;
    try {
      setAuditEntries(await runtime.listAuditEntries(offset, auditPageSize));
      setActionError(null);
    } catch {
      setActionError("action");
    }
  }, [runtime]);

  useEffect(() => {
    loadAudit(auditOffset).catch(() => {});
  }, [auditOffset, loadAudit]);

  if (!runtime.policyAvailable) return null;
  if (state.loading && !snapshot) return <p>{translate("settings.mcp.loading")}</p>;
  if (!snapshot) return <p role="alert">{translate("settings.mcp.unavailable")}</p>;

  const documentWorkspaceAvailable = snapshot.workspace?.available === true;
  const update = (change: (config: McpConfig) => McpConfig) => {
    state.updateConfig(change(snapshot.config)).catch(() => {});
  };
  const ask = async (key: I18nKey) => {
    const message = translate(key);
    if (confirmAction) return Promise.resolve(confirmAction(message));
    return globalThis.confirm?.(message) ?? false;
  };
  const runAction = async (action: () => Promise<unknown>) => {
    try {
      await action();
      setActionError(null);
    } catch {
      setActionError("action");
    }
  };
  const visibleError = actionError ?? state.error;

  return (
    <div
      className={`mcp-settings min-w-0 ${compact ? "[&_.settings-row]:grid-cols-1 [&_.settings-row]:gap-2 [&_.settings-row>div:last-child]:w-full [&_.settings-row>div:last-child]:justify-start" : ""}`}
      data-mcp-presentation={compact ? "compact" : "desktop"}
    >
      <p className="mb-6 text-[13px] leading-5 text-(--text-secondary)">
        {translate("settings.mcp.summary")}
      </p>
      {visibleError ? <p role="alert">{settingsError(visibleError, translate)}</p> : null}
      <SettingsSection label={translate("settings.mcp.section.service")}>
        <SettingsRow
          title={translate(snapshot.config.enabled ? "settings.mcp.disabled" : "settings.mcp.enabled")}
          description={translate(snapshot.config.enabled ? "settings.mcp.enabledDescription" : "settings.mcp.disabledDescription")}
          action={(
            <Button className={compactControlClass} onClick={() => update((config) => ({ ...config, enabled: !config.enabled }))}>
              {translate(snapshot.config.enabled ? "settings.mcp.disabled" : "settings.mcp.enabled")}
            </Button>
          )}
        />
        {runtime.localServiceAvailable ? (
          <SettingsRow title={translate("settings.mcp.endpoint")} action={<code>{snapshot.endpoint ?? "—"}</code>} />
        ) : (
          <p className="py-3 text-[12px] leading-5 text-(--text-secondary)" role="note">
            {translate("settings.mcp.service.policyOnly")}
          </p>
        )}
        {runtime.localServiceAvailable ? (
          <>
            <SettingsRow
              title={translate("settings.mcp.workspace.current")}
              description={translate("settings.mcp.workspace.description")}
              action={<span>{snapshot.workspace?.displayName ?? translate("settings.mcp.workspace.none")}</span>}
            />
            {!documentWorkspaceAvailable ? (
              <p className="py-3 text-[12px] leading-5 text-(--text-secondary)" role="note">
                {translate("settings.mcp.workspace.documentToolsUnavailable")}
              </p>
            ) : null}
            <SettingsRow
              title={translate("settings.mcp.health")}
              action={<HealthStatus health={snapshot.health} translate={translate} />}
            />
          </>
        ) : null}
      </SettingsSection>

      {snapshot.config.enabled && runtime.localServiceAvailable ? (
        snapshot.clientCommand ? (
          <McpClientConfiguration
            command={snapshot.clientCommand}
            translate={translate}
            writeClipboard={writeClipboard}
          />
        ) : (
          <SettingsSection label={translate("settings.mcp.section.clientConnection")}>
            <p className="py-3 text-[12px] leading-5 text-(--text-secondary)" role="note">
              {translate("settings.mcp.client.unavailable")}
            </p>
          </SettingsSection>
        )
      ) : null}

      <SettingsSection label={translate("settings.mcp.section.permissions")}>
        {permissionLabels.map(([key, labelKey]) => {
          const label = translate(labelKey);
          return (
            <SettingsRow
              key={key}
              title={label}
              action={(
                <input
                  aria-label={label}
                  checked={snapshot.config.permissions[key]}
                  className={compactControlClass}
                  type="checkbox"
                  onChange={() => update((config) => ({
                    ...config,
                    permissions: { ...config.permissions, [key]: !config.permissions[key] }
                  }))}
                />
              )}
            />
          );
        })}
      </SettingsSection>

      <SettingsSection label={translate("settings.mcp.section.policy")}>
        <PolicySelect
          label={translate("settings.mcp.policy.confirmation")}
          compact={compact}
          value={snapshot.config.confirmation}
          options={[
            ["never", "settings.mcp.policy.confirmation.never"],
            ["destructive-only", "settings.mcp.policy.confirmation.destructiveOnly"],
            ["all-writes", "settings.mcp.policy.confirmation.allWrites"]
          ]}
          translate={translate}
          onChange={(value) => update((config) => ({ ...config, confirmation: value as McpConfig["confirmation"] }))}
        />
        <PolicySelect
          label={translate("settings.mcp.policy.dryRun")}
          compact={compact}
          value={snapshot.config.dryRun}
          options={[
            ["never", "settings.mcp.policy.dryRun.never"],
            ["high-risk", "settings.mcp.policy.dryRun.highRisk"],
            ["all-writes", "settings.mcp.policy.dryRun.allWrites"]
          ]}
          translate={translate}
          onChange={(value) => update((config) => ({ ...config, dryRun: value as McpConfig["dryRun"] }))}
        />
        <PolicySelect
          label={translate("settings.mcp.policy.deletion")}
          compact={compact}
          value={snapshot.config.deletion}
          options={[
            ["system-trash", "settings.mcp.policy.deletion.systemTrash"],
            ["qing-yu-recycle-bin", "settings.mcp.policy.deletion.qingYuRecycleBin"],
            ["permanent", "settings.mcp.policy.deletion.permanent"]
          ]}
          translate={translate}
          onChange={(value) => update((config) => ({ ...config, deletion: value as McpConfig["deletion"] }))}
        />
        {snapshot.config.deletion === "qing-yu-recycle-bin" ? (
          <SettingsRow
            title={translate("settings.mcp.policy.recycleBinCleanup")}
            action={(
              <SettingsSelect
                compact={compact}
                label={translate("settings.mcp.policy.recycleBinCleanup")}
                value={String(snapshot.config.recycleBinRetentionDays)}
                options={([
                  [0, "settings.mcp.policy.recycleBinCleanup.never"],
                  [7, "settings.mcp.policy.recycleBinCleanup.days7"],
                  [30, "settings.mcp.policy.recycleBinCleanup.days30"],
                  [90, "settings.mcp.policy.recycleBinCleanup.days90"]
                ] satisfies Array<[McpRecycleBinRetentionDays, I18nKey]>).map(([value, labelKey]) => ({
                  label: translate(labelKey),
                  value: String(value)
                }))}
                onChange={(value) => update((config) => ({
                  ...config,
                  recycleBinRetentionDays: Number(value) as McpRecycleBinRetentionDays
                }))}
              />
            )}
          />
        ) : null}
        <PolicySelect
          label={translate("settings.mcp.policy.syncAfterWrite")}
          compact={compact}
          value={snapshot.config.syncAfterWrite}
          options={[
            ["follow-workspace", "settings.mcp.policy.syncAfterWrite.followWorkspace"],
            ["always", "settings.mcp.policy.syncAfterWrite.always"],
            ["never", "settings.mcp.policy.syncAfterWrite.never"]
          ]}
          translate={translate}
          onChange={(value) => update((config) => ({ ...config, syncAfterWrite: value as McpConfig["syncAfterWrite"] }))}
        />
        <PolicySelect
          label={translate("settings.mcp.policy.syncExecution")}
          compact={compact}
          value={snapshot.config.syncExecution}
          options={[
            ["background", "settings.mcp.policy.syncExecution.background"],
            ["wait", "settings.mcp.policy.syncExecution.wait"]
          ]}
          translate={translate}
          onChange={(value) => update((config) => ({ ...config, syncExecution: value as McpConfig["syncExecution"] }))}
        />
      </SettingsSection>

      <SettingsSection label={translate(
        runtime.localServiceAvailable ? "settings.mcp.section.audit" : "settings.mcp.section.auditPolicy"
      )}>
        <SettingsRow
          title={translate("settings.mcp.audit.enabled")}
          action={(
            <input
              aria-label={translate("settings.mcp.audit.enabled")}
              checked={snapshot.config.audit.enabled}
              className={compactControlClass}
              type="checkbox"
              onChange={() => update((config) => ({
                ...config,
                audit: { ...config.audit, enabled: !config.audit.enabled }
              }))}
            />
          )}
        />
        <SettingsRow
          title={translate("settings.mcp.audit.retentionDays")}
          action={<NumberInput compact={compact} label={translate("settings.mcp.audit.retentionDays")} value={snapshot.config.audit.retentionDays} onChange={(value) => update((config) => ({ ...config, audit: { ...config.audit, retentionDays: value } }))} />}
        />
        <SettingsRow
          title={translate("settings.mcp.audit.maxEntries")}
          action={<NumberInput compact={compact} label={translate("settings.mcp.audit.maxEntries")} value={snapshot.config.audit.maxEntries} onChange={(value) => update((config) => ({ ...config, audit: { ...config.audit, maxEntries: value } }))} />}
        />
        {runtime.localServiceAvailable ? (
          <>
            <AuditTable entries={auditEntries} translate={translate} />
            <div className="flex items-center justify-end gap-2 py-4">
              <Button className={compactControlClass} disabled={auditOffset === 0} onClick={() => setAuditOffset(Math.max(0, auditOffset - auditPageSize))}>
                {translate("settings.mcp.audit.previous")}
              </Button>
              <Button className={compactControlClass} disabled={auditEntries.length < auditPageSize} onClick={() => setAuditOffset(auditOffset + auditPageSize)}>
                {translate("settings.mcp.audit.next")}
              </Button>
              <Button className={compactControlClass} onClick={() => runAction(async () => {
                if (!await ask("settings.mcp.confirm.clearAudit")) return;
                await runtime.clearAuditEntries();
                setAuditOffset(0);
                setAuditEntries([]);
              })}>{translate("settings.mcp.audit.clear")}</Button>
            </div>
          </>
        ) : null}
      </SettingsSection>
    </div>
  );
}

function HealthStatus({ health, translate }: { health: McpServerHealth; translate: Translate }) {
  return (
    <span className="inline-flex items-center gap-2">
      <span>{translate(`settings.mcp.health.${health.state}` as I18nKey)}</span>
      {health.errorCode ? <code>{health.errorCode}</code> : null}
    </span>
  );
}

function AuditTable({ entries, translate }: { entries: McpAuditEntry[]; translate: Translate }) {
  if (entries.length === 0) {
    return <p className="py-4 text-[12px] text-(--text-secondary)">{translate("settings.mcp.audit.empty")}</p>;
  }
  const headings = [
    "settings.mcp.audit.time",
    "settings.mcp.audit.tool",
    "settings.mcp.audit.workspace",
    "settings.mcp.audit.target",
    "settings.mcp.audit.outcome",
    "settings.mcp.audit.revisions",
    "settings.mcp.audit.runId",
    "settings.mcp.audit.duration"
  ] as I18nKey[];
  return (
    <div className="overflow-x-auto py-4">
      <table className="w-full border-collapse text-left text-[12px]">
        <thead>
          <tr>{headings.map((heading) => <th className="border-b border-(--border-default) px-2 py-2" key={heading}>{translate(heading)}</th>)}</tr>
        </thead>
        <tbody>
          {entries.map((entry) => (
            <tr key={entry.requestId}>
              <td className="px-2 py-2">{new Date(entry.timestampMs).toLocaleString()}</td>
              <td className="px-2 py-2"><code>{entry.tool}</code></td>
              <td className="px-2 py-2">{entry.workspaceDisplayName ?? "—"}</td>
              <td className="px-2 py-2"><code>{entry.logicalTarget ?? "—"}</code></td>
              <td className="px-2 py-2">{translate(`settings.mcp.outcome.${entry.outcome}` as I18nKey)}{entry.errorCode ? ` · ${entry.errorCode}` : ""}</td>
              <td className="px-2 py-2"><code>{entry.revisionBefore ?? "—"} → {entry.revisionAfter ?? "—"}</code></td>
              <td className="px-2 py-2"><code>{entry.syncRunId ?? "—"}</code></td>
              <td className="px-2 py-2">{entry.durationMs} ms</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function settingsError(error: string, translate: Translate) {
  if (isMcpRevisionConflict(error)) return translate("settings.mcp.error.revisionConflict");
  if (error.includes("bind") || error.includes("start")) return translate("settings.mcp.error.start");
  if (error === "action") return translate("settings.mcp.error.action");
  return translate("settings.mcp.error.save");
}

function PolicySelect({ compact, disabled, label, onChange, options, translate, value }: {
  compact?: boolean;
  disabled?: boolean;
  label: string;
  onChange: (value: string) => unknown;
  options: Array<[string, I18nKey]>;
  translate: Translate;
  value: string;
}) {
  return (
    <SettingsRow
      title={label}
      action={(
        <select className={compact ? "min-h-11 min-w-11 w-full" : undefined} aria-label={label} disabled={disabled} value={value} onChange={(event) => onChange(event.currentTarget.value)}>
          {options.map(([option, labelKey]) => <option key={option} value={option}>{translate(labelKey)}</option>)}
        </select>
      )}
    />
  );
}

function NumberInput({ compact, disabled, label, onChange, value }: {
  compact?: boolean;
  disabled?: boolean;
  label: string;
  onChange: (value: number) => unknown;
  value: number;
}) {
  return (
    <input
      aria-label={label}
      className={compact ? "min-h-11 min-w-11 w-full" : undefined}
      disabled={disabled}
      min={1}
      type="number"
      value={value}
      onChange={(event) => onChange(Number(event.currentTarget.value))}
    />
  );
}
