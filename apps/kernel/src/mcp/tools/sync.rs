
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncProvider {
    S3,
    Webdav,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncMode {
    Automatic,
    StartupExit,
    FullyManual,
}

use std::time::{Duration, Instant};

use crate::contract::{
    CredentialChange, PatchSyncConfigRequest, Revision, RunId, SyncConfigChangesDto,
    SyncConfigViewDto, SyncIntervalSeconds, SyncRunCompletionState, TestSyncConnectionRequest,
    TriggerSyncRunRequest,
};
use rmcp::schemars::JsonSchema;
use tokio_util::sync::CancellationToken;

use crate::mcp::config::SyncExecutionPolicy;


use super::{failure_from_code, failure_from_kernel, McpServices, ToolResult};

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
    #[schemars(with = "Option<String>")]
    pub(super) mode: Option<SyncMode>,
    pub(super) interval_seconds: Option<u32>,
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

pub(super) async fn get_config(
    services: &McpServices,
    _input: SyncConfigGetInput,
    cancellation: &CancellationToken,
) -> ToolResult {
    let config = services
        .kernel
        .get_sync_config(cancellation)
        .await
        .map_err(failure_from_kernel)?;
    let status = services
        .kernel
        .get_sync_status(cancellation)
        .await
        .map_err(failure_from_kernel)?;
    Ok(serde_json::json!({ "config": config, "status": status }))
}

pub(super) async fn update_config(
    services: &McpServices,
    input: &SyncConfigUpdateInput,
    cancellation: &CancellationToken,
) -> ToolResult {
    let changes = SyncConfigChangesDto {
        enabled: input.enabled,
        provider: input.provider.map(convert_enum).transpose()?,
        remote_root: input.remote_root.clone(),
        mode: input.mode.map(convert_enum).transpose()?,
        interval_seconds: input
            .interval_seconds
            .map(|value| {
                SyncIntervalSeconds::new(value)
                    .map_err(|_| failure_from_code("invalid_arguments", None))
            })
            .transpose()?,
        webdav_server_url: input.webdav_server_url.clone(),
        s3_endpoint_url: input.s3_endpoint_url.clone(),
        s3_region: input.s3_region.clone(),
        s3_bucket: input.s3_bucket.clone(),
        ..SyncConfigChangesDto::default()
    };
    patch(services, &input.expected_revision, changes, cancellation).await
}

pub(super) async fn update_credentials(
    services: &McpServices,
    input: &SyncCredentialsUpdateInput,
    cancellation: &CancellationToken,
) -> ToolResult {
    let changes = credential_changes(input)?;
    patch(services, &input.expected_revision, changes, cancellation).await
}

fn credential_changes(
    input: &SyncCredentialsUpdateInput,
) -> Result<SyncConfigChangesDto, crate::mcp::error::McpToolFailure> {
    let clear = input.clear_credentials.unwrap_or(false);
    let provided = [
        input.webdav_username.as_ref(),
        input.webdav_password.as_ref(),
        input.s3_access_key_id.as_ref(),
        input.s3_secret_access_key.as_ref(),
    ];
    if clear && provided.iter().any(|value| value.is_some()) {
        return Err(failure_from_code("invalid_arguments", None));
    }
    if provided.iter().flatten().any(|value| value.is_empty()) {
        return Err(failure_from_code("invalid_arguments", None));
    }
    if !clear && provided.iter().all(|value| value.is_none()) {
        return Err(failure_from_code("invalid_arguments", None));
    }
    if clear {
        return Ok(SyncConfigChangesDto {
            webdav_username: Some(String::new()),
            webdav_password: Some(CredentialChange::Clear {}),
            s3_access_key_id: Some(CredentialChange::Clear {}),
            s3_secret_access_key: Some(CredentialChange::Clear {}),
            ..SyncConfigChangesDto::default()
        });
    }
    let replacement = |value: &Option<String>| {
        value.as_ref().map(|value| CredentialChange::Replace {
            value: value.clone(),
        })
    };
    Ok(SyncConfigChangesDto {
        webdav_username: input.webdav_username.clone(),
        webdav_password: replacement(&input.webdav_password),
        s3_access_key_id: replacement(&input.s3_access_key_id),
        s3_secret_access_key: replacement(&input.s3_secret_access_key),
        ..SyncConfigChangesDto::default()
    })
}

pub(super) async fn test(
    services: &McpServices,
    input: SyncTestInput,
    cancellation: &CancellationToken,
) -> ToolResult {
    let config = services
        .kernel
        .get_sync_config(cancellation)
        .await
        .map_err(failure_from_kernel)?;
    let request = TestSyncConnectionRequest {
        expected_revision: parse_revision(&input.expected_revision)?,
        changes: current_target_changes(&config),
    };
    services
        .kernel
        .test_sync_connection(&request, cancellation)
        .await
        .and_then(|result| {
            serde_json::to_value(result)
                .map_err(|_| crate::mcp::kernel_port::McpKernelFailure::InvalidResponse)
        })
        .map_err(failure_from_kernel)
}

pub(super) async fn run(
    services: &McpServices,
    input: &SyncRunInput,
    execution: SyncExecutionPolicy,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> ToolResult {
    let accepted = services
        .kernel
        .trigger_sync_run(
            &TriggerSyncRunRequest {
                expected_config_revision: parse_revision(&input.expected_revision)?,
            },
            cancellation,
        )
        .await
        .map_err(failure_from_kernel)?;
    if execution == SyncExecutionPolicy::Background {
        return serde_json::to_value(accepted)
            .map_err(|_| failure_from_code("operation_failed", None));
    }
    let deadline = Instant::now() + timeout;
    loop {
        let status = services
            .kernel
            .get_sync_run(accepted.run_id, cancellation)
            .await
            .map_err(failure_from_kernel)?;
        if status.completion_state != SyncRunCompletionState::Attempting {
            return serde_json::to_value(status)
                .map_err(|_| failure_from_code("operation_failed", None));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(failure_from_code("sync_wait_timeout", None));
        }
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err(failure_from_code("request_cancelled", None));
            }
            _ = tokio::time::sleep(remaining.min(Duration::from_millis(100))) => {}
        }
    }
}

pub(super) async fn status(
    services: &McpServices,
    input: SyncStatusInput,
    cancellation: &CancellationToken,
) -> ToolResult {
    match input.run_id {
        Some(run_id) => services
            .kernel
            .get_sync_run(RunId::new(run_id), cancellation)
            .await
            .and_then(|status| {
                serde_json::to_value(status)
                    .map_err(|_| crate::mcp::kernel_port::McpKernelFailure::InvalidResponse)
            })
            .map_err(failure_from_kernel),
        None => services
            .kernel
            .get_sync_status(cancellation)
            .await
            .and_then(|status| {
                serde_json::to_value(status)
                    .map_err(|_| crate::mcp::kernel_port::McpKernelFailure::InvalidResponse)
            })
            .map(|status| serde_json::json!({ "status": status }))
            .map_err(failure_from_kernel),
    }
}

async fn patch(
    services: &McpServices,
    expected_revision: &str,
    changes: SyncConfigChangesDto,
    cancellation: &CancellationToken,
) -> ToolResult {
    changes
        .validate()
        .map_err(|_| failure_from_code("invalid_arguments", None))?;
    let request = PatchSyncConfigRequest {
        expected_revision: parse_revision(expected_revision)?,
        changes,
    };
    services
        .kernel
        .patch_sync_config(&request, cancellation)
        .await
        .and_then(|config| {
            serde_json::to_value(config)
                .map_err(|_| crate::mcp::kernel_port::McpKernelFailure::InvalidResponse)
        })
        .map_err(failure_from_kernel)
}

fn parse_revision(value: &str) -> Result<Revision, crate::mcp::error::McpToolFailure> {
    Revision::parse(value.to_owned()).map_err(|_| failure_from_code("invalid_arguments", None))
}

fn convert_enum<T: Serialize, U: serde::de::DeserializeOwned>(
    value: T,
) -> Result<U, crate::mcp::error::McpToolFailure> {
    serde_json::from_value(
        serde_json::to_value(value).map_err(|_| failure_from_code("invalid_arguments", None))?,
    )
    .map_err(|_| failure_from_code("invalid_arguments", None))
}

fn current_target_changes(config: &SyncConfigViewDto) -> SyncConfigChangesDto {
    SyncConfigChangesDto {
        provider: Some(config.provider),
        remote_root: Some(config.remote_root.clone()),
        ..SyncConfigChangesDto::default()
    }
}

#[cfg(test)]
mod tests {
    use crate::contract::CredentialChange;

    use super::{credential_changes, SyncCredentialsUpdateInput};

    fn input() -> SyncCredentialsUpdateInput {
        SyncCredentialsUpdateInput {
            expected_revision: "sync-1".to_string(),
            webdav_username: None,
            webdav_password: None,
            s3_access_key_id: None,
            s3_secret_access_key: None,
            clear_credentials: None,
            dry_run: None,
            preview_token: None,
        }
    }

    #[test]
    fn credential_patch_rejects_empty_noop_and_mixed_clear_inputs() {
        let error = credential_changes(&input()).expect_err("empty credential patch");
        assert_eq!(error.code, "invalid_arguments");

        let mut empty_replacement = input();
        empty_replacement.webdav_password = Some(String::new());
        let error = credential_changes(&empty_replacement).expect_err("empty replacement");
        assert_eq!(error.code, "invalid_arguments");

        let mut mixed_clear = input();
        mixed_clear.clear_credentials = Some(true);
        mixed_clear.s3_access_key_id = Some("access-key".to_string());
        let error = credential_changes(&mixed_clear).expect_err("mixed clear and replacement");
        assert_eq!(error.code, "invalid_arguments");
    }

    #[test]
    fn credential_clear_covers_username_and_every_secret() {
        let mut clear = input();
        clear.clear_credentials = Some(true);

        let changes = credential_changes(&clear).expect("clear credential changes");

        assert_eq!(changes.webdav_username.as_deref(), Some(""));
        assert_eq!(changes.webdav_password, Some(CredentialChange::Clear {}));
        assert_eq!(changes.s3_access_key_id, Some(CredentialChange::Clear {}));
        assert_eq!(
            changes.s3_secret_access_key,
            Some(CredentialChange::Clear {})
        );
    }

    #[test]
    fn credential_replacements_preserve_each_non_empty_field() {
        let mut replace = input();
        replace.webdav_username = Some("user".to_string());
        replace.webdav_password = Some("password".to_string());
        replace.s3_access_key_id = Some("access-key".to_string());
        replace.s3_secret_access_key = Some("secret-key".to_string());

        let changes = credential_changes(&replace).expect("replacement credential changes");

        assert_eq!(changes.webdav_username.as_deref(), Some("user"));
        assert_eq!(
            changes.webdav_password,
            Some(CredentialChange::Replace {
                value: "password".to_string()
            })
        );
        assert_eq!(
            changes.s3_access_key_id,
            Some(CredentialChange::Replace {
                value: "access-key".to_string()
            })
        );
        assert_eq!(
            changes.s3_secret_access_key,
            Some(CredentialChange::Replace {
                value: "secret-key".to_string()
            })
        );
    }
}
