import { invoke } from "@tauri-apps/api/core";
import type { AppSettingsGroup } from "@markra/app/runtime";

export async function readNativeAppSettingsGroup<TValue>(group: AppSettingsGroup) {
  const value = await invoke<TValue | null>("read_app_settings_group", { group });

  return value ?? undefined;
}

export function writeNativeAppSettingsGroup(group: AppSettingsGroup, value: unknown) {
  return invoke("write_app_settings_group", { group, value });
}

export function replaceNativePortableAppSettings(settings: unknown) {
  return invoke("replace_portable_app_settings", { settings });
}

export function readNativePrimaryWorkspaceState() {
  return invoke<unknown | null>("read_primary_workspace_state");
}

export function writeNativePrimaryWorkspaceState(input: {
  expectedState?: unknown;
  state: unknown;
}) {
  return invoke<{ applied: boolean; state: unknown }>("write_primary_workspace_state", { input });
}
