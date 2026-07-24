export type McpConfirmationPolicy = "never" | "destructive-only" | "all-writes";
export type McpDryRunPolicy = "never" | "high-risk" | "all-writes";
export type McpDeletionPolicy = "system-trash" | "qing-yu-recycle-bin" | "permanent";
export type McpRecycleBinRetentionDays = 0 | 7 | 30 | 90;
export type McpSyncAfterWritePolicy = "follow-workspace" | "always" | "never";
export type McpSyncExecutionPolicy = "background" | "wait";

export type McpPermissions = {
  documentsRead: boolean;
  documentsWrite: boolean;
  documentsMove: boolean;
  documentsDelete: boolean;
  settingsRead: boolean;
  settingsWrite: boolean;
  syncRead: boolean;
  syncWrite: boolean;
  syncCredentialsWrite: boolean;
  syncRun: boolean;
};

export type McpCurrentWorkspace = {
  workspaceId: string;
  workspaceGeneration: number;
  displayName: string;
  leafName: string;
  available: boolean;
};

export type McpAuditPolicy = {
  enabled: boolean;
  retentionDays: number;
  maxEntries: number;
};

export type McpConfig = {
  version: number;
  enabled: boolean;
  permissions: McpPermissions;
  confirmation: McpConfirmationPolicy;
  dryRun: McpDryRunPolicy;
  deletion: McpDeletionPolicy;
  recycleBinRetentionDays: McpRecycleBinRetentionDays;
  syncAfterWrite: McpSyncAfterWritePolicy;
  syncExecution: McpSyncExecutionPolicy;
  documentLimitBytes: number;
  requestLimitBytes: number;
  responseLimitBytes: number;
  requestsPerMinute: number;
  burstRequests: number;
  concurrentCalls: number;
  toolTimeoutSecs: number;
  audit: McpAuditPolicy;
};

export type McpServerHealth = {
  state: "disabled" | "stopped" | "starting" | "running" | "error";
  endpoint: string | null;
  errorCode: string | null;
};

export type McpSettingsSnapshot = {
  revision: string;
  config: McpConfig;
  clientCommand: string | null;
  endpoint: string | null;
  health: McpServerHealth;
  workspace: McpCurrentWorkspace | null;
};

export type McpAuditEntry = {
  requestId: string;
  timestampMs: number;
  tool: string;
  workspaceId: string | null;
  workspaceDisplayName: string | null;
  logicalTarget: string | null;
  dryRun: boolean;
  confirmation: "allowed" | "rejected" | "timed_out" | null;
  outcome: "succeeded" | "failed" | "previewed";
  errorCode: string | null;
  revisionBefore: string | null;
  revisionAfter: string | null;
  syncRunId: string | null;
  durationMs: number;
  counts: Record<string, number>;
};

export function isMcpRevisionConflict(message: string) {
  return message.includes("revision-conflict") || message.includes("settings_revision_conflict");
}

export function defaultMcpConfig(): McpConfig {
  return {
    version: 1,
    enabled: false,
    permissions: {
      documentsRead: false,
      documentsWrite: false,
      documentsMove: false,
      documentsDelete: false,
      settingsRead: false,
      settingsWrite: false,
      syncRead: false,
      syncWrite: false,
      syncCredentialsWrite: false,
      syncRun: false
    },
    confirmation: "destructive-only",
    dryRun: "high-risk",
    deletion: "system-trash",
    recycleBinRetentionDays: 30,
    syncAfterWrite: "follow-workspace",
    syncExecution: "background",
    documentLimitBytes: 8 * 1024 * 1024,
    requestLimitBytes: 8 * 1024 * 1024,
    responseLimitBytes: 8 * 1024 * 1024,
    requestsPerMinute: 120,
    burstRequests: 20,
    concurrentCalls: 8,
    toolTimeoutSecs: 60,
    audit: { enabled: true, retentionDays: 30, maxEntries: 10000 }
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function booleanOr(value: unknown, fallback: boolean) {
  return typeof value === "boolean" ? value : fallback;
}

function integerInRange(value: unknown, fallback: number, minimum: number, maximum: number) {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) return fallback;
  return Math.min(maximum, Math.max(minimum, value));
}

function enumOr<T extends string>(value: unknown, allowed: readonly T[], fallback: T) {
  return typeof value === "string" && allowed.includes(value as T) ? value as T : fallback;
}

function recycleBinRetentionDaysOr(value: unknown): McpRecycleBinRetentionDays {
  return value === 0 || value === 7 || value === 30 || value === 90 ? value : 30;
}

export function normalizeMcpConfig(value: unknown): McpConfig {
  const defaults = defaultMcpConfig();
  if (!isRecord(value)) return defaults;
  const permissions = isRecord(value.permissions) ? value.permissions : {};
  const audit = isRecord(value.audit) ? value.audit : {};
  const maxBytes = 64 * 1024 * 1024;

  return {
    version: 1,
    enabled: booleanOr(value.enabled, defaults.enabled),
    permissions: {
      documentsRead: booleanOr(permissions.documentsRead, false),
      documentsWrite: booleanOr(permissions.documentsWrite, false),
      documentsMove: booleanOr(permissions.documentsMove, false),
      documentsDelete: booleanOr(permissions.documentsDelete, false),
      settingsRead: booleanOr(permissions.settingsRead, false),
      settingsWrite: booleanOr(permissions.settingsWrite, false),
      syncRead: booleanOr(permissions.syncRead, false),
      syncWrite: booleanOr(permissions.syncWrite, false),
      syncCredentialsWrite: booleanOr(permissions.syncCredentialsWrite, false),
      syncRun: booleanOr(permissions.syncRun, false)
    },
    confirmation: enumOr(
      value.confirmation,
      ["never", "destructive-only", "all-writes"],
      defaults.confirmation
    ),
    dryRun: enumOr(value.dryRun, ["never", "high-risk", "all-writes"], defaults.dryRun),
    deletion: enumOr(
      value.deletion,
      ["system-trash", "qing-yu-recycle-bin", "permanent"],
      defaults.deletion
    ),
    recycleBinRetentionDays: recycleBinRetentionDaysOr(value.recycleBinRetentionDays),
    syncAfterWrite: enumOr(
      value.syncAfterWrite,
      ["follow-workspace", "always", "never"],
      defaults.syncAfterWrite
    ),
    syncExecution: enumOr(value.syncExecution, ["background", "wait"], defaults.syncExecution),
    documentLimitBytes: integerInRange(value.documentLimitBytes, defaults.documentLimitBytes, 1, maxBytes),
    requestLimitBytes: integerInRange(value.requestLimitBytes, defaults.requestLimitBytes, 1, maxBytes),
    responseLimitBytes: integerInRange(value.responseLimitBytes, defaults.responseLimitBytes, 1, maxBytes),
    requestsPerMinute: integerInRange(value.requestsPerMinute, defaults.requestsPerMinute, 1, 600),
    burstRequests: integerInRange(value.burstRequests, defaults.burstRequests, 1, 100),
    concurrentCalls: integerInRange(value.concurrentCalls, defaults.concurrentCalls, 1, 32),
    toolTimeoutSecs: integerInRange(value.toolTimeoutSecs, defaults.toolTimeoutSecs, 5, 600),
    audit: {
      enabled: booleanOr(audit.enabled, defaults.audit.enabled),
      retentionDays: integerInRange(audit.retentionDays, defaults.audit.retentionDays, 1, 365),
      maxEntries: integerInRange(audit.maxEntries, defaults.audit.maxEntries, 100, 100000)
    }
  };
}
