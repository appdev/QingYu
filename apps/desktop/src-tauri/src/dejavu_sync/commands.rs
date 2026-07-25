use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use super::scheduler::DejavuScheduler;
use super::service::{AcceptedSyncJob, DejavuSyncService, RepositoryJobError, SyncJobRequest};

#[derive(Default)]
pub(crate) struct DejavuSyncServiceOwner {
    service: OnceLock<DejavuSyncService>,
}

#[derive(Default)]
pub(crate) struct DejavuSchedulerOwner {
    scheduler: OnceLock<DejavuScheduler>,
    exit_triggered: AtomicBool,
}

impl DejavuSchedulerOwner {
    #[allow(dead_code)]
    pub(crate) fn install(&self, scheduler: DejavuScheduler) -> Result<(), RepositoryJobError> {
        self.scheduler
            .set(scheduler)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) fn activate_root(&self, root: &Path) -> bool {
        self.scheduler
            .get()
            .is_some_and(|scheduler| scheduler.activate_root(root).unwrap_or(false))
    }

    pub(crate) fn deactivate_root(&self, root: &Path) -> bool {
        self.scheduler
            .get()
            .is_some_and(|scheduler| scheduler.deactivate_root(root))
    }

    pub(crate) fn record_file_change(&self, root: &Path, path: &Path) -> bool {
        self.scheduler
            .get()
            .is_some_and(|scheduler| scheduler.record_file_change(root, path).unwrap_or(false))
    }

    pub(crate) fn trigger_startup(&self) {
        let Some(scheduler) = self.scheduler.get().cloned() else {
            return;
        };
        tauri::async_runtime::spawn(async move {
            let _accepted = scheduler.trigger_startup().await;
        });
    }

    pub(crate) fn trigger_exit(&self) {
        let Some(scheduler) = self.scheduler.get().cloned() else {
            return;
        };
        if self
            .exit_triggered
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        tauri::async_runtime::spawn(async move {
            let _accepted = scheduler.trigger_exit().await;
        });
    }
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::DejavuSchedulerOwner;

    #[test]
    fn uninstalled_scheduler_owner_is_safe_for_all_native_call_sites() {
        let owner = DejavuSchedulerOwner::default();
        assert!(!owner.activate_root(Path::new("/notes/uninstalled")));
        assert!(!owner.record_file_change(
            Path::new("/notes/uninstalled"),
            Path::new("/notes/uninstalled/note.md"),
        ));
        assert!(!owner.deactivate_root(Path::new("/notes/uninstalled")));
        owner.trigger_startup();
        owner.trigger_exit();
    }
}
