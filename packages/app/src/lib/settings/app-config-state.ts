import { normalizeNullableString } from "@markra/shared";
import { showAppToast } from "../app-toast";
import {
  getAppRuntime,
  kernelWorkspacePathFromRelativePath,
  kernelWorkspaceRelativePathFromPath,
  kernelWorkspaceRoot,
  type AppConfigRuntime,
  type KernelAppConfigSnapshot,
  type KernelAppConfigStateOperation,
  type KernelWorkspaceLayoutPatch
} from "../../runtime";
import { normalizeRecentMarkdownFiles, type RecentMarkdownFile } from "./recent-markdown";
import {
  defaultWorkspaceState,
  normalizeStoredFileTreeSort,
  normalizeWorkspaceState,
  type StoredFileTreeSort,
  type StoredFileTreeSortByWorkspace,
  type StoredWorkspaceDraftTab,
  type StoredWorkspaceSideBySideGroup,
  type StoredWorkspaceState,
  type StoredWorkspaceWindow
} from "./workspace-state";

const mainWindowLabel = "main";
const settingsWindowLabel = "markra-settings";
const draftCoalescingMs = 400;
const maxTransientAttempts = 2;
const persistenceWarningId = "app-config-state-persistence-warning";

type StoredWorkspaceStateOptions = {
  windowLabel?: string | null;
};

type PatchOperation = Extract<KernelAppConfigStateOperation, { type: "patch-ui-layout" }>;

type PersistenceEntry = {
  operation: KernelAppConfigStateOperation;
  reject: Array<(error: unknown) => unknown>;
  resolve: Array<(value: unknown) => unknown>;
};

export type AppConfigStatePersistenceCoordinator = {
  enqueueImmediate: (operation: KernelAppConfigStateOperation) => Promise<unknown>;
  enqueueDraft: (operation: PatchOperation) => Promise<unknown>;
  flush: () => Promise<unknown>;
  retire: () => undefined;
};

type CoordinatorOwner = {
  appConfig: AppConfigRuntime;
  coordinator: AppConfigStatePersistenceCoordinator;
};

let coordinatorOwner: CoordinatorOwner | null = null;

function appConfigStatePersistenceCoordinator() {
  const appConfig = getAppRuntime().appConfig;
  if (coordinatorOwner?.appConfig !== appConfig) {
    coordinatorOwner?.coordinator.retire();
    coordinatorOwner = {
      appConfig,
      coordinator: createAppConfigStatePersistenceCoordinator(appConfig)
    };
    installPersistenceFlushListeners(coordinatorOwner.coordinator);
  }
  return coordinatorOwner.coordinator;
}

function createAppConfigStatePersistenceCoordinator(
  appConfig: AppConfigRuntime
): AppConfigStatePersistenceCoordinator {
  const queue: PersistenceEntry[] = [];
  const drainWaiters: Array<(value?: unknown) => unknown> = [];
  let draft: PersistenceEntry | null = null;
  let draftTimer: ReturnType<typeof setTimeout> | null = null;
  let processing = false;
  let retired = false;
  let warned = false;

  const settleDrain = () => {
    if (processing || queue.length > 0 || draft) return;
    drainWaiters.splice(0).forEach((resolve) => resolve());
  };

  const warnOnce = () => {
    if (warned || retired) return;
    warned = true;
    showAppToast({
      id: persistenceWarningId,
      message: "Workspace state could not be saved.",
      status: "warning"
    });
  };

  const dropQueued = () => {
    queue.splice(0).forEach((entry) => entry.resolve.forEach((resolve) => resolve(undefined)));
    if (draft) {
      draft.resolve.forEach((resolve) => resolve(undefined));
      draft = null;
    }
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = null;
  };

  const retire = () => {
    if (retired) return undefined;
    retired = true;
    dropQueued();
    settleDrain();
    return undefined;
  };

  const send = async (entry: PersistenceEntry) => {
    let attempt = 0;
    while (attempt < maxTransientAttempts) {
      attempt += 1;
      try {
        return await appConfig.patchState([entry.operation]);
      } catch (error: unknown) {
        if (isStaleGenerationError(error)) {
          retire();
          throw error;
        }
        if (!isTransientPersistenceError(error) || attempt >= maxTransientAttempts) {
          warnOnce();
          throw error;
        }
      }
    }
    return undefined;
  };

  const process = async () => {
    if (processing || retired) return;
    processing = true;
    while (!retired && queue.length > 0) {
      const entry = queue.shift()!;
      try {
        const result = await send(entry);
        entry.resolve.forEach((resolve) => resolve(result));
      } catch (error: unknown) {
        entry.reject.forEach((reject) => reject(error));
      }
    }
    processing = false;
    settleDrain();
  };

  const enqueueEntry = (entry: PersistenceEntry) => {
    if (retired) {
      entry.resolve.forEach((resolve) => resolve(undefined));
      return;
    }
    queue.push(entry);
    process().catch(() => undefined);
  };

  const flushDraft = () => {
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = null;
    const pending = draft;
    draft = null;
    if (pending) enqueueEntry(pending);
  };

  const promiseEntry = (operation: KernelAppConfigStateOperation) => {
    let reject!: (error: unknown) => unknown;
    let resolve!: (value: unknown) => unknown;
    const promise = new Promise<unknown>((promiseResolve, promiseReject) => {
      reject = promiseReject;
      resolve = promiseResolve;
    });
    return { entry: { operation, reject: [reject], resolve: [resolve] }, promise };
  };

  const enqueueImmediate = (operation: KernelAppConfigStateOperation) => {
    flushDraft();
    const pending = promiseEntry(operation);
    enqueueEntry(pending.entry);
    return pending.promise;
  };

  const enqueueDraft = (operation: PatchOperation) => {
    const pending = promiseEntry(operation);
    if (retired) {
      pending.entry.resolve[0]?.(undefined);
      return pending.promise;
    }
    if (draft && draft.operation.type === "patch-ui-layout" &&
      draft.operation.windowLabel === operation.windowLabel) {
      draft.operation = {
        ...operation,
        patch: { ...draft.operation.patch, ...operation.patch }
      };
      draft.reject.push(...pending.entry.reject);
      draft.resolve.push(...pending.entry.resolve);
    } else {
      flushDraft();
      draft = pending.entry;
    }
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = setTimeout(flushDraft, draftCoalescingMs);
    return pending.promise;
  };

  const flush = () => {
    flushDraft();
    if (!processing && queue.length === 0) return Promise.resolve(undefined);
    return new Promise<unknown>((resolve) => drainWaiters.push(resolve));
  };

  return { enqueueDraft, enqueueImmediate, flush, retire };
}

function installPersistenceFlushListeners(coordinator: AppConfigStatePersistenceCoordinator) {
  if (typeof window !== "undefined") {
    window.addEventListener("pagehide", () => {
      coordinator.flush().catch(() => undefined);
    }, { once: true });
  }
  if (typeof document !== "undefined") {
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") coordinator.flush().catch(() => undefined);
    });
  }
  getAppRuntime().window.listenAppExitRequested(() => coordinator.flush()).catch(() => undefined);
}

function errorProperty(error: unknown, property: string) {
  if (typeof error !== "object" || error === null || !(property in error)) return undefined;
  return (error as Record<string, unknown>)[property];
}

function isStaleGenerationError(error: unknown) {
  const code = errorProperty(error, "code");
  return code === "workspace_generation_stale" || code === "workspace-generation-mismatch";
}

function isTransientPersistenceError(error: unknown) {
  const code = errorProperty(error, "code");
  const kind = errorProperty(error, "kind");
  if (kind === "network" || kind === "connection") return true;
  if (typeof code === "string") {
    return [
      "app_config_unavailable",
      "authentication_unavailable",
      "internal_error",
      "kernel_not_ready",
      "workspace_locked",
      "workspace_unavailable"
    ].includes(code);
  }
  return error instanceof Error;
}

async function resolveWindowLabel(options: StoredWorkspaceStateOptions = {}) {
  let label = options.windowLabel;
  if (!("windowLabel" in options)) {
    try {
      label = await getAppRuntime().window.getCurrentWindowLabel();
    } catch {
      label = null;
    }
  }
  const normalized = label?.trim() || mainWindowLabel;
  return normalized === settingsWindowLabel ? mainWindowLabel : normalized;
}

function relativePath(path: string) {
  return kernelWorkspaceRelativePathFromPath(path);
}

function nullableRelativePath(path: string | null | undefined) {
  const normalized = normalizeNullableString(path);
  return normalized === null ? null : relativePath(normalized);
}

function mapDraftTab(draft: StoredWorkspaceDraftTab) {
  return {
    ...draft,
    path: nullableRelativePath(draft.path)
  };
}

function mapSideBySideGroup(group: StoredWorkspaceSideBySideGroup | null | undefined) {
  if (!group) return null;
  const primaryFilePath = relativePath(group.primaryFilePath);
  const sideFilePath = relativePath(group.sideFilePath);
  if (primaryFilePath === sideFilePath) return null;
  return { primaryFilePath, sideFilePath };
}

function mapWindow(windowState: StoredWorkspaceWindow) {
  return {
    filePath: nullableRelativePath(windowState.filePath),
    label: windowState.label,
    openFilePaths: windowState.openFilePaths.map(relativePath)
  };
}

function layoutPatch(patch: Partial<StoredWorkspaceState>): KernelWorkspaceLayoutPatch {
  const mapped: KernelWorkspaceLayoutPatch = {};
  if (patch.activeDraftId !== undefined) mapped.activeDraftId = normalizeNullableString(patch.activeDraftId);
  if (patch.draftTabs !== undefined) {
    const normalized = normalizeWorkspaceState({ ...defaultWorkspaceState, draftTabs: patch.draftTabs });
    mapped.draftTabs = (normalized.draftTabs ?? []).map(mapDraftTab);
  }
  if (patch.filePath !== undefined) mapped.filePath = nullableRelativePath(patch.filePath);
  if (patch.fileTreeAssetsVisible !== undefined) mapped.fileTreeAssetsVisible = patch.fileTreeAssetsVisible !== false;
  if (patch.fileTreeOpen !== undefined) mapped.fileTreeOpen = patch.fileTreeOpen === true;
  if (patch.folderName !== undefined) mapped.folderName = normalizeNullableString(patch.folderName);
  if (patch.folderPath !== undefined) mapped.folderPath = nullableRelativePath(patch.folderPath);
  if (patch.openFilePaths !== undefined) {
    mapped.openFilePaths = normalizeWorkspaceState({
      ...defaultWorkspaceState,
      openFilePaths: patch.openFilePaths
    }).openFilePaths.map(relativePath);
  }
  if (patch.openWindows !== undefined) {
    mapped.openWindows = (normalizeWorkspaceState({
      ...defaultWorkspaceState,
      openWindows: patch.openWindows
    }).openWindows ?? []).map(mapWindow);
  }
  if (patch.sideBySideGroup !== undefined) {
    mapped.sideBySideGroup = mapSideBySideGroup(patch.sideBySideGroup);
  }
  return mapped;
}

function isDraftPatch(patch: Partial<StoredWorkspaceState>) {
  return patch.activeDraftId !== undefined || patch.draftTabs !== undefined;
}

function recentFilesFromSnapshot(value: KernelAppConfigSnapshot) {
  return normalizeRecentMarkdownFiles(value.localState.recentMarkdownFiles.map((file) => ({
    name: file.name,
    path: kernelWorkspacePathFromRelativePath(file.path)
  })));
}

export async function getStoredWorkspaceState(
  options: StoredWorkspaceStateOptions = {}
): Promise<StoredWorkspaceState> {
  return getAppRuntime().appConfig.readWorkspaceState(await resolveWindowLabel(options));
}

export async function saveStoredWorkspaceState(
  patch: Partial<StoredWorkspaceState>,
  options: StoredWorkspaceStateOptions = {}
) {
  const operation: PatchOperation = {
    patch: layoutPatch(patch),
    type: "patch-ui-layout",
    windowLabel: await resolveWindowLabel(options)
  };
  const coordinator = appConfigStatePersistenceCoordinator();
  return isDraftPatch(patch)
    ? coordinator.enqueueDraft(operation)
    : coordinator.enqueueImmediate(operation);
}

export async function getStoredRecentMarkdownFiles(): Promise<RecentMarkdownFile[]> {
  return recentFilesFromSnapshot(getAppRuntime().appConfig.getSnapshot());
}

export async function saveStoredRecentMarkdownFile(file: RecentMarkdownFile) {
  const normalized = normalizeRecentMarkdownFiles([file])[0];
  if (!normalized) return getStoredRecentMarkdownFiles();
  const result = await appConfigStatePersistenceCoordinator().enqueueImmediate({
    file: {
      name: normalized.name,
      path: relativePath(normalized.path)
    },
    type: "remember-recent-file"
  });
  return recentFilesFromSnapshot((result ?? getAppRuntime().appConfig.getSnapshot()) as KernelAppConfigSnapshot);
}

export async function removeStoredRecentMarkdownFile(path: string) {
  const normalized = normalizeNullableString(path);
  if (!normalized) return getStoredRecentMarkdownFiles();
  const result = await appConfigStatePersistenceCoordinator().enqueueImmediate({
    path: relativePath(normalized),
    type: "remove-recent-file"
  });
  if (result === undefined) return undefined;
  return recentFilesFromSnapshot(result as KernelAppConfigSnapshot);
}

export async function clearStoredRecentMarkdownFiles() {
  await appConfigStatePersistenceCoordinator().enqueueImmediate({ type: "clear-recent-files" });
}

export async function getStoredFileTreeSortByWorkspace(): Promise<StoredFileTreeSortByWorkspace> {
  return {
    [kernelWorkspaceRoot]: normalizeStoredFileTreeSort(
      getAppRuntime().appConfig.getSnapshot().localState.fileTreeSort
    )
  };
}

export async function saveStoredFileTreeSortForWorkspace(
  workspacePath: string | null | undefined,
  sort: StoredFileTreeSort
) {
  if (!normalizeNullableString(workspacePath)) return;
  await appConfigStatePersistenceCoordinator().enqueueImmediate({
    sort: normalizeStoredFileTreeSort(sort),
    type: "set-file-tree-sort"
  });
}

function normalizePandocPath(value: unknown) {
  if (typeof value !== "string") return "";
  return value.trim().slice(0, 500);
}

export async function loadLocalPandocPath() {
  return normalizePandocPath(getAppRuntime().appConfig.getSnapshot().localState.pandocPath);
}

export async function saveLocalPandocPath(path: string) {
  const normalized = normalizePandocPath(path);
  await appConfigStatePersistenceCoordinator().enqueueImmediate({
    path: normalized || null,
    type: "set-pandoc-path"
  });
  return normalized;
}

export function flushAppConfigStatePersistence() {
  return appConfigStatePersistenceCoordinator().flush();
}

export function retireAppConfigStatePersistence() {
  const coordinator = appConfigStatePersistenceCoordinator();
  coordinator.retire();
  coordinatorOwner = null;
  return undefined;
}
