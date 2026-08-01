//! App configuration service composition boundary.

use async_trait::async_trait;

use crate::{
    app_config::{AppConfigService, AppConfigServiceError, AppConfigServiceErrorKind},
    contract::{AppConfigSnapshotDto, ErrorCode, PatchAppConfigStateRequest},
    runtime::{AppConfigApiService, ServiceFailure},
};

#[async_trait]
impl AppConfigApiService for AppConfigService {
    async fn get_app_config(&self) -> Result<AppConfigSnapshotDto, ServiceFailure> {
        self.read().map_err(service_failure)
    }

    async fn patch_app_config_state(
        &self,
        request: PatchAppConfigStateRequest,
    ) -> Result<AppConfigSnapshotDto, ServiceFailure> {
        self.patch_state(request).map_err(service_failure)
    }
}

fn service_failure(error: AppConfigServiceError) -> ServiceFailure {
    let code = match error.kind() {
        AppConfigServiceErrorKind::InvalidAppConfigState => ErrorCode::InvalidAppConfigState,
        AppConfigServiceErrorKind::StaleWorkspaceGeneration => ErrorCode::WorkspaceGenerationStale,
        AppConfigServiceErrorKind::Unavailable | AppConfigServiceErrorKind::RecoveryRequired => {
            ErrorCode::AppConfigUnavailable
        }
    };
    ServiceFailure::new(code, None)
        .expect("app configuration service errors use compatible public error details")
}
