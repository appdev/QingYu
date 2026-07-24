use std::collections::BTreeMap;

use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::app_settings::{AppSettingsService, ExposedSettingsPatch};

use super::{failure_from_code, McpServices, ToolResult};

#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SettingsGetInput {}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct SettingsUpdateInput {
    pub(super) expected_revision: String,
    pub(super) values: BTreeMap<String, serde_json::Value>,
    pub(super) dry_run: Option<bool>,
    pub(super) preview_token: Option<String>,
}

pub(super) fn get(services: &McpServices) -> ToolResult {
    let exposed = services
        .settings
        .read_exposed()
        .map_err(|error| failure_from_code(error.code, None))?;
    Ok(serde_json::json!({
        "fields": AppSettingsService::exposed_field_names(),
        "revision": exposed.revision,
        "values": exposed.values,
        "credentialsPresent": exposed.credentials_present,
    }))
}

pub(super) fn update(services: &McpServices, input: &SettingsUpdateInput) -> ToolResult {
    services
        .settings
        .patch_exposed(ExposedSettingsPatch {
            expected_revision: input.expected_revision.clone(),
            values: input.values.clone(),
        })
        .map(|exposed| {
            serde_json::json!({
                "revision": exposed.revision,
                "values": exposed.values,
                "credentialsPresent": exposed.credentials_present,
            })
        })
        .map_err(|error| failure_from_code(error.code, None))
}
