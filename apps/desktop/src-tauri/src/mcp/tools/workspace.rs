use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{failure_from_code, McpServices, ToolResult};

#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceListInput {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceListEntry {
    workspace_id: uuid::Uuid,
    workspace_generation: u64,
    display_name: String,
    leaf_name: String,
    available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    root_folder_id: Option<String>,
    sync_configured: bool,
}

pub(super) fn list(services: &McpServices) -> ToolResult {
    services
        .workspaces
        .with_authority(|| {
            let primary = services
                .workspaces
                .require_primary_workspace()
                .map_err(|error| failure_from_code(error.code, None))?;
            let workspace = services
                .workspaces
                .list_safe()
                .into_iter()
                .find(|workspace| workspace.workspace_id == primary.workspace_id)
                .ok_or_else(|| failure_from_code("mcp-workspace-unavailable", None))?;
            let root_folder_id = services
                .documents
                .root_folder_id(&primary)
                .map_err(|error| failure_from_code(error.code, None))?;
            let sync_configured = services.sync.sync_enabled_for_authoritative_primary();
            let workspaces = vec![WorkspaceListEntry {
                workspace_id: workspace.workspace_id,
                workspace_generation: workspace.workspace_generation,
                display_name: workspace.display_name,
                leaf_name: workspace.leaf_name,
                available: workspace.available,
                root_folder_id: Some(root_folder_id),
                sync_configured,
            }];
            serde_json::to_value(serde_json::json!({ "workspaces": workspaces }))
                .map_err(|_| failure_from_code("response_too_large", None))
        })
        .map_err(|error| failure_from_code(error.code, None))?
}
