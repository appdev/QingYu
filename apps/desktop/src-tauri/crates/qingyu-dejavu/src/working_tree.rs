use std::any::Any;
use std::future::Future;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryRelativePath(String);

impl RepositoryRelativePath {
    pub fn new(path: impl Into<String>) -> Result<Self, crate::RepoError> {
        let path = path.into();
        validate_repository_relative_path(&path)?;
        Ok(Self(path))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl TryFrom<String> for RepositoryRelativePath {
    type Error = crate::RepoError;

    fn try_from(path: String) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

impl TryFrom<&str> for RepositoryRelativePath {
    type Error = crate::RepoError;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpectedRevision {
    Absent,
    File { id: String, size: i64, updated: i64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkingTreeAction {
    Write,
    Remove,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingTreeChange {
    pub path: RepositoryRelativePath,
    pub expected_revision: ExpectedRevision,
    pub action: WorkingTreeAction,
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

/// Runs an operation under a prepared working-tree permit.
///
/// Normal success and error returns await exactly one release. If this future is
/// dropped, `Drop` only schedules a best-effort release when a live Tokio runtime
/// is available; runtime shutdown is not a completion guarantee.
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
        runtime: tokio::runtime::Handle::try_current().ok(),
    };
    let result = operation().await;
    scope.release().await;
    result
}

struct WorkingTreePermitScope {
    coordinator: Arc<dyn WorkingTreeCoordinator>,
    permit: Option<WorkingTreePermit>,
    runtime: Option<tokio::runtime::Handle>,
}

impl WorkingTreePermitScope {
    async fn release(mut self) {
        if let Some(permit) = self.permit.take() {
            let coordinator = Arc::clone(&self.coordinator);
            if let Some(runtime) = &self.runtime {
                let release = runtime.spawn(async move { coordinator.release(permit).await });
                let _release_result = release.await;
            } else {
                coordinator.release(permit).await;
            }
        }
    }
}

impl Drop for WorkingTreePermitScope {
    fn drop(&mut self) {
        if let Some(permit) = self.permit.take() {
            let Some(runtime) = &self.runtime else {
                return;
            };
            let coordinator = Arc::clone(&self.coordinator);
            drop(runtime.spawn(async move { coordinator.release(permit).await }));
        }
    }
}

fn validate_repository_relative_path(path: &str) -> Result<(), crate::RepoError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
    {
        return Err(crate::RepoError::UnsafePath);
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.contains(':')
            || component.ends_with(['.', ' '])
        {
            return Err(crate::RepoError::UnsafePath);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use tokio::sync::Notify;

    use crate::RepoError;

    use super::{
        with_working_tree_permit, ExpectedRevision, NoopWorkingTreeCoordinator,
        RepositoryRelativePath, WorkingTreeAction, WorkingTreeChange, WorkingTreeCoordinator,
        WorkingTreePermit,
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

    #[derive(Default)]
    struct BlockingReleaseCoordinator {
        release_started: Notify,
        allow_release: Notify,
        releases: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl WorkingTreeCoordinator for BlockingReleaseCoordinator {
        async fn prepare(
            &self,
            _changes: &[WorkingTreeChange],
        ) -> Result<WorkingTreePermit, RepoError> {
            Ok(WorkingTreePermit::new(0_usize))
        }

        async fn release(&self, _permit: WorkingTreePermit) {
            self.release_started.notify_one();
            self.allow_release.notified().await;
            self.releases.fetch_add(1, Ordering::SeqCst);
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
        WorkingTreeChange {
            path: RepositoryRelativePath::new("notes/document.md").unwrap(),
            expected_revision: ExpectedRevision::File {
                id: "file-id".to_owned(),
                size: 42,
                updated: 1_234,
            },
            action: WorkingTreeAction::Write,
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
    async fn explicit_cancelled_result_is_released_once_before_returning() {
        let coordinator = Arc::new(RecordingCoordinator::default());
        let result = with_working_tree_permit(coordinator.clone(), &[change()], || async {
            Err::<(), _>(RepoError::Cancelled)
        })
        .await;

        assert!(matches!(result, Err(RepoError::Cancelled)));
        assert_eq!(coordinator.released(), [0]);
    }

    #[tokio::test]
    async fn normal_completion_waits_until_release_finishes() {
        let coordinator = Arc::new(BlockingReleaseCoordinator::default());
        let task = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                with_working_tree_permit(coordinator, &[change()], || async {
                    Ok::<_, RepoError>("done")
                })
                .await
            })
        };

        coordinator.release_started.notified().await;
        assert!(!task.is_finished());
        assert_eq!(coordinator.releases.load(Ordering::SeqCst), 0);
        coordinator.allow_release.notify_one();

        assert_eq!(task.await.unwrap().unwrap(), "done");
        assert_eq!(coordinator.releases.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dropped_helper_future_schedules_best_effort_release_on_a_live_runtime() {
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
            WorkingTreeChange {
                path: RepositoryRelativePath::new("notes/removed.md").unwrap(),
                expected_revision: ExpectedRevision::Absent,
                action: WorkingTreeAction::Remove,
            },
        ];

        let permit = coordinator.prepare(&changes).await.unwrap();
        assert!(permit.token::<usize>().is_none());
        coordinator.release(permit).await;
    }

    #[test]
    fn repository_relative_paths_reject_platform_and_traversal_escapes() {
        for path in [
            "",
            "/absolute",
            "../escape",
            "notes/../escape",
            "notes//file",
            "notes\\file",
            "C:/drive",
            "notes/name:stream",
            "notes/trailing.",
            "notes/trailing ",
            "notes/\0file",
        ] {
            assert!(matches!(
                RepositoryRelativePath::new(path),
                Err(RepoError::UnsafePath)
            ));
        }
        assert_eq!(
            RepositoryRelativePath::new("notes/document.md")
                .unwrap()
                .as_str(),
            "notes/document.md"
        );
    }
}
