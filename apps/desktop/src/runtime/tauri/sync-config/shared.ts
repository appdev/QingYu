import {
  appLogger,
  notebookNameFromRoot,
  normalizeSyncConfigLoadResult,
  type AppSyncConfigRuntime,
  type SyncConfigDocument,
  type SyncConfigLoadResult,
  type SyncConnectionTestResult,
  type SyncRunResult
} from "@markra/app/runtime";
import { invokeNative } from "../invoke";

export function enableNativeSyncConfig(
  input: Parameters<AppSyncConfigRuntime["enable"]>[0]
): ReturnType<AppSyncConfigRuntime["enable"]> {
  return invokeNative<SyncConfigDocument>("enable_sync_config", {
    expectedRevision: input.expectedRevision
  });
}

export async function loadNativeSyncConfig(): ReturnType<AppSyncConfigRuntime["load"]> {
  return normalizeSyncConfigLoadResult(
    await invokeNative<SyncConfigLoadResult>("load_sync_config")
  );
}

export function listNativeNotebooks(
  input: Parameters<AppSyncConfigRuntime["listNotebooks"]>[0]
): ReturnType<AppSyncConfigRuntime["listNotebooks"]> {
  return invokeNative("list_remote_notebooks", { request: input });
}

export function loadNativeSyncConfigEditing(): ReturnType<AppSyncConfigRuntime["loadEditing"]> {
  return invokeNative("load_sync_config_editing");
}

export async function cancelNativeSyncConfigApply(
  input: Parameters<AppSyncConfigRuntime["cancelApply"]>[0]
): ReturnType<AppSyncConfigRuntime["cancelApply"]> {
  const event = await invokeNative<
    Awaited<ReturnType<AppSyncConfigRuntime["cancelApply"]>>["event"]
  >("cancel_sync_config_apply", { request: input });
  return { broadcasted: false, event };
}

export function loadNativeSyncStatus(): ReturnType<AppSyncConfigRuntime["loadStatus"]> {
  return invokeNative("load_sync_status");
}

export function patchNativeSyncConfig(
  input: Parameters<AppSyncConfigRuntime["patch"]>[0]
): ReturnType<AppSyncConfigRuntime["patch"]> {
  return invokeNative<SyncConfigDocument>("patch_sync_config", {
    request: input
  });
}

export function recoverNativeSyncConfig(
  input: Parameters<AppSyncConfigRuntime["recover"]>[0]
): ReturnType<AppSyncConfigRuntime["recover"]> {
  return invokeNative<SyncConfigDocument>("recover_sync_config", {
    request: input
  });
}

export function resetNativeSyncConfig(
  input: Parameters<AppSyncConfigRuntime["reset"]>[0]
): ReturnType<AppSyncConfigRuntime["reset"]> {
  return invokeNative<SyncConfigDocument>("reset_sync_config", {
    request: input
  });
}

export async function setNativeSyncConfigEditing(
  input: Parameters<AppSyncConfigRuntime["setEditing"]>[0]
): ReturnType<AppSyncConfigRuntime["setEditing"]> {
  const event = await invokeNative<
    Awaited<ReturnType<AppSyncConfigRuntime["setEditing"]>>["event"]
  >("set_sync_config_editing", { request: input });
  return { broadcasted: true, event };
}

export async function requestNativeSyncConfigApply(
  input: Parameters<AppSyncConfigRuntime["requestApply"]>[0]
): ReturnType<AppSyncConfigRuntime["requestApply"]> {
  const event = await invokeNative<
    Awaited<ReturnType<AppSyncConfigRuntime["requestApply"]>>["event"]
  >("request_sync_config_apply", { request: input });
  return { broadcasted: true, event };
}

export async function syncApplication(
  input: Parameters<AppSyncConfigRuntime["sync"]>[0]
): Promise<SyncRunResult> {
  const result = await invokeNative<SyncRunResult>("sync_application", { request: input });
  const expectedNotesRoot = "notesRoot" in input ? input.notesRoot : result.notesRoot;
  const expectedNotebookName = "notebookName" in input
    ? input.notebookName
    : notebookNameFromRoot(expectedNotesRoot);
  if (
    result.notesRoot !== expectedNotesRoot ||
    result.notebookName !== expectedNotebookName ||
    result.revision !== input.revision
  ) {
    throw new Error("sync-result-mismatch");
  }
  appLogger.info("sync", "Application synchronization completed", {
    bytesDownloaded: result.summary.bytesDownloaded,
    bytesUploaded: result.summary.bytesUploaded,
    conflictFiles: result.summary.conflictFiles,
    downloadedFiles: result.summary.downloadedFiles,
    provider: result.provider,
    revision: result.revision,
    scannedFiles: result.summary.scannedFiles,
    skippedFiles: result.summary.skippedFiles,
    trigger: result.trigger,
    uploadedFiles: result.summary.uploadedFiles
  });
  return result;
}

export function testSyncConnection(
  input: Parameters<AppSyncConfigRuntime["testConnection"]>[0]
): Promise<SyncConnectionTestResult> {
  return invokeNative<SyncConnectionTestResult>("test_sync_connection", { request: input });
}
