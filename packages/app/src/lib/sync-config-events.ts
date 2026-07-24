import { getAppRuntime, type RuntimeCleanup } from "../runtime";
import type {
  SyncApplyUpdate,
  SyncEditingEvent,
  SyncEditingUpdate,
  SyncPendingApply,
  SyncRunResult,
  SyncSafeError,
  SyncStatus,
  SyncTrigger
} from "./sync-config";

export const syncConfigChangedEvent = "qingyu://sync-config-changed";
export const syncEditingEvent = "qingyu://sync-config-editing";
export const syncApplyRequestedEvent = "qingyu://sync-config-apply-requested";
export const syncRunRequestedEvent = "qingyu://sync-run-requested";
export const syncRunCompletedEvent = "qingyu://sync-run-completed";
export const syncStatusChangedEvent = "qingyu://sync-status-changed";

export type SyncConfigChangedPayload = { revision: string };
export type SyncEditingPayload = SyncEditingEvent;
export type SyncApplyRequestedPayload = SyncPendingApply;
export type SyncRunRequestedPayload = {
  notebookName: string;
  notesRoot: string;
  requestId: string;
  revision: string;
  sessionId: string;
  trigger: SyncTrigger;
};
export type SyncRunCompletedPayload = {
  accepted: boolean;
  error: SyncSafeError | null;
  notebookName: string;
  notesRoot: string;
  requestId: string;
  result: SyncRunResult | null;
  revision: string;
  sessionId: string;
  trigger: SyncTrigger;
};
export type SyncStatusChangedPayload = {
  notebookName: string;
  notesRoot: string;
  revision: string;
  status: SyncStatus;
};

function emitEvent<TPayload>(event: string, payload: TPayload): Promise<unknown> {
  if (!getAppRuntime().events.isAvailable()) return Promise.resolve(undefined);
  return getAppRuntime().events.emit(event, payload);
}

function listenEvent<TPayload>(
  event: string,
  handler: (payload: TPayload) => unknown
): Promise<RuntimeCleanup> {
  if (!getAppRuntime().events.isAvailable()) return Promise.resolve(() => {});
  return getAppRuntime().events.listen<TPayload>(event, ({ payload }) => handler(payload));
}

export function emitSyncConfigChanged(payload: SyncConfigChangedPayload) {
  return emitEvent(syncConfigChangedEvent, payload);
}

export function listenSyncConfigChanged(handler: (payload: SyncConfigChangedPayload) => unknown) {
  return listenEvent(syncConfigChangedEvent, handler);
}

export async function emitSyncEditing(payload: SyncEditingUpdate) {
  const stored = await getAppRuntime().syncConfig.setEditing(payload);
  if (!stored.broadcasted) await emitEvent(syncEditingEvent, stored.event);
  return stored.event;
}

export function listenSyncEditing(handler: (payload: SyncEditingPayload) => unknown) {
  return listenEvent(syncEditingEvent, handler);
}

export async function emitSyncApplyRequested(payload: SyncApplyUpdate) {
  const stored = await getAppRuntime().syncConfig.requestApply(payload);
  if (!stored.broadcasted) await emitEvent(syncApplyRequestedEvent, stored.event);
  return stored.event;
}

export function listenSyncApplyRequested(handler: (payload: SyncApplyRequestedPayload) => unknown) {
  return listenEvent(syncApplyRequestedEvent, handler);
}

export function emitSyncRunRequested(payload: SyncRunRequestedPayload) {
  return emitEvent(syncRunRequestedEvent, payload);
}

export function listenSyncRunRequested(handler: (payload: SyncRunRequestedPayload) => unknown) {
  return listenEvent(syncRunRequestedEvent, handler);
}

export function emitSyncRunCompleted(payload: SyncRunCompletedPayload) {
  return emitEvent(syncRunCompletedEvent, payload);
}

export function listenSyncRunCompleted(handler: (payload: SyncRunCompletedPayload) => unknown) {
  return listenEvent(syncRunCompletedEvent, handler);
}

export function emitSyncStatusChanged(payload: SyncStatusChangedPayload) {
  return emitEvent(syncStatusChangedEvent, payload);
}

export function listenSyncStatusChanged(handler: (payload: SyncStatusChangedPayload) => unknown) {
  return listenEvent(syncStatusChangedEvent, handler);
}
