use std::any::Any;
use std::future::Future;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkingTreeChange {
    Write {
        path: String,
        planned_revision: Option<String>,
    },
    Delete {
        path: String,
        planned_revision: Option<String>,
    },
}

pub struct WorkingTreePermit {
    token: Option<Box<dyn Any + Send + Sync>>,
}

impl WorkingTreePermit {
    pub fn new<T>(token: T) -> Self
    where
        T: Any + Send + Sync,
    {
        Self {
            token: Some(Box::new(token)),
        }
    }

    pub fn token<T: Any>(&self) -> Option<&T> {
        self.token.as_ref()?.downcast_ref()
    }

    fn noop() -> Self {
        Self { token: None }
    }
}

#[async_trait::async_trait]
pub trait WorkingTreeCoordinator: Send + Sync {
    async fn prepare(
        &self,
        changes: &[WorkingTreeChange],
    ) -> Result<WorkingTreePermit, crate::RepoError>;
    async fn release(&self, permit: WorkingTreePermit);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopWorkingTreeCoordinator;

#[async_trait::async_trait]
impl WorkingTreeCoordinator for NoopWorkingTreeCoordinator {
    async fn prepare(
        &self,
        _changes: &[WorkingTreeChange],
    ) -> Result<WorkingTreePermit, crate::RepoError> {
        Ok(WorkingTreePermit::noop())
    }

    async fn release(&self, _permit: WorkingTreePermit) {}
}

pub async fn with_working_tree_permit<T, F, Fut>(
    coordinator: Arc<dyn WorkingTreeCoordinator>,
    changes: &[WorkingTreeChange],
    operation: F,
) -> Result<T, crate::RepoError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, crate::RepoError>>,
{
    if changes.is_empty() {
        return operation().await;
    }

    let permit = coordinator.prepare(changes).await?;
    let scope = WorkingTreePermitScope {
        coordinator,
        permit: Some(permit),
        runtime: tokio::runtime::Handle::current(),
    };
    let result = operation().await;
    scope.release().await;
    result
}

struct WorkingTreePermitScope {
    coordinator: Arc<dyn WorkingTreeCoordinator>,
    permit: Option<WorkingTreePermit>,
    runtime: tokio::runtime::Handle,
}

impl WorkingTreePermitScope {
    async fn release(mut self) {
        if let Some(permit) = self.permit.take() {
            let coordinator = Arc::clone(&self.coordinator);
            let release = self
                .runtime
                .spawn(async move { coordinator.release(permit).await });
            let _release_result = release.await;
        }
    }
}

impl Drop for WorkingTreePermitScope {
    fn drop(&mut self) {
        if let Some(permit) = self.permit.take() {
            let coordinator = Arc::clone(&self.coordinator);
            drop(
                self.runtime
                    .spawn(async move { coordinator.release(permit).await }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use tokio::sync::Notify;

    use crate::RepoError;

    use super::{
        with_working_tree_permit, NoopWorkingTreeCoordinator, WorkingTreeChange,
        WorkingTreeCoordinator, WorkingTreePermit,
    };

    #[derive(Default)]
    struct RecordingCoordinator {
        prepares: AtomicUsize,
        releases: Mutex<Vec<usize>>,
        prepare_error: bool,
    }

    impl RecordingCoordinator {
        fn failing() -> Self {
            Self {
                prepare_error: true,
                ..Self::default()
            }
        }

        fn prepare_count(&self) -> usize {
            self.prepares.load(Ordering::SeqCst)
        }

        fn released(&self) -> Vec<usize> {
            self.releases.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl WorkingTreeCoordinator for RecordingCoordinator {
        async fn prepare(
            &self,
            _changes: &[WorkingTreeChange],
        ) -> Result<WorkingTreePermit, RepoError> {
            let id = self.prepares.fetch_add(1, Ordering::SeqCst);
            if self.prepare_error {
                return Err(RepoError::Cancelled);
            }
            Ok(WorkingTreePermit::new(id))
        }

        async fn release(&self, permit: WorkingTreePermit) {
            self.releases
                .lock()
                .unwrap()
                .push(*permit.token::<usize>().unwrap());
        }
    }

    fn change() -> WorkingTreeChange {
        WorkingTreeChange::Write {
            path: "notes/document.md".to_owned(),
            planned_revision: Some("file-id".to_owned()),
        }
    }

    #[tokio::test]
    async fn acquired_permit_is_released_once_after_success() {
        let coordinator = Arc::new(RecordingCoordinator::default());
        let result = with_working_tree_permit(coordinator.clone(), &[change()], || async {
            Ok::<_, RepoError>("complete")
        })
        .await;

        assert_eq!(result.unwrap(), "complete");
        assert_eq!(coordinator.prepare_count(), 1);
        assert_eq!(coordinator.released(), [0]);
    }

    #[tokio::test]
    async fn acquired_permit_is_released_once_after_operation_error() {
        let coordinator = Arc::new(RecordingCoordinator::default());
        let result = with_working_tree_permit(coordinator.clone(), &[change()], || async {
            Err::<(), _>(RepoError::IndexFileChanged)
        })
        .await;

        assert!(matches!(result, Err(RepoError::IndexFileChanged)));
        assert_eq!(coordinator.prepare_count(), 1);
        assert_eq!(coordinator.released(), [0]);
    }

    #[tokio::test]
    async fn acquired_permit_is_released_once_when_operation_is_cancelled() {
        let coordinator = Arc::new(RecordingCoordinator::default());
        let operation_started = Arc::new(Notify::new());
        let task = {
            let coordinator = coordinator.clone();
            let operation_started = operation_started.clone();
            tokio::spawn(async move {
                with_working_tree_permit(coordinator, &[change()], || async move {
                    operation_started.notify_one();
                    pending::<Result<(), RepoError>>().await
                })
                .await
            })
        };
        operation_started.notified().await;

        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::task::yield_now().await;

        assert_eq!(coordinator.prepare_count(), 1);
        assert_eq!(coordinator.released(), [0]);
    }

    #[tokio::test]
    async fn prepare_error_and_empty_change_set_do_not_release_a_permit() {
        let failing = Arc::new(RecordingCoordinator::failing());
        let result = with_working_tree_permit(failing.clone(), &[change()], || async {
            Ok::<_, RepoError>(())
        })
        .await;
        assert!(matches!(result, Err(RepoError::Cancelled)));
        assert_eq!(failing.prepare_count(), 1);
        assert!(failing.released().is_empty());

        let empty = Arc::new(RecordingCoordinator::default());
        with_working_tree_permit(empty.clone(), &[], || async { Ok::<_, RepoError>(()) })
            .await
            .unwrap();
        assert_eq!(empty.prepare_count(), 0);
        assert!(empty.released().is_empty());
    }

    #[tokio::test]
    async fn noop_coordinator_accepts_write_and_delete_changes_without_work() {
        let coordinator = NoopWorkingTreeCoordinator;
        let changes = [
            change(),
            WorkingTreeChange::Delete {
                path: "notes/removed.md".to_owned(),
                planned_revision: None,
            },
        ];

        let permit = coordinator.prepare(&changes).await.unwrap();
        assert!(permit.token::<usize>().is_none());
        coordinator.release(permit).await;
    }
}
