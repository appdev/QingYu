import { invoke } from "@tauri-apps/api/core";
import type { AppMcpRuntime } from "@markra/app/runtime";
import type { McpConfig, McpSettingsSnapshot } from "@markra/app/settings";

type NativeMcpPolicyDocument = {
  config: McpConfig;
  revision: string;
};

function policySettingsSnapshot(document: NativeMcpPolicyDocument): McpSettingsSnapshot {
  return {
    ...document,
    clientCommand: null,
    endpoint: null,
    health: { state: "stopped", endpoint: null, errorCode: null },
    workspace: null
  };
}

export const getNativeMcpPolicySettings: AppMcpRuntime["getSettings"] = async () =>
  policySettingsSnapshot(await invoke<NativeMcpPolicyDocument>("get_mcp_policy"));

export const updateNativeMcpPolicySettings: AppMcpRuntime["updateSettings"] = async (input) =>
  policySettingsSnapshot(await invoke<NativeMcpPolicyDocument>("update_mcp_policy", { input }));
