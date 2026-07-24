import { invoke } from "@tauri-apps/api/core";
import type { AppMcpRuntime } from "@markra/app/runtime";

export const getNativeMcpSettings: AppMcpRuntime["getSettings"] = () =>
  invoke("get_mcp_settings");

export const updateNativeMcpSettings: AppMcpRuntime["updateSettings"] = (input) =>
  invoke("update_mcp_settings", { input });

export const setNativeMcpPrimaryWorkspace: AppMcpRuntime["setPrimaryWorkspace"] = (input) =>
  invoke("set_mcp_primary_workspace", input);

export const getNativeMcpHealth: AppMcpRuntime["getHealth"] = () =>
  invoke("get_mcp_health");

export const listNativeMcpAuditEntries: AppMcpRuntime["listAuditEntries"] = (offset, limit) =>
  invoke("list_mcp_audit_entries", { offset, limit });

export const clearNativeMcpAuditEntries: AppMcpRuntime["clearAuditEntries"] = () =>
  invoke("clear_mcp_audit_entries");
