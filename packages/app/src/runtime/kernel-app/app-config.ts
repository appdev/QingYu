import {
  defaultWorkspaceState,
  type StoredWorkspaceDraftTab,
  type StoredWorkspaceSideBySideGroup,
  type StoredWorkspaceState,
  type StoredWorkspaceWindow,
  type StoredWorkspaceWindowState,
} from "../../lib/settings/workspace-state";
import type {
  KernelAppConfigSnapshot,
  KernelAppConfigStateOperation,
  KernelDomainPort,
  KernelRecentMarkdownFile,
  KernelRevision,
  KernelSettingEntrySnapshot,
  KernelStoredWorkspaceDraft,
  KernelStoredWorkspaceLayout,
  KernelStoredWorkspaceSplitGroup,
  KernelStoredWorkspaceWindow,
  KernelStoredWorkspaceWindowState,
  KernelWorkspaceGeneration,
  KernelWorkspaceRelativePath,
} from "../kernel-domain";
import { kernelWorkspaceRoot } from "./files";

const mainWindowLabel = "main";
const settingsWindowLabel = "markra-settings";

export type AppConfigRuntime = {
  readonly bootstrap: KernelAppConfigSnapshot;
  getSnapshot: () => KernelAppConfigSnapshot;
  reload: () => Promise<KernelAppConfigSnapshot>;
  readWorkspaceState: (windowLabel?: string | null) => Promise<StoredWorkspaceState>;
  patchState: (
    operations: readonly KernelAppConfigStateOperation[]
  ) => Promise<KernelAppConfigSnapshot>;
};

export function createKernelAppConfigRuntime(
  kernel: KernelDomainPort,
  getWindowLabel: () => Promise<string | null>,
): AppConfigRuntime {
  if (kernel.availability === "unavailable") return createUnavailableAppConfigRuntime();

  const bootstrap = freezeKernelAppConfigSnapshot(kernel.appConfig.bootstrap);
  const identity = bootstrap.workspace;
  let current = bootstrap;

  const accept = (candidate: KernelAppConfigSnapshot) => {
    const validated = freezeKernelAppConfigSnapshot(candidate);
    if (
      validated.workspace.id !== identity.id ||
      validated.workspace.generation !== identity.generation
    ) {
      throw new Error("The Kernel AppConfig workspace identity changed.");
    }
    current = validated;
    return current;
  };
  const reload = async () => accept(await kernel.appConfig.read());
  const runtime: AppConfigRuntime = Object.freeze({
    bootstrap,
    getSnapshot: () => current,
    patchState: async (operations) => accept(await kernel.appConfig.patchState({
      operations,
      workspaceGeneration: current.workspace.generation,
    })),
    readWorkspaceState: async (windowLabel) => {
      const label = await resolveWindowLabel(windowLabel, getWindowLabel);
      return mapWorkspaceState(current.localState.uiLayout, label);
    },
    reload,
  });

  kernel.invalidations.subscribe((notice) => {
    if (notice.scopes.includes("app-config")) {
      reload().catch(() => undefined);
    }
    return undefined;
  });
  return runtime;
}

export function createUnavailableAppConfigRuntime(): AppConfigRuntime {
  const unavailable = () => new Error(
    "AppConfig is unavailable without a ready Kernel-backed runtime.",
  );
  const rejectUnavailable = <T>(): Promise<T> => Promise.reject(unavailable());
  const runtime: AppConfigRuntime = {
    get bootstrap(): never {
      throw unavailable();
    },
    getSnapshot: () => {
      throw unavailable();
    },
    patchState: rejectUnavailable,
    readWorkspaceState: rejectUnavailable,
    reload: rejectUnavailable,
  };
  return Object.freeze(runtime);
}

export function freezeKernelAppConfigSnapshot(
  source: KernelAppConfigSnapshot,
): KernelAppConfigSnapshot {
  requireAppConfigSnapshot(source);
  return deepFreeze(cloneSnapshot(source));
}

export function kernelWorkspacePathFromRelativePath(
  relativePath: KernelWorkspaceRelativePath,
): string {
  requireWorkspaceRelativePath(relativePath, { allowEmpty: true, markdown: false });
  if (relativePath === "") return kernelWorkspaceRoot;
  return `${kernelWorkspaceRoot}/${relativePath
    .split("/")
    .map((segment) => encodeURIComponent(segment))
    .join("/")}`;
}

export function kernelWorkspaceRelativePathFromPath(
  path: string,
): KernelWorkspaceRelativePath {
  if (path === kernelWorkspaceRoot) return "" as KernelWorkspaceRelativePath;
  const prefix = `${kernelWorkspaceRoot}/`;
  if (!path.startsWith(prefix)) {
    throw new Error("The path is outside the Kernel workspace.");
  }
  const encoded = path.slice(prefix.length);
  let segments: string[];
  try {
    segments = encoded.split("/").map((segment) => decodeURIComponent(segment));
  } catch {
    throw new Error("The Kernel workspace path is invalid.");
  }
  if (segments.some((segment) => segment.includes("/") || segment.includes("\\"))) {
    throw new Error("The Kernel workspace path is invalid.");
  }
  const decoded = segments.join("/");
  requireWorkspaceRelativePath(decoded, { allowEmpty: false, markdown: false });
  return decoded as KernelWorkspaceRelativePath;
}

export function kernelWorkspaceDocumentRelativePathFromPath(
  path: string,
): KernelWorkspaceRelativePath {
  const relativePath = kernelWorkspaceRelativePathFromPath(path);
  requireWorkspaceRelativePath(relativePath, { allowEmpty: false, markdown: true });
  return relativePath;
}

async function resolveWindowLabel(
  requested: string | null | undefined,
  getWindowLabel: () => Promise<string | null>,
) {
  let candidate = requested;
  if (candidate === undefined) {
    try {
      candidate = await getWindowLabel();
    } catch {
      candidate = null;
    }
  }
  const normalized = candidate?.trim() || mainWindowLabel;
  return normalized === settingsWindowLabel ? mainWindowLabel : normalized;
}

function mapWorkspaceState(
  layout: KernelStoredWorkspaceLayout,
  windowLabel: string,
): StoredWorkspaceState {
  const selected = layout.windowStates[windowLabel];
  const state = selected === undefined
    ? { ...defaultWorkspaceState, openWindows: undefined }
    : mapWindowState(selected);
  return {
    ...state,
    openWindows: layout.openWindows.map(mapWindow),
  };
}

function mapWindowState(state: KernelStoredWorkspaceWindowState): StoredWorkspaceWindowState {
  return {
    activeDraftId: state.activeDraftId,
    draftTabs: state.draftTabs.map(mapDraft),
    filePath: mapNullablePath(state.filePath),
    fileTreeAssetsVisible: state.fileTreeAssetsVisible,
    fileTreeOpen: state.fileTreeOpen,
    folderName: state.folderName,
    folderPath: mapNullablePath(state.folderPath),
    openFilePaths: state.openFilePaths.map(kernelWorkspacePathFromRelativePath),
    sideBySideGroup: state.sideBySideGroup === null
      ? null
      : mapSplitGroup(state.sideBySideGroup),
  };
}

function mapDraft(draft: KernelStoredWorkspaceDraft): StoredWorkspaceDraftTab {
  return {
    content: draft.content,
    id: draft.id,
    name: draft.name,
    path: mapNullablePath(draft.path),
  };
}

function mapSplitGroup(group: KernelStoredWorkspaceSplitGroup): StoredWorkspaceSideBySideGroup {
  return {
    primaryFilePath: kernelWorkspacePathFromRelativePath(group.primaryFilePath),
    sideFilePath: kernelWorkspacePathFromRelativePath(group.sideFilePath),
  };
}

function mapWindow(window: KernelStoredWorkspaceWindow): StoredWorkspaceWindow {
  return {
    filePath: mapNullablePath(window.filePath),
    label: window.label,
    openFilePaths: window.openFilePaths.map(kernelWorkspacePathFromRelativePath),
  };
}

function mapNullablePath(path: KernelWorkspaceRelativePath | null) {
  return path === null ? null : kernelWorkspacePathFromRelativePath(path);
}

function requireAppConfigSnapshot(source: KernelAppConfigSnapshot) {
  if (
    source.appConfigVersion !== 1 ||
    !source.workspace.id ||
    !source.workspace.generation ||
    source.localState.uiLayout.schemaVersion !== 1
  ) {
    throw new Error("The Kernel AppConfig snapshot is invalid.");
  }
  for (const [label, state] of Object.entries(source.localState.uiLayout.windowStates)) {
    if (!label || label !== label.trim()) invalidSnapshot();
    requireWindowState(state);
  }
  for (const window of source.localState.uiLayout.openWindows) {
    if (!window.label || window.label !== window.label.trim()) invalidSnapshot();
    requireNullableMarkdownPath(window.filePath);
    window.openFilePaths.forEach(requireMarkdownPath);
  }
  source.localState.recentMarkdownFiles.forEach((file) => {
    if (!file.name || file.name !== file.name.trim()) invalidSnapshot();
    requireMarkdownPath(file.path);
  });
}

function requireWindowState(state: KernelStoredWorkspaceWindowState) {
  requireNullableMarkdownPath(state.filePath);
  if (state.folderPath !== null) {
    requireWorkspaceRelativePath(state.folderPath, { allowEmpty: true, markdown: false });
  }
  state.openFilePaths.forEach(requireMarkdownPath);
  state.draftTabs.forEach((draft) => requireNullableMarkdownPath(draft.path));
  if (state.sideBySideGroup !== null) {
    requireMarkdownPath(state.sideBySideGroup.primaryFilePath);
    requireMarkdownPath(state.sideBySideGroup.sideFilePath);
  }
}

function requireNullableMarkdownPath(path: KernelWorkspaceRelativePath | null) {
  if (path !== null) requireMarkdownPath(path);
}

function requireMarkdownPath(path: KernelWorkspaceRelativePath) {
  requireWorkspaceRelativePath(path, { allowEmpty: false, markdown: true });
}

function requireWorkspaceRelativePath(
  path: string,
  options: { allowEmpty: boolean; markdown: boolean },
) {
  const segments = path.split("/");
  if (
    (path === "" && !options.allowEmpty) ||
    path.startsWith("/") ||
    path.startsWith("\\") ||
    path.includes("\\") ||
    /^[A-Za-z]:/u.test(path) ||
    /[\u0000-\u001f\u007f-\u009f]/u.test(path) ||
    (path !== "" && segments.some((segment) => segment === "" || segment === "." || segment === "..")) ||
    (options.markdown && !/\.(?:md|markdown)$/iu.test(path))
  ) {
    invalidSnapshot();
  }
}

function invalidSnapshot(): never {
  throw new Error("The Kernel AppConfig snapshot contains an invalid workspace path.");
}

function cloneSnapshot(source: KernelAppConfigSnapshot): KernelAppConfigSnapshot {
  return {
    appConfigVersion: 1,
    localState: {
      fileTreeSort: { ...source.localState.fileTreeSort },
      pandocPath: source.localState.pandocPath,
      recentMarkdownFiles: source.localState.recentMarkdownFiles.map(cloneRecentFile),
      revision: source.localState.revision as KernelRevision,
      uiLayout: {
        openWindows: source.localState.uiLayout.openWindows.map((window) => ({
          filePath: window.filePath,
          label: window.label,
          openFilePaths: [...window.openFilePaths],
        })),
        schemaVersion: 1,
        windowStates: Object.fromEntries(Object.entries(source.localState.uiLayout.windowStates)
          .map(([label, state]) => [label, cloneWindowState(state)])),
      },
    },
    settings: {
      revision: source.settings.revision as KernelRevision,
      values: source.settings.values.map(cloneSettingEntry),
    },
    workspace: {
      generation: source.workspace.generation as KernelWorkspaceGeneration,
      id: source.workspace.id,
    },
  };
}

function cloneWindowState(state: KernelStoredWorkspaceWindowState): KernelStoredWorkspaceWindowState {
  return {
    activeDraftId: state.activeDraftId,
    draftTabs: state.draftTabs.map((draft) => ({ ...draft })),
    filePath: state.filePath,
    fileTreeAssetsVisible: state.fileTreeAssetsVisible,
    fileTreeOpen: state.fileTreeOpen,
    folderName: state.folderName,
    folderPath: state.folderPath,
    openFilePaths: [...state.openFilePaths],
    sideBySideGroup: state.sideBySideGroup === null ? null : { ...state.sideBySideGroup },
  };
}

function cloneRecentFile(file: KernelRecentMarkdownFile): KernelRecentMarkdownFile {
  return { name: file.name, path: file.path };
}

function cloneSettingEntry(entry: KernelSettingEntrySnapshot): KernelSettingEntrySnapshot {
  return entry.value.type === "font-family"
    ? { key: entry.key, value: { type: entry.value.type, value: { ...entry.value.value } } }
    : { key: entry.key, value: { ...entry.value } };
}

function deepFreeze<T>(value: T): T {
  if (typeof value !== "object" || value === null || Object.isFrozen(value)) return value;
  for (const child of Object.values(value)) deepFreeze(child);
  return Object.freeze(value);
}
