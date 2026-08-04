#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct McpToolFailure {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_revision: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::McpToolFailure;

    #[test]
    fn safe_failure_uses_stable_camel_case_fields() {
        let failure = McpToolFailure {
            code: "permission_denied",
            message: "Permission denied".to_string(),
            retryable: false,
            recovery_hint: Some("Enable document read access".to_string()),
            current_revision: None,
        };

        let value = serde_json::to_value(failure).expect("failure should serialize");

        assert_eq!(value["code"], "permission_denied");
        assert_eq!(value["recoveryHint"], "Enable document read access");
        assert!(value.get("currentRevision").is_none());
    }
}
