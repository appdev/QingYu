import { normalizeNullableString } from "@markra/shared";
import { getAppRuntime, type RuntimeStore } from "../../runtime";
import {
  normalizeRecentMarkdownFiles,
  normalizeRecentNotebooks,
  prependRecentMarkdownFile,
  prependRecentNotebook,
  type RecentMarkdownFile,
  type RecentNotebook
} from "./recent-markdown";
import {
  defaultWorkspaceState,
  normalizeFileTreeSortByWorkspace,
  normalizeStoredFileTreeSort,
  normalizeWorkspaceState,
  type StoredFileTreeSort,
  type StoredFileTreeSortByWorkspace,
  type StoredWorkspaceState,
  type StoredWorkspaceWindow,
  type StoredWorkspaceWindowState
} from "./workspace-state";

const localStateStorePath = "local-state.json";
const localStateSchemaVersionKey = "schemaVersion";
const localStateSchemaVersion = 2;
const primaryWorkspaceKey = "primaryWorkspace";
const pandocPathKey = "pandocPath";
const welcomeDocumentSeenKey = "welcomeDocumentSeen";
const fileTreeSortByWorkspaceKey = "fileTreeSortByWorkspace";
const workspaceKey = "workspace";
const recentMarkdownFilesKey = "recentMarkdownFiles";
const recentNotebooksKey = "recentNotebooks";
const mainWorkspaceWindowLabel = "main";
const settingsWorkspaceWindowLabel = "markra-settings";
const maxFileTreeSortWorkspaceEntries = 50;

export type PrimaryWorkspaceState = {
  desktopWorkspaceRoot: string | null;
  desktopPath: string | null;
  managedName: string | null;
  onboardingCompleted: boolean;
  onboardingRequestedForNextLaunch?: true;
  version: 3;
};

export const defaultPrimaryWorkspaceState: PrimaryWorkspaceState = {
  desktopWorkspaceRoot: null,
  desktopPath: null,
  managedName: null,
  onboardingCompleted: false,
  version: 3
};

export function isValidManagedNotebookName(name: string) {
  const normalizedName = name.toLocaleLowerCase("en-US");
  return name.length > 0 &&
    name !== "." &&
    name !== ".." &&
    !name.includes("/") &&
    !name.includes("\\") &&
    !name.includes("\0") &&
    normalizedName !== ".qingyu" &&
    normalizedName !== ".markra-sync" &&
    !normalizedName.startsWith(".markra-sync-stage-");
}

type StoredWorkspaceStateOptions = {
  windowLabel?: string | null;
};

type PrimaryWorkspacePersistenceCoordinator = {
  activeWrites: number;
  initialization: Promise<PrimaryWorkspaceState> | null;
  latest: { revision: number; state: PrimaryWorkspaceState } | null;
  nextRevision: number;
  settings: ReturnType<typeof getAppRuntime>["settings"];
};

let primaryWorkspacePersistenceCoordinator: PrimaryWorkspacePersistenceCoordinator | null = null;

type RecentNotebooksPersistenceCoordinator = {
  settings: ReturnType<typeof getAppRuntime>["settings"];
  tail: Promise<unknown>;
};

let recentNotebooksPersistenceCoordinator: RecentNotebooksPersistenceCoordinator | null = null;

type StoredWorkspaceStore = {
  legacyState: StoredWorkspaceState;
  openWindows: StoredWorkspaceWindow[];
  windowStates: Record<string, StoredWorkspaceWindowState>;
};

type StoredWorkspaceStoreValue = Partial<StoredWorkspaceState> & {
  windowStates?: unknown;
};

function localStore() {
  return getAppRuntime().settings.loadStore(localStateStorePath, {
    autoSave: false,
    defaults: {}
  });
}

function runRecentNotebooksStoreOperation<T>(
  operation: (store: RuntimeStore) => Promise<T>
) {
  const settings = getAppRuntime().settings;
  if (recentNotebooksPersistenceCoordinator?.settings !== settings) {
    recentNotebooksPersistenceCoordinator = {
      settings,
      tail: Promise.resolve()
    };
  }
  const coordinator = recentNotebooksPersistenceCoordinator;
  const result = coordinator.tail
    .catch(() => undefined)
    .then(async () => {
      const store = await settings.loadStore(localStateStorePath, {
        autoSave: false,
        defaults: {}
      });
      return await operation(store);
    });
  coordinator.tail = result.then(
    () => undefined,
    () => undefined
  );
  return result;
}

async function saveLocalStore(store: RuntimeStore) {
  await store.set(localStateSchemaVersionKey, localStateSchemaVersion);
  await store.save();
}

function primaryWorkspaceCoordinator() {
  const settings = getAppRuntime().settings;
  if (primaryWorkspacePersistenceCoordinator?.settings !== settings) {
    primaryWorkspacePersistenceCoordinator = {
      activeWrites: 0,
      initialization: null,
      latest: null,
      nextRevision: 0,
      settings
    };
  }
  return primaryWorkspacePersistenceCoordinator;
}

async function loadPrimaryWorkspaceStateFromStore() {
  const store = await localStore();
  return normalizePrimaryWorkspaceState(await store.get<unknown>(primaryWorkspaceKey));
}

async function savePrimaryWorkspaceStateToStore(state: PrimaryWorkspaceState) {
  const store = await localStore();
  await store.set(localStateSchemaVersionKey, localStateSchemaVersion);
  await store.set(primaryWorkspaceKey, state);
  await store.save();
}

async function initializePrimaryWorkspaceCoordinator(
  coordinator: PrimaryWorkspacePersistenceCoordinator
) {
  if (coordinator.activeWrites > 0 && coordinator.latest) return coordinator.latest.state;
  if (!coordinator.initialization) {
    coordinator.initialization = loadPrimaryWorkspaceStateFromStore().then((state) => {
      coordinator.latest = { revision: coordinator.nextRevision, state };
      return state;
    }).finally(() => {
      coordinator.initialization = null;
    });
  }
  return coordinator.initialization;
}

async function savePrimaryWorkspaceStateThroughStore(
  state: PrimaryWorkspaceState,
  expectedState?: PrimaryWorkspaceState
) {
  const coordinator = primaryWorkspaceCoordinator();
  const current = await initializePrimaryWorkspaceCoordinator(coordinator);
  if (expectedState !== undefined && !primaryWorkspaceStatesEqual(current, expectedState)) {
    return current;
  }

  const revision = coordinator.nextRevision + 1;
  coordinator.nextRevision = revision;
  coordinator.latest = { revision, state };
  coordinator.activeWrites += 1;
  try {
    await savePrimaryWorkspaceStateToStore(state);

    while (coordinator.latest.revision !== revision) {
      const latest = coordinator.latest;
      await savePrimaryWorkspaceStateToStore(latest.state);
      if (coordinator.latest.revision === latest.revision) break;
    }

    return coordinator.latest.state;
  } finally {
    coordinator.activeWrites -= 1;
    if (coordinator.activeWrites === 0) coordinator.latest = null;
  }
}

function primaryWorkspaceStatesEqual(
  left: PrimaryWorkspaceState,
  right: PrimaryWorkspaceState
) {
  return left.desktopPath === right.desktopPath &&
    left.desktopWorkspaceRoot === right.desktopWorkspaceRoot &&
    left.managedName === right.managedName &&
    left.onboardingCompleted === right.onboardingCompleted &&
    left.onboardingRequestedForNextLaunch === right.onboardingRequestedForNextLaunch &&
    left.version === right.version;
}

export function normalizePrimaryWorkspaceState(value: unknown): PrimaryWorkspaceState {
  if (!value || typeof value !== "object") return defaultPrimaryWorkspaceState;
  const candidate = value as Partial<PrimaryWorkspaceState>;
  if (candidate.version !== 3) return defaultPrimaryWorkspaceState;

  const hasOwn = (key: keyof PrimaryWorkspaceState) =>
    Object.prototype.hasOwnProperty.call(candidate, key);
  const nullableStringHasInvalidType = (key: keyof Pick<
    PrimaryWorkspaceState,
    "desktopWorkspaceRoot" | "desktopPath" | "managedName"
  >) => hasOwn(key) && candidate[key] !== null && typeof candidate[key] !== "string";
  if (
    nullableStringHasInvalidType("desktopWorkspaceRoot") ||
    nullableStringHasInvalidType("desktopPath") ||
    nullableStringHasInvalidType("managedName") ||
    (hasOwn("onboardingCompleted") && typeof candidate.onboardingCompleted !== "boolean") ||
    (
      hasOwn("onboardingRequestedForNextLaunch") &&
      typeof candidate.onboardingRequestedForNextLaunch !== "boolean"
    )
  ) {
    return defaultPrimaryWorkspaceState;
  }

  const desktopWorkspaceRoot = normalizeNullableString(candidate.desktopWorkspaceRoot);
  const desktopPath = normalizeNullableString(candidate.desktopPath);
  const managedName = typeof candidate.managedName === "string"
    ? candidate.managedName
    : null;
  const managedNameIsInvalid = managedName !== null && !isValidManagedNotebookName(managedName);
  const hasDesktopWorkspaceRoot = desktopWorkspaceRoot !== null;
  const hasDesktopPath = desktopPath !== null;
  const hasDesktopIdentity = hasDesktopWorkspaceRoot && hasDesktopPath;
  const desktopIdentityIsInvalid = hasDesktopWorkspaceRoot !== hasDesktopPath;
  if (
    desktopIdentityIsInvalid ||
    (hasDesktopIdentity && managedName !== null) ||
    managedNameIsInvalid
  ) {
    return defaultPrimaryWorkspaceState;
  }

  return {
    desktopWorkspaceRoot,
    desktopPath,
    managedName,
    onboardingCompleted: candidate.onboardingCompleted === true,
    ...(candidate.onboardingRequestedForNextLaunch === true
      ? { onboardingRequestedForNextLaunch: true as const }
      : {}),
    version: 3
  };
}

export async function loadPrimaryWorkspaceState(): Promise<PrimaryWorkspaceState> {
  const runtimeRead = getAppRuntime().settings.readPrimaryWorkspaceState;
  if (runtimeRead) return normalizePrimaryWorkspaceState(await runtimeRead());
  return initializePrimaryWorkspaceCoordinator(primaryWorkspaceCoordinator());
}

export async function savePrimaryWorkspaceState(
  state: PrimaryWorkspaceState
): Promise<PrimaryWorkspaceState> {
  const normalized = normalizePrimaryWorkspaceState(state);
  const runtimeWrite = getAppRuntime().settings.writePrimaryWorkspaceState;
  if (runtimeWrite) {
    const result = await runtimeWrite({ state: normalized });
    return normalizePrimaryWorkspaceState(result.state);
  }
  return (await savePrimaryWorkspaceStateThroughStore(normalized)) ?? normalized;
}

export async function saveCanonicalPrimaryWorkspaceState(
  state: PrimaryWorkspaceState,
  expectedState: PrimaryWorkspaceState
): Promise<PrimaryWorkspaceState> {
  const normalized = normalizePrimaryWorkspaceState(state);
  const normalizedExpectedState = normalizePrimaryWorkspaceState(expectedState);
  const runtimeWrite = getAppRuntime().settings.writePrimaryWorkspaceState;
  if (runtimeWrite) {
    const result = await runtimeWrite({ expectedState: normalizedExpectedState, state: normalized });
    return normalizePrimaryWorkspaceState(result.state);
  }
  return savePrimaryWorkspaceStateThroughStore(normalized, normalizedExpectedState);
}

export async function updatePrimaryWorkspaceState(
  change: Partial<Omit<PrimaryWorkspaceState, "version">>
): Promise<PrimaryWorkspaceState> {
  const current = await loadPrimaryWorkspaceState();
  const normalized = normalizePrimaryWorkspaceState({ ...current, ...change, version: 3 });
  return savePrimaryWorkspaceState(normalized);
}

function normalizeLocalPandocPath(value: unknown) {
  if (typeof value !== "string") return "";
  return value.trim().slice(0, 500);
}

export async function loadLocalPandocPath(): Promise<string> {
  const store = await localStore();
  return normalizeLocalPandocPath(await store.get<unknown>(pandocPathKey));
}

export async function saveLocalPandocPath(path: string): Promise<string> {
  const normalized = normalizeLocalPandocPath(path);
  const store = await localStore();
  await store.set(pandocPathKey, normalized);
  await saveLocalStore(store);
  return normalized;
}

export async function consumeWelcomeDocumentState() {
  const store = await localStore();
  const hasSeenWelcomeDocument = await store.get<boolean>(welcomeDocumentSeenKey);

  if (hasSeenWelcomeDocument) return false;

  await store.set(welcomeDocumentSeenKey, true);
  await saveLocalStore(store);
  return true;
}

export async function resetWelcomeDocumentState() {
  const store = await localStore();
  await store.delete(welcomeDocumentSeenKey);
  await saveLocalStore(store);
}

export async function getStoredFileTreeSortByWorkspace(): Promise<StoredFileTreeSortByWorkspace> {
  const store = await localStore();
  const sortByWorkspace = await store.get<StoredFileTreeSortByWorkspace>(fileTreeSortByWorkspaceKey);
  return normalizeFileTreeSortByWorkspace(sortByWorkspace);
}

export async function saveStoredFileTreeSortForWorkspace(
  workspacePath: string | null | undefined,
  sort: StoredFileTreeSort
) {
  const normalizedWorkspacePath = normalizeNullableString(workspacePath);
  if (!normalizedWorkspacePath) return;

  const store = await localStore();
  const current = normalizeFileTreeSortByWorkspace(
    await store.get<StoredFileTreeSortByWorkspace>(fileTreeSortByWorkspaceKey)
  );
  const nextSortByWorkspaceEntries = [
    [normalizedWorkspacePath, normalizeStoredFileTreeSort(sort)] as const,
    ...Object.entries(current).filter(([path]) => path !== normalizedWorkspacePath)
  ].slice(0, maxFileTreeSortWorkspaceEntries);

  await store.set(fileTreeSortByWorkspaceKey, Object.fromEntries(nextSortByWorkspaceEntries));
  await saveLocalStore(store);
}

function normalizeWorkspaceWindowLabel(label: string | null | undefined) {
  const trimmedLabel = label?.trim();
  return trimmedLabel ? trimmedLabel : mainWorkspaceWindowLabel;
}

function workspaceWindowStateFromWorkspaceState(state: StoredWorkspaceState): StoredWorkspaceWindowState {
  const { openWindows: _openWindows, ...windowState } = state;
  return windowState;
}

function workspaceWindowStateIsEmpty(state: StoredWorkspaceWindowState) {
  return (
    !state.activeDraftId &&
    !state.draftTabs?.length &&
    state.fileTreeAssetsVisible !== false &&
    !state.filePath &&
    !state.fileTreeOpen &&
    !state.folderName &&
    !state.folderPath &&
    state.openFilePaths.length === 0 &&
    state.recentFoldersOpen !== false &&
    !state.sideBySideGroup
  );
}

function normalizeWorkspaceWindowStates(value: unknown) {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};

  const states: Record<string, StoredWorkspaceWindowState> = {};
  Object.entries(value as Record<string, unknown>).forEach(([label, state]) => {
    states[normalizeWorkspaceWindowLabel(label)] = workspaceWindowStateFromWorkspaceState(
      normalizeWorkspaceState(state)
    );
  });
  return states;
}

function normalizeWorkspaceStore(value: unknown): StoredWorkspaceStore {
  const legacyState = normalizeWorkspaceState(value);
  const candidate = typeof value === "object" && value !== null
    ? value as StoredWorkspaceStoreValue
    : {};
  const legacyWindowState = workspaceWindowStateFromWorkspaceState(legacyState);
  const legacyWindowStates: Record<string, StoredWorkspaceWindowState> = {};

  if (!workspaceWindowStateIsEmpty(legacyWindowState)) {
    legacyWindowStates[mainWorkspaceWindowLabel] = legacyWindowState;
  }

  return {
    legacyState,
    openWindows: legacyState.openWindows ?? [],
    windowStates: {
      ...legacyWindowStates,
      ...normalizeWorkspaceWindowStates(candidate.windowStates)
    }
  };
}

async function resolveStoredWorkspaceWindowLabel(options: StoredWorkspaceStateOptions = {}) {
  if ("windowLabel" in options) return normalizeWorkspaceWindowLabel(options.windowLabel);

  try {
    return normalizeWorkspaceWindowLabel(await getAppRuntime().window.getCurrentWindowLabel());
  } catch {
    return mainWorkspaceWindowLabel;
  }
}

function workspaceStateForWindowLabel(store: StoredWorkspaceStore, label: string): StoredWorkspaceState {
  const targetLabel = label === settingsWorkspaceWindowLabel ? mainWorkspaceWindowLabel : label;
  const windowState =
    store.windowStates[targetLabel] ??
    (targetLabel === mainWorkspaceWindowLabel
      ? workspaceWindowStateFromWorkspaceState(store.legacyState)
      : workspaceWindowStateFromWorkspaceState(defaultWorkspaceState));

  return { ...windowState, openWindows: store.openWindows };
}

function workspaceWindowPatchFromStatePatch(patch: Partial<StoredWorkspaceState>) {
  const { openWindows: _openWindows, ...windowPatch } = patch;
  return windowPatch;
}

function workspacePatchHasWindowState(patch: Partial<StoredWorkspaceState>) {
  return Object.keys(patch).some((key) => key !== "openWindows");
}

function serializedWorkspaceStore(store: StoredWorkspaceStore) {
  return { openWindows: store.openWindows, windowStates: store.windowStates };
}

export async function getStoredWorkspaceState(
  options: StoredWorkspaceStateOptions = {}
): Promise<StoredWorkspaceState> {
  const store = await localStore();
  const workspace = normalizeWorkspaceStore(await store.get<StoredWorkspaceStoreValue>(workspaceKey));
  const windowLabel = await resolveStoredWorkspaceWindowLabel(options);
  return workspaceStateForWindowLabel(workspace, windowLabel);
}

export async function saveStoredWorkspaceState(
  patch: Partial<StoredWorkspaceState>,
  options: StoredWorkspaceStateOptions = {}
) {
  const store = await localStore();
  const current = normalizeWorkspaceStore(await store.get<StoredWorkspaceStoreValue>(workspaceKey));

  if (patch.openWindows !== undefined) {
    current.openWindows = normalizeWorkspaceState({ openWindows: patch.openWindows }).openWindows ?? [];
  }

  if (workspacePatchHasWindowState(patch)) {
    current.openWindows = [];
    const windowLabel = await resolveStoredWorkspaceWindowLabel(options);
    const targetLabel = windowLabel === settingsWorkspaceWindowLabel ? mainWorkspaceWindowLabel : windowLabel;
    const currentWindowState = workspaceStateForWindowLabel(current, targetLabel);
    const nextWindowState = workspaceWindowStateFromWorkspaceState(
      normalizeWorkspaceState({
        ...currentWindowState,
        ...workspaceWindowPatchFromStatePatch(patch),
        openWindows: []
      })
    );

    if (workspaceWindowStateIsEmpty(nextWindowState)) {
      delete current.windowStates[targetLabel];
    } else {
      current.windowStates[targetLabel] = nextWindowState;
    }
  }

  await store.set(workspaceKey, serializedWorkspaceStore(current));
  await saveLocalStore(store);
}

export async function getStoredRecentMarkdownFiles(): Promise<RecentMarkdownFile[]> {
  const store = await localStore();
  return normalizeRecentMarkdownFiles(await store.get<RecentMarkdownFile[]>(recentMarkdownFilesKey));
}

export async function saveStoredRecentMarkdownFile(file: RecentMarkdownFile) {
  const store = await localStore();
  const current = normalizeRecentMarkdownFiles(await store.get<RecentMarkdownFile[]>(recentMarkdownFilesKey));
  const files = prependRecentMarkdownFile(current, file);
  await store.set(recentMarkdownFilesKey, files);
  await saveLocalStore(store);
  return files;
}

export async function removeStoredRecentMarkdownFile(path: string) {
  const normalizedPath = path.trim();
  const store = await localStore();
  const current = normalizeRecentMarkdownFiles(await store.get<RecentMarkdownFile[]>(recentMarkdownFilesKey));
  const files = normalizedPath ? current.filter((file) => file.path !== normalizedPath) : current;
  await store.set(recentMarkdownFilesKey, files);
  await saveLocalStore(store);
  return files;
}

export async function clearStoredRecentMarkdownFiles() {
  const store = await localStore();
  await store.delete(recentMarkdownFilesKey);
  await saveLocalStore(store);
}

export async function getStoredRecentNotebooks(): Promise<RecentNotebook[]> {
  return await runRecentNotebooksStoreOperation(async (store) => (
    normalizeRecentNotebooks(await store.get<RecentNotebook[]>(recentNotebooksKey))
  ));
}

export async function saveStoredRecentNotebook(notebook: RecentNotebook) {
  return await runRecentNotebooksStoreOperation(async (store) => {
    const current = normalizeRecentNotebooks(await store.get<RecentNotebook[]>(recentNotebooksKey));
    const notebooks = prependRecentNotebook(current, notebook);
    await store.set(recentNotebooksKey, notebooks);
    await saveLocalStore(store);
    return notebooks;
  });
}

export async function removeStoredRecentNotebook(path: string) {
  return await runRecentNotebooksStoreOperation(async (store) => {
    const current = normalizeRecentNotebooks(await store.get<RecentNotebook[]>(recentNotebooksKey));
    const notebooks = current.filter((notebook) => notebook.path !== path);
    await store.set(recentNotebooksKey, notebooks);
    await saveLocalStore(store);
    return notebooks;
  });
}
