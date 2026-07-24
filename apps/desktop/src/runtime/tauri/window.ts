import { invokeNative } from "./invoke";
import { exit } from "@tauri-apps/plugin-process";
import { listenNativeEvent, safeNativeEventCleanup } from "./events";

export type NativeSettingsWindowTarget = "exportPandocPath" | "sync";

export type NativeSettingsWindowContext = {
  projectRoot: string | null;
  sourceWindowLabel: string | null;
  workspaceSourcePath: string | null;
};

export type NativeEditorWindowRestoreState = {
  filePath: string | null;
  label: string;
  openFilePaths: string[];
};

export type SetNativeEditorWindowRestoreStateInput = {
  filePath: string | null;
  openFilePaths: string[];
};

export type NativeWindowCloseRequestEvent = {
  preventDefault: () => unknown;
};

type NativeSettingsWindowTargetPayload = {
  projectRoot?: unknown;
  sourceWindowLabel?: unknown;
  target?: unknown;
  workspaceSourcePath?: unknown;
};

export type NativeSettingsWindowHideRequest = {
  generation: number;
};

type NativeSettingsWindowHideRequestPayload = {
  generation?: unknown;
};

const nativeSettingsWindowTargetEvent = "markra://settings-window-target";
const nativeSettingsWindowHideRequestedEvent = "qingyu://settings-hide-requested";
const nativeAppExitRequestedEvent = "markra://app-exit-requested";

function isNativeSettingsWindowTarget(value: unknown): value is NativeSettingsWindowTarget {
  return value === "exportPandocPath" || value === "sync";
}

export function openSettingsWindow(
  target?: NativeSettingsWindowTarget,
  projectRoot?: string | null,
  workspaceSourcePath?: string | null
) {
  return invokeNative("open_settings_window", {
    projectRoot: projectRoot ?? null,
    target: target ?? null,
    workspaceSourcePath: workspaceSourcePath ?? null
  });
}

export function requestNativePrimaryCloudNotebookCatalog() {
  return invokeNative("request_primary_cloud_notebook_catalog");
}

export function markSettingsWindowReady() {
  return invokeNative("mark_settings_window_ready");
}

export function hideSettingsWindow() {
  return invokeNative("hide_settings_window");
}

export function acknowledgeSettingsWindowHide(generation: number) {
  return invokeNative("acknowledge_settings_window_hide", { generation });
}

export function cancelSettingsWindowHide(generation: number) {
  return invokeNative("cancel_settings_window_hide", { generation });
}

export function completeSettingsWindowHide(generation: number) {
  return invokeNative("complete_settings_window_hide", { generation });
}

export async function listenNativeSettingsWindowHideRequested(
  onHideRequested: (request: NativeSettingsWindowHideRequest) => unknown | Promise<unknown>
) {
  if (!("__TAURI_INTERNALS__" in window)) {
    return () => {};
  }

  return listenNativeEvent<NativeSettingsWindowHideRequestPayload>(
    nativeSettingsWindowHideRequestedEvent,
    (event) => {
      const generation = event.payload.generation;
      if (
        typeof generation === "number" &&
        Number.isSafeInteger(generation) &&
        generation > 0
      ) {
        onHideRequested({ generation });
      }
    }
  );
}

export async function listenNativeSettingsWindowTarget(
  onTarget: (target: NativeSettingsWindowTarget) => unknown,
  onContext?: (context: NativeSettingsWindowContext) => unknown
) {
  if (!("__TAURI_INTERNALS__" in window)) {
    return () => {};
  }

  return listenNativeEvent<NativeSettingsWindowTargetPayload>(nativeSettingsWindowTargetEvent, (event) => {
    if (isNativeSettingsWindowTarget(event.payload.target)) {
      onTarget(event.payload.target);
    }
    const hasContext = ["projectRoot", "sourceWindowLabel", "workspaceSourcePath"].some((key) =>
      Object.prototype.hasOwnProperty.call(event.payload, key)
    );
    if (!hasContext) return;

    const nullableString = (value: unknown) =>
      typeof value === "string" && value.trim() ? value.trim() : null;
    onContext?.({
      projectRoot: nullableString(event.payload.projectRoot),
      sourceWindowLabel: nullableString(event.payload.sourceWindowLabel),
      workspaceSourcePath: nullableString(event.payload.workspaceSourcePath)
    });
  });
}

export async function listenNativeAppExitRequested(onExitRequested: () => unknown | Promise<unknown>) {
  if (!("__TAURI_INTERNALS__" in window)) {
    return () => {};
  }

  return listenNativeEvent(nativeAppExitRequestedEvent, () => onExitRequested());
}

export async function setNativeWindowTitle(title: string) {
  window.document.title = title;

  if (!("__TAURI_INTERNALS__" in window)) {
    return;
  }

  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().setTitle(title);
}

async function getCurrentNativeWindow() {
  if (!("__TAURI_INTERNALS__" in window)) {
    return null;
  }

  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

export async function getCurrentNativeWindowLabel() {
  return (await getCurrentNativeWindow())?.label ?? null;
}

function normalizeNativeEditorWindowRestoreStates(value: unknown): NativeEditorWindowRestoreState[] {
  if (!Array.isArray(value)) return [];

  return value.flatMap((item) => {
    if (typeof item !== "object" || item === null) return [];

    const candidate = item as Partial<NativeEditorWindowRestoreState>;
    const label = candidate.label?.trim();
    const trimmedFilePath = typeof candidate.filePath === "string" ? candidate.filePath.trim() : "";
    const filePath = trimmedFilePath
      ? trimmedFilePath
      : null;
    const openFilePaths = Array.isArray(candidate.openFilePaths)
      ? candidate.openFilePaths.flatMap((path) => {
        if (typeof path !== "string") return [];

        const trimmedPath = path.trim();
        return trimmedPath ? [trimmedPath] : [];
      })
      : [];

    if (!label || (!filePath && openFilePaths.length === 0)) return [];

    return [{
      filePath,
      label,
      openFilePaths
    }];
  });
}

export async function setNativeEditorWindowRestoreState(input: SetNativeEditorWindowRestoreStateInput) {
  if (!("__TAURI_INTERNALS__" in window)) {
    return;
  }

  await invokeNative("set_editor_window_restore_state", input);
}

export async function listNativeEditorWindowRestoreStates() {
  if (!("__TAURI_INTERNALS__" in window)) {
    return [];
  }

  return normalizeNativeEditorWindowRestoreStates(await invokeNative("list_editor_window_restore_states"));
}

export async function exitNativeApp() {
  if (!("__TAURI_INTERNALS__" in window)) {
    window.close();
    return;
  }

  await exit(0);
}

export async function listenNativeWindowCloseRequested(
  onCloseRequested: (event: NativeWindowCloseRequestEvent) => unknown | Promise<unknown>
) {
  const currentWindow = await getCurrentNativeWindow();
  if (!currentWindow) return () => {};

  return safeNativeEventCleanup(await currentWindow.onCloseRequested(async (event) => {
    await onCloseRequested({
      preventDefault: () => event.preventDefault()
    });
  }));
}

export async function closeNativeWindow() {
  const currentWindow = await getCurrentNativeWindow();
  await currentWindow?.close();
}

export async function destroyNativeWindow() {
  if (!("__TAURI_INTERNALS__" in window)) {
    window.close();
    return;
  }

  await invokeNative("destroy_current_editor_window");
}

export async function minimizeNativeWindow() {
  if (!("__TAURI_INTERNALS__" in window)) {
    return;
  }

  await invokeNative("minimize_current_window");
}

export async function showNativeWindow() {
  const currentWindow = await getCurrentNativeWindow();
  if (!currentWindow) return;

  if (!(await currentWindow.isVisible())) {
    await currentWindow.show();
  }
  try {
    await currentWindow.setFocus();
  } catch {
    // Focusing is best-effort; showing the window is the startup-critical action.
  }
}

export async function toggleNativeWindowMaximized() {
  const currentWindow = await getCurrentNativeWindow();
  await currentWindow?.toggleMaximize();
}

export async function toggleNativeWindowFullscreen() {
  const currentWindow = await getCurrentNativeWindow();
  if (!currentWindow) return;

  await currentWindow.setFullscreen(!(await currentWindow.isFullscreen()));
}
