import {
  getAppRuntime,
  type NativeSettingsWindowContext,
  type NativeSettingsWindowHideRequest
} from "../../runtime";

export type { NativeSettingsWindowContext, NativeSettingsWindowHideRequest } from "../../runtime";

export type NativeSettingsWindowTarget = "exportPandocPath" | "sync";

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

export function exitNativeApp() {
  return getAppRuntime().window.exitApp();
}

export function openSettingsWindow(
  target?: NativeSettingsWindowTarget,
  projectRoot?: string | null,
  workspaceSourcePath?: string | null
) {
  return getAppRuntime().window.openSettingsWindow(target, projectRoot, workspaceSourcePath);
}

export function markSettingsWindowReady() {
  return getAppRuntime().window.markSettingsWindowReady();
}

export function hideSettingsWindow() {
  return getAppRuntime().window.hideSettingsWindow();
}

export function acknowledgeSettingsWindowHide(generation: number) {
  return getAppRuntime().window.acknowledgeSettingsWindowHide(generation);
}

export function cancelSettingsWindowHide(generation: number) {
  return getAppRuntime().window.cancelSettingsWindowHide(generation);
}

export function completeSettingsWindowHide(generation: number) {
  return getAppRuntime().window.completeSettingsWindowHide(generation);
}

export function listenNativeSettingsWindowHideRequested(
  onHideRequested: (request: NativeSettingsWindowHideRequest) => unknown | Promise<unknown>
) {
  return getAppRuntime().window.listenSettingsWindowHideRequested(onHideRequested);
}

export function listenNativeSettingsWindowTarget(
  onTarget: (target: NativeSettingsWindowTarget) => unknown,
  onContext?: (context: NativeSettingsWindowContext) => unknown
) {
  return getAppRuntime().window.listenSettingsWindowTarget(onTarget, onContext);
}

export function listenNativeAppExitRequested(onExitRequested: () => unknown | Promise<unknown>) {
  return getAppRuntime().window.listenAppExitRequested(onExitRequested);
}

export function listenNativeWindowCloseRequested(
  onCloseRequested: (event: NativeWindowCloseRequestEvent) => unknown | Promise<unknown>
) {
  return getAppRuntime().window.listenWindowCloseRequested(onCloseRequested);
}

export function openNativeExternalUrl(url: string) {
  return getAppRuntime().window.openExternalUrl(url);
}

export function setNativeWindowTitle(title: string) {
  return getAppRuntime().window.setWindowTitle(title);
}

export function setNativeEditorWindowRestoreState(input: SetNativeEditorWindowRestoreStateInput) {
  return getAppRuntime().window.setEditorWindowRestoreState(input);
}

export function listNativeEditorWindowRestoreStates() {
  return getAppRuntime().window.listEditorWindowRestoreStates();
}

export function closeNativeWindow() {
  return getAppRuntime().window.closeWindow();
}

export function destroyNativeWindow() {
  return getAppRuntime().window.destroyWindow();
}

export function minimizeNativeWindow() {
  return getAppRuntime().window.minimizeWindow();
}

export function showNativeWindow() {
  return getAppRuntime().window.showWindow();
}

export function toggleNativeWindowMaximized() {
  return getAppRuntime().window.toggleWindowMaximized();
}

export function toggleNativeWindowFullscreen() {
  return getAppRuntime().window.toggleWindowFullscreen();
}
