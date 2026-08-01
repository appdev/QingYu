import { invoke } from "@tauri-apps/api/core";

export function readNativePrimaryWorkspaceState() {
  return invoke<unknown | null>("read_primary_workspace_state");
}

export function writeNativePrimaryWorkspaceState(input: {
  expectedState?: unknown;
  state: unknown;
}) {
  return invoke<{ applied: boolean; state: unknown }>("write_primary_workspace_state", { input });
}
