use crate::contract::ErrorCode;
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::mcp::kernel_port::McpKernelFailure;

use super::{failure_from_code, failure_from_kernel, McpServices, ToolResult};

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

pub(super) async fn list(services: &McpServices, cancellation: &CancellationToken) -> ToolResult {
    let (workspace, root_folder_id) = services
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
                .handles
                .issue_folder(primary.workspace_id, primary.workspace_generation, "")
                .map_err(|error| failure_from_code(error.code, None))?;
            Ok((workspace, root_folder_id))
        })
        .map_err(|error| failure_from_code(error.code, None))??;
    let sync_configured = match services.kernel.get_sync_config(cancellation).await {
        Ok(config) => config.configured && config.enabled,
        Err(McpKernelFailure::Api(ErrorCode::SyncConfigAbsent)) => false,
        Err(error) => return Err(failure_from_kernel(error)),
    };
    services
        .workspaces
        .resolve_at_generation(workspace.workspace_id, workspace.workspace_generation)
        .map_err(|error| failure_from_code(error.code, None))?;
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
}
