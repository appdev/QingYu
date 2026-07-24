use std::time::Duration;

use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    mcp::config::SyncExecutionPolicy,
    remote_sync::mcp_service::{SyncConfigPatchInput, SyncCredentialPatchInput},
    sync_config::model::SyncProvider,
};

use super::{failure_from_code, McpServices, ToolResult};

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct SyncConfigGetInput {}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct SyncConfigUpdateInput {
    pub(super) expected_revision: String,
    pub(super) enabled: Option<bool>,
    #[schemars(with = "Option<String>")]
    pub(super) provider: Option<SyncProvider>,
    pub(super) remote_root: Option<String>,
    pub(super) auto_sync_on_save: Option<bool>,
    pub(super) interval_minutes: Option<u32>,
    pub(super) webdav_server_url: Option<String>,
    pub(super) s3_endpoint_url: Option<String>,
    pub(super) s3_region: Option<String>,
    pub(super) s3_bucket: Option<String>,
    pub(super) dry_run: Option<bool>,
    pub(super) preview_token: Option<String>,
}

impl SyncConfigUpdateInput {
    pub(super) fn changes_remote_target(&self) -> bool {
        self.provider.is_some()
            || self.remote_root.is_some()
            || self.webdav_server_url.is_some()
            || self.s3_endpoint_url.is_some()
            || self.s3_region.is_some()
            || self.s3_bucket.is_some()
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct SyncCredentialsUpdateInput {
    pub(super) expected_revision: String,
    pub(super) webdav_username: Option<String>,
    pub(super) webdav_password: Option<String>,
    pub(super) s3_access_key_id: Option<String>,
    pub(super) s3_secret_access_key: Option<String>,
    pub(super) clear_credentials: Option<bool>,
    pub(super) dry_run: Option<bool>,
    pub(super) preview_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct SyncTestInput {
    pub(super) expected_revision: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct SyncRunInput {
    pub(super) expected_revision: String,
    pub(super) dry_run: Option<bool>,
    pub(super) preview_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct SyncStatusInput {
    #[schemars(with = "Option<String>")]
    pub(super) run_id: Option<uuid::Uuid>,
}

pub(super) fn get_config(services: &McpServices, _input: SyncConfigGetInput) -> ToolResult {
    let config = services
        .sync
        .get_config()
        .map_err(|error| failure_from_code(error.code, None))?;
    let persisted_status = services
        .sync
        .persisted_status()
        .map_err(|error| failure_from_code(error.code, None))?;
    Ok(serde_json::json!({
        "config": config,
        "status": persisted_status,
    }))
}

pub(super) fn update_config(services: &McpServices, input: &SyncConfigUpdateInput) -> ToolResult {
    services
        .sync
        .update_config(SyncConfigPatchInput {
            expected_revision: input.expected_revision.clone(),
            enabled: input.enabled,
            provider: input.provider,
            remote_root: input.remote_root.clone(),
            auto_sync_on_save: input.auto_sync_on_save,
            interval_minutes: input.interval_minutes,
            webdav_server_url: input.webdav_server_url.clone(),
            s3_endpoint_url: input.s3_endpoint_url.clone(),
            s3_region: input.s3_region.clone(),
            s3_bucket: input.s3_bucket.clone(),
        })
        .and_then(|config| serde_json::to_value(config).map_err(|_| unreachable!()))
        .map_err(|error| failure_from_code(error.code, None))
}

pub(super) fn update_credentials(
    services: &McpServices,
    input: &SyncCredentialsUpdateInput,
) -> ToolResult {
    services
        .sync
        .update_credentials(SyncCredentialPatchInput {
            expected_revision: input.expected_revision.clone(),
            webdav_username: input.webdav_username.clone(),
            webdav_password: input.webdav_password.clone(),
            s3_access_key_id: input.s3_access_key_id.clone(),
            s3_secret_access_key: input.s3_secret_access_key.clone(),
            clear_credentials: input.clear_credentials,
        })
        .and_then(|config| serde_json::to_value(config).map_err(|_| unreachable!()))
        .map_err(|error| failure_from_code(error.code, None))
}

pub(super) async fn test(services: &McpServices, input: SyncTestInput) -> ToolResult {
    services
        .sync
        .test(&input.expected_revision)
        .await
        .and_then(|result| serde_json::to_value(result).map_err(|_| unreachable!()))
        .map_err(|error| failure_from_code(error.code, None))
}

pub(super) async fn run(
    services: &McpServices,
    input: &SyncRunInput,
    execution: SyncExecutionPolicy,
    timeout: Duration,
) -> ToolResult {
    services
        .sync
        .run(&input.expected_revision, execution, timeout)
        .await
        .and_then(|status| serde_json::to_value(status).map_err(|_| unreachable!()))
        .map_err(|error| failure_from_code(error.code, None))
}

pub(super) fn status(services: &McpServices, input: SyncStatusInput) -> ToolResult {
    match input.run_id {
        Some(run_id) => services
            .sync
            .status(run_id)
            .and_then(|status| serde_json::to_value(status).map_err(|_| unreachable!()))
            .map_err(|error| failure_from_code(error.code, None)),
        None => services
            .sync
            .persisted_status()
            .map(|status| serde_json::json!({ "status": status }))
            .map_err(|error| failure_from_code(error.code, None)),
    }
}
