use std::collections::BTreeMap;

use qingyu_kernel::contract::{PatchSettingsRequest, Revision, SettingEntryDto, SettingValueDto};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::{failure_from_code, failure_from_kernel, McpServices, ToolResult};

const EXPOSED_FIELDS: [&str; 24] = [
    "appearance.mode",
    "appearance.lightTheme",
    "appearance.darkTheme",
    "language",
    "editor.bodyFontSize",
    "editor.contentWidth",
    "editor.contentWidthPx",
    "editor.fontFamily",
    "editor.lineHeight",
    "editor.paragraphSpacingPx",
    "editor.showWordCount",
    "editor.wrapCodeBlocks",
    "editor.viewMode",
    "files.ignoreRules",
    "export.fontFamily",
    "export.pdfAuthor",
    "export.pdfFooter",
    "export.pdfHeader",
    "export.pdfHeightMm",
    "export.pdfMarginMm",
    "export.pdfMarginPreset",
    "export.pdfPageBreakOnH1",
    "export.pdfPageSize",
    "export.pdfWidthMm",
];

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

pub(super) async fn get(services: &McpServices, cancellation: &CancellationToken) -> ToolResult {
    let snapshot = services
        .kernel
        .get_settings(cancellation)
        .await
        .map_err(failure_from_kernel)?;
    snapshot_value(snapshot)
}

pub(super) async fn update(
    services: &McpServices,
    input: &SettingsUpdateInput,
    cancellation: &CancellationToken,
) -> ToolResult {
    let values = input
        .values
        .iter()
        .map(|(key, value)| setting_entry(key, value.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    let request = PatchSettingsRequest {
        expected_revision: Revision::parse(input.expected_revision.clone())
            .map_err(|_| failure_from_code("invalid_arguments", None))?,
        values,
    };
    request
        .validate()
        .map_err(|_| failure_from_code("invalid_arguments", None))?;
    let snapshot = services
        .kernel
        .patch_settings(&request, cancellation)
        .await
        .map_err(failure_from_kernel)?;
    snapshot_value(snapshot)
}

fn snapshot_value(snapshot: qingyu_kernel::contract::SettingsSnapshotDto) -> ToolResult {
    let values = snapshot
        .values
        .into_iter()
        .map(|entry| {
            let key = serde_json::to_value(entry.key)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(|| failure_from_code("operation_failed", None))?;
            Ok((key, raw_setting_value(entry.value)?))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    Ok(serde_json::json!({
        "fields": EXPOSED_FIELDS,
        "revision": snapshot.revision.as_str(),
        "values": values,
        "credentialsPresent": false,
    }))
}

fn raw_setting_value(
    value: SettingValueDto,
) -> Result<serde_json::Value, crate::mcp::error::McpToolFailure> {
    let encoded =
        serde_json::to_value(value).map_err(|_| failure_from_code("operation_failed", None))?;
    let object = encoded
        .as_object()
        .ok_or_else(|| failure_from_code("operation_failed", None))?;
    object
        .get("value")
        .cloned()
        .ok_or_else(|| failure_from_code("operation_failed", None))
}

fn setting_entry(
    key: &str,
    value: serde_json::Value,
) -> Result<SettingEntryDto, crate::mcp::error::McpToolFailure> {
    if !EXPOSED_FIELDS.contains(&key) {
        return Err(failure_from_code("invalid_settings_field", None));
    }
    let value_type = match key {
        "editor.showWordCount" | "editor.wrapCodeBlocks" | "export.pdfPageBreakOnH1" => "boolean",
        "editor.bodyFontSize"
        | "editor.paragraphSpacingPx"
        | "export.pdfHeightMm"
        | "export.pdfMarginMm"
        | "export.pdfWidthMm" => "integer",
        "editor.lineHeight" => "number",
        "editor.contentWidthPx" => "nullable-integer",
        "export.fontFamily" => "nullable-string",
        "editor.fontFamily" => "font-family",
        _ => "string",
    };
    serde_json::from_value(serde_json::json!({
        "key": key,
        "value": { "type": value_type, "value": value },
    }))
    .map_err(|_| failure_from_code("invalid_arguments", None))
}
