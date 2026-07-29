//! Settings service composition boundary.

use async_trait::async_trait;

use crate::{
    contract::{
        ErrorCode, ErrorDetails, PatchSettingsRequest, SettingsSnapshotDto, ValidationField,
        ValidationIssueCode, ValidationIssueDto, ValidationIssues,
    },
    runtime::{ServiceFailure, SettingsApiService},
    settings::service::{SettingsService, SettingsServiceError, SettingsServiceErrorKind},
};

#[async_trait]
impl SettingsApiService for SettingsService {
    async fn get_settings(&self) -> Result<SettingsSnapshotDto, ServiceFailure> {
        self.read_exposed().map_err(service_failure)
    }

    async fn patch_settings(
        &self,
        request: PatchSettingsRequest,
    ) -> Result<SettingsSnapshotDto, ServiceFailure> {
        self.patch_exposed(request).map_err(service_failure)
    }
}

fn service_failure(error: SettingsServiceError) -> ServiceFailure {
    let (code, details) = match error.kind() {
        SettingsServiceErrorKind::InvalidField => (
            ErrorCode::InvalidSettingsField,
            Some(ErrorDetails::Validation {
                issues: ValidationIssues::new(
                    ValidationIssueDto::new(
                        ValidationField::Values,
                        ValidationIssueCode::InvalidFormat,
                    ),
                    [],
                ),
            }),
        ),
        SettingsServiceErrorKind::RevisionConflict => (
            ErrorCode::SettingsRevisionConflict,
            Some(ErrorDetails::RevisionConflict {
                current_revision: error.current_revision().cloned(),
            }),
        ),
        SettingsServiceErrorKind::Unavailable
        | SettingsServiceErrorKind::ReconcileFailed
        | SettingsServiceErrorKind::RecoveryRequired => (ErrorCode::SettingsUnavailable, None),
    };
    ServiceFailure::new(code, details)
        .expect("settings service errors use compatible public error details")
}
