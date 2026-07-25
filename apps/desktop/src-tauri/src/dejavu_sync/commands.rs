use std::sync::OnceLock;

use super::service::{AcceptedSyncJob, DejavuSyncService, RepositoryJobError, SyncJobRequest};

#[derive(Default)]
pub(crate) struct DejavuSyncServiceOwner {
    service: OnceLock<DejavuSyncService>,
}

impl DejavuSyncServiceOwner {
    #[allow(dead_code)]
    pub(crate) fn install(&self, service: DejavuSyncService) -> Result<(), RepositoryJobError> {
        self.service
            .set(service)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    #[allow(dead_code)]
    pub(crate) async fn enqueue(
        &self,
        request: SyncJobRequest,
    ) -> Result<AcceptedSyncJob, RepositoryJobError> {
        self.service
            .get()
            .ok_or(RepositoryJobError::RepositoryUnavailable)?
            .enqueue(request)
            .await
    }

    #[allow(dead_code)]
    pub(crate) async fn cancel_all_for_shutdown_or_reset(&self) {
        if let Some(service) = self.service.get() {
            service.cancel_all_for_shutdown_or_reset().await;
        }
    }
}
