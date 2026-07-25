use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use qingyu_dejavu::{RepoError, WorkingTreeChange, WorkingTreeCoordinator, WorkingTreePermit};
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::oneshot;

use super::repository::WorkingTreeCoordinatorFactory;
use super::service::{RepositoryJobError, SyncAttemptContext};

pub(crate) const SYNC_PATH_GUARD_REQUEST_EVENT: &str = "qingyu://sync-path-guard-request";
pub(crate) const SYNC_PATH_GUARD_RELEASE_EVENT: &str = "qingyu://sync-path-guard-release";
const PRIMARY_EDITOR_WINDOW_LABEL: &str = "main";
const PATH_GUARD_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_TRACKED_OWNED_JOBS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncPathGuardRequest {
    pub(crate) request_id: String,
    pub(crate) job_id: String,
    pub(crate) notes_root: PathBuf,
    pub(crate) relative_paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncPathGuardRelease {
    pub(crate) request_id: String,
    pub(crate) notes_root: PathBuf,
    pub(crate) relative_paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PathGuardAcknowledgeInput {
    pub(crate) request_id: String,
    pub(crate) notes_root: PathBuf,
}

pub(crate) trait PathGuardEventBridge: Send + Sync {
    fn primary_notes_root(&self) -> Option<PathBuf>;
    fn primary_window_label(&self) -> &str;
    fn emit_request(&self, request: SyncPathGuardRequest) -> Result<(), RepoError>;
    fn emit_release(&self, release: SyncPathGuardRelease);
}

struct PendingRequest {
    owner_window_label: String,
    request: SyncPathGuardRequest,
    acknowledgement: Option<oneshot::Sender<Result<(), RepoError>>>,
}

struct PathGuardFactoryInner {
    bridge: Arc<dyn PathGuardEventBridge>,
    pending: Mutex<HashMap<String, PendingRequest>>,
    seen_owned_jobs: Mutex<HashSet<(String, PathBuf)>>,
    seen_owned_order: Mutex<VecDeque<(String, PathBuf)>>,
    timeout: Duration,
}

#[derive(Clone)]
pub(crate) struct PathGuardCoordinatorFactory {
    inner: Arc<PathGuardFactoryInner>,
}

impl PathGuardCoordinatorFactory {
    pub(crate) fn new<Bridge>(bridge: Arc<Bridge>, timeout: Duration) -> Self
    where
        Bridge: PathGuardEventBridge + 'static,
    {
        let bridge: Arc<dyn PathGuardEventBridge> = bridge;
        Self {
            inner: Arc::new(PathGuardFactoryInner {
                bridge,
                pending: Mutex::new(HashMap::new()),
                seen_owned_jobs: Mutex::new(HashSet::new()),
                seen_owned_order: Mutex::new(VecDeque::new()),
                timeout,
            }),
        }
    }

    pub(crate) fn acknowledge(
        &self,
        window_label: &str,
        input: PathGuardAcknowledgeInput,
    ) -> Result<(), RepositoryJobError> {
        validate_uuid(&input.request_id)?;
        let (owner_window_label, request_root) = {
            let pending = self.inner.pending.lock().unwrap();
            let request = pending
                .get(&input.request_id)
                .ok_or(RepositoryJobError::WorkingTreeChanged)?;
            (
                request.owner_window_label.clone(),
                request.request.notes_root.clone(),
            )
        };
        let input_root = canonical_exact_root(&input.notes_root);
        let owner_still_matches = self
            .inner
            .bridge
            .primary_notes_root()
            .is_some_and(|root| root == request_root);
        let valid = window_label == owner_window_label
            && input_root
                .as_ref()
                .is_some_and(|root| root == &request_root)
            && owner_still_matches;
        let mut pending = self.inner.pending.lock().unwrap();
        let request = pending
            .get_mut(&input.request_id)
            .ok_or(RepositoryJobError::WorkingTreeChanged)?;
        let Some(acknowledgement) = request.acknowledgement.take() else {
            return Err(RepositoryJobError::WorkingTreeChanged);
        };
        if valid {
            acknowledgement
                .send(Ok(()))
                .map_err(|_| RepositoryJobError::WorkingTreeChanged)
        } else {
            let _send_result = acknowledgement.send(Err(RepoError::WorkingTreeChanged));
            Err(RepositoryJobError::WorkingTreeChanged)
        }
    }
}

impl WorkingTreeCoordinatorFactory for PathGuardCoordinatorFactory {
    fn create(
        &self,
        context: &SyncAttemptContext,
    ) -> Result<Arc<dyn WorkingTreeCoordinator>, RepositoryJobError> {
        validate_uuid(&context.job_id)?;
        let notes_root = canonical_exact_root(&context.request.notes_root)
            .ok_or(RepositoryJobError::InvalidBinding)?;
        let key = (context.job_id.clone(), notes_root.clone());
        let currently_owned = self
            .inner
            .bridge
            .primary_notes_root()
            .is_some_and(|root| root == notes_root);
        let was_owned = {
            let mut seen = self.inner.seen_owned_jobs.lock().unwrap();
            if currently_owned {
                let inserted = seen.insert(key.clone());
                if inserted {
                    let mut order = self.inner.seen_owned_order.lock().unwrap();
                    order.push_back(key.clone());
                    while order.len() > MAX_TRACKED_OWNED_JOBS {
                        if let Some(expired) = order.pop_front() {
                            seen.remove(&expired);
                        }
                    }
                }
            }
            seen.contains(&key)
        };
        if !currently_owned && !was_owned {
            return Ok(Arc::new(InactiveWorkingTreeCoordinator));
        }

        Ok(Arc::new(PathGuardCoordinator {
            cancellation: context.cancellation.clone(),
            inner: Arc::clone(&self.inner),
            job_id: context.job_id.clone(),
            notes_root,
        }))
    }
}

#[cfg(test)]
impl PathGuardCoordinatorFactory {
    fn tracked_owned_job_count(&self) -> usize {
        self.inner.seen_owned_jobs.lock().unwrap().len()
    }
}

struct InactiveWorkingTreeCoordinator;

impl WorkingTreeCoordinator for InactiveWorkingTreeCoordinator {
    fn prepare<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _changes: &'life1 [WorkingTreeChange],
    ) -> Pin<Box<dyn Future<Output = Result<WorkingTreePermit, RepoError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Ok(WorkingTreePermit::new(())) })
    }

    fn release<'life0, 'async_trait>(
        &'life0 self,
        permit: WorkingTreePermit,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { drop(permit) })
    }
}

struct PathGuardCoordinator {
    cancellation: super::service::JobCancellationToken,
    inner: Arc<PathGuardFactoryInner>,
    job_id: String,
    notes_root: PathBuf,
}

impl WorkingTreeCoordinator for PathGuardCoordinator {
    fn prepare<'life0, 'life1, 'async_trait>(
        &'life0 self,
        changes: &'life1 [WorkingTreeChange],
    ) -> Pin<Box<dyn Future<Output = Result<WorkingTreePermit, RepoError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            if self.cancellation.is_cancelled() {
                return Err(RepoError::Cancelled);
            }
            if !self.owner_matches() {
                return Err(RepoError::WorkingTreeChanged);
            }
            let relative_paths = validated_relative_paths(changes)?;
            let request_id = uuid::Uuid::new_v4().to_string();
            let request = SyncPathGuardRequest {
                request_id: request_id.clone(),
                job_id: self.job_id.clone(),
                notes_root: self.notes_root.clone(),
                relative_paths,
            };
            let release = SyncPathGuardRelease {
                request_id: request_id.clone(),
                notes_root: self.notes_root.clone(),
                relative_paths: request.relative_paths.clone(),
            };
            let (acknowledgement, receiver) = oneshot::channel();
            {
                let mut pending = self.inner.pending.lock().unwrap();
                if pending
                    .insert(
                        request_id.clone(),
                        PendingRequest {
                            owner_window_label: self.inner.bridge.primary_window_label().to_owned(),
                            request: request.clone(),
                            acknowledgement: Some(acknowledgement),
                        },
                    )
                    .is_some()
                {
                    return Err(RepoError::WorkingTreeChanged);
                }
            }
            let published = PublishedPathGuard {
                inner: Arc::clone(&self.inner),
                release: Some(release),
            };
            self.inner.bridge.emit_request(request)?;
            let result = tokio::time::timeout(self.inner.timeout, receiver)
                .await
                .map_err(|_| RepoError::Cancelled)?
                .map_err(|_| RepoError::Cancelled)?;
            result?;
            if self.cancellation.is_cancelled() {
                return Err(RepoError::Cancelled);
            }
            if !self.owner_matches() {
                return Err(RepoError::WorkingTreeChanged);
            }

            Ok(WorkingTreePermit::new(published))
        })
    }

    fn release<'life0, 'async_trait>(
        &'life0 self,
        permit: WorkingTreePermit,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { drop(permit) })
    }
}

impl PathGuardCoordinator {
    fn owner_matches(&self) -> bool {
        self.inner
            .bridge
            .primary_notes_root()
            .is_some_and(|root| root == self.notes_root)
    }
}

struct PublishedPathGuard {
    inner: Arc<PathGuardFactoryInner>,
    release: Option<SyncPathGuardRelease>,
}

impl Drop for PublishedPathGuard {
    fn drop(&mut self) {
        let Some(release) = self.release.take() else {
            return;
        };
        self.inner
            .pending
            .lock()
            .unwrap()
            .remove(&release.request_id);
        self.inner.bridge.emit_release(release);
    }
}

fn validated_relative_paths(changes: &[WorkingTreeChange]) -> Result<Vec<String>, RepoError> {
    let paths = changes
        .iter()
        .map(|change| change.path.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    if paths.is_empty() {
        return Err(RepoError::WorkingTreeChanged);
    }
    Ok(paths.into_iter().collect())
}

fn canonical_exact_root(root: &Path) -> Option<PathBuf> {
    if !root.is_absolute() {
        return None;
    }
    let canonical = root.canonicalize().ok()?;
    (canonical == root).then_some(canonical)
}

fn validate_uuid(value: &str) -> Result<(), RepositoryJobError> {
    let parsed =
        uuid::Uuid::parse_str(value).map_err(|_| RepositoryJobError::WorkingTreeChanged)?;
    if parsed.to_string() != value {
        return Err(RepositoryJobError::WorkingTreeChanged);
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct PathGuardCoordinatorOwner {
    factory: OnceLock<PathGuardCoordinatorFactory>,
}

impl PathGuardCoordinatorOwner {
    pub(crate) fn install(
        &self,
        factory: PathGuardCoordinatorFactory,
    ) -> Result<(), RepositoryJobError> {
        self.factory
            .set(factory)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    fn acknowledge(
        &self,
        window_label: &str,
        input: PathGuardAcknowledgeInput,
    ) -> Result<(), RepositoryJobError> {
        self.factory
            .get()
            .ok_or(RepositoryJobError::RepositoryUnavailable)?
            .acknowledge(window_label, input)
    }
}

struct TauriPathGuardBridge {
    app: tauri::AppHandle,
}

impl TauriPathGuardBridge {
    fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl PathGuardEventBridge for TauriPathGuardBridge {
    fn primary_notes_root(&self) -> Option<PathBuf> {
        crate::primary_workspace::resolve_sync_primary_workspace(&self.app).ok()
    }

    fn primary_window_label(&self) -> &str {
        PRIMARY_EDITOR_WINDOW_LABEL
    }

    fn emit_request(&self, request: SyncPathGuardRequest) -> Result<(), RepoError> {
        self.app
            .emit_to(
                PRIMARY_EDITOR_WINDOW_LABEL,
                SYNC_PATH_GUARD_REQUEST_EVENT,
                request,
            )
            .map_err(|_| RepoError::Cancelled)
    }

    fn emit_release(&self, release: SyncPathGuardRelease) {
        let _emit_result = self.app.emit_to(
            PRIMARY_EDITOR_WINDOW_LABEL,
            SYNC_PATH_GUARD_RELEASE_EVENT,
            release,
        );
    }
}

pub(crate) fn tauri_path_guard_factory(app: tauri::AppHandle) -> PathGuardCoordinatorFactory {
    PathGuardCoordinatorFactory::new(Arc::new(TauriPathGuardBridge::new(app)), PATH_GUARD_TIMEOUT)
}

#[tauri::command]
pub(crate) fn acknowledge_path_guard(
    window: tauri::WebviewWindow,
    owner: tauri::State<'_, PathGuardCoordinatorOwner>,
    request: PathGuardAcknowledgeInput,
) -> Result<(), String> {
    owner
        .acknowledge(window.label(), request)
        .map_err(|error| error.safe_code().to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use qingyu_dejavu::{
        ExpectedRevision, RepoError, RepositoryRelativePath, WorkingTreeAction, WorkingTreeChange,
    };
    use tempfile::tempdir;
    use tokio::sync::mpsc;

    use super::{
        PathGuardAcknowledgeInput, PathGuardCoordinatorFactory, PathGuardEventBridge,
        SyncPathGuardRelease, SyncPathGuardRequest, MAX_TRACKED_OWNED_JOBS,
    };
    use crate::dejavu_sync::repository::WorkingTreeCoordinatorFactory;
    use crate::dejavu_sync::service::{JobCancellationToken, SyncAttemptContext, SyncJobRequest};
    use crate::sync_config::status::SyncTrigger;

    struct FakeBridge {
        owned_root: Mutex<Option<PathBuf>>,
        requests: mpsc::UnboundedSender<SyncPathGuardRequest>,
        releases: Mutex<Vec<SyncPathGuardRelease>>,
    }

    impl FakeBridge {
        fn new(
            owned_root: Option<PathBuf>,
        ) -> (Arc<Self>, mpsc::UnboundedReceiver<SyncPathGuardRequest>) {
            let (requests, receiver) = mpsc::unbounded_channel();
            (
                Arc::new(Self {
                    owned_root: Mutex::new(owned_root),
                    requests,
                    releases: Mutex::new(Vec::new()),
                }),
                receiver,
            )
        }

        fn change_owner(&self, root: Option<PathBuf>) {
            *self.owned_root.lock().unwrap() = root;
        }

        fn releases(&self) -> Vec<SyncPathGuardRelease> {
            self.releases.lock().unwrap().clone()
        }
    }

    impl PathGuardEventBridge for FakeBridge {
        fn primary_notes_root(&self) -> Option<PathBuf> {
            self.owned_root.lock().unwrap().clone()
        }

        fn primary_window_label(&self) -> &str {
            "main"
        }

        fn emit_request(&self, request: SyncPathGuardRequest) -> Result<(), RepoError> {
            self.requests
                .send(request)
                .map_err(|_| RepoError::Cancelled)
        }

        fn emit_release(&self, release: SyncPathGuardRelease) {
            self.releases.lock().unwrap().push(release);
        }
    }

    fn context(root: &Path, job_id: &str) -> SyncAttemptContext {
        SyncAttemptContext {
            request: SyncJobRequest {
                notes_root: root.to_path_buf(),
                repository_id: "6f26bc85-9b50-4c90-9ea5-456eea9b8aa4".to_owned(),
                trigger: SyncTrigger::Interval,
            },
            job_id: job_id.to_owned(),
            attempt: 1,
            cancellation: JobCancellationToken::new(),
        }
    }

    fn changes() -> Vec<WorkingTreeChange> {
        vec![
            WorkingTreeChange {
                path: RepositoryRelativePath::new("notes/second.md").unwrap(),
                expected_revision: ExpectedRevision::Absent,
                action: WorkingTreeAction::Write,
            },
            WorkingTreeChange {
                path: RepositoryRelativePath::new("notes/first.md").unwrap(),
                expected_revision: ExpectedRevision::Absent,
                action: WorkingTreeAction::Remove,
            },
            WorkingTreeChange {
                path: RepositoryRelativePath::new("notes/second.md").unwrap(),
                expected_revision: ExpectedRevision::Absent,
                action: WorkingTreeAction::Write,
            },
        ]
    }

    #[tokio::test]
    async fn owner_ack_uses_canonical_identity_and_releases_exactly_once() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (bridge, mut requests) = FakeBridge::new(Some(root.clone()));
        let factory = PathGuardCoordinatorFactory::new(bridge.clone(), Duration::from_secs(1));
        let coordinator = factory
            .create(&context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac"))
            .unwrap();
        let prepare = tokio::spawn({
            let coordinator = Arc::clone(&coordinator);
            async move { coordinator.prepare(&changes()).await }
        });
        let request = requests.recv().await.unwrap();
        assert_eq!(
            request.relative_paths,
            vec!["notes/first.md", "notes/second.md"]
        );
        factory
            .acknowledge(
                "main",
                PathGuardAcknowledgeInput {
                    request_id: request.request_id.clone(),
                    notes_root: root.clone(),
                },
            )
            .unwrap();
        let permit = prepare.await.unwrap().unwrap();
        assert!(factory
            .acknowledge(
                "main",
                PathGuardAcknowledgeInput {
                    request_id: request.request_id.clone(),
                    notes_root: root.clone(),
                },
            )
            .is_err());

        coordinator.release(permit).await;
        assert_eq!(bridge.releases().len(), 1);
        drop(coordinator);
        assert_eq!(bridge.releases().len(), 1);
    }

    #[tokio::test]
    async fn malformed_root_non_owner_and_mismatch_abort_and_cleanup() {
        for (window, acknowledged_root) in [("side", "owned"), ("main", "alias"), ("main", "other")]
        {
            let directory = tempdir().unwrap();
            let root = directory.path().canonicalize().unwrap();
            let other = tempdir().unwrap();
            let other_root = other.path().canonicalize().unwrap();
            let (bridge, mut requests) = FakeBridge::new(Some(root.clone()));
            let factory = PathGuardCoordinatorFactory::new(bridge.clone(), Duration::from_secs(1));
            let coordinator = factory
                .create(&context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac"))
                .unwrap();
            let prepare = tokio::spawn({
                let coordinator = Arc::clone(&coordinator);
                async move { coordinator.prepare(&changes()).await }
            });
            let request = requests.recv().await.unwrap();
            let notes_root = match acknowledged_root {
                "owned" => root.clone(),
                "alias" => root.join("..").join(root.file_name().unwrap()),
                _ => other_root,
            };
            assert!(factory
                .acknowledge(
                    window,
                    PathGuardAcknowledgeInput {
                        request_id: request.request_id,
                        notes_root,
                    },
                )
                .is_err());
            assert!(prepare.await.unwrap().is_err());
            assert_eq!(bridge.releases().len(), 1);
        }
    }

    #[tokio::test]
    async fn timeout_and_listener_loss_apply_nothing_and_cleanup_release() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (bridge, mut requests) = FakeBridge::new(Some(root.clone()));
        let factory = PathGuardCoordinatorFactory::new(bridge.clone(), Duration::from_millis(5));
        let coordinator = factory
            .create(&context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac"))
            .unwrap();

        assert!(coordinator.prepare(&changes()).await.is_err());
        let request = requests.recv().await.unwrap();
        assert!(factory
            .acknowledge(
                "main",
                PathGuardAcknowledgeInput {
                    request_id: request.request_id,
                    notes_root: root,
                },
            )
            .is_err());
        assert_eq!(bridge.releases().len(), 1);
    }

    #[tokio::test]
    async fn ownership_switch_after_publication_aborts_and_stays_guarded_on_retry() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let root = first.path().canonicalize().unwrap();
        let next_root = second.path().canonicalize().unwrap();
        let (bridge, mut requests) = FakeBridge::new(Some(root.clone()));
        let factory = PathGuardCoordinatorFactory::new(bridge.clone(), Duration::from_secs(1));
        let attempt = context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac");
        let coordinator = factory.create(&attempt).unwrap();
        let prepare = tokio::spawn({
            let coordinator = Arc::clone(&coordinator);
            async move { coordinator.prepare(&changes()).await }
        });
        let request = requests.recv().await.unwrap();
        bridge.change_owner(Some(next_root));
        assert!(factory
            .acknowledge(
                "main",
                PathGuardAcknowledgeInput {
                    request_id: request.request_id,
                    notes_root: root.clone(),
                },
            )
            .is_err());
        assert!(matches!(
            prepare.await.unwrap(),
            Err(RepoError::WorkingTreeChanged)
        ));
        assert_eq!(bridge.releases().len(), 1);

        let retry = factory.create(&attempt).unwrap();
        assert!(matches!(
            retry.prepare(&changes()).await,
            Err(RepoError::WorkingTreeChanged)
        ));
    }

    #[tokio::test]
    async fn a_never_owned_inactive_root_uses_a_product_layer_noop_permit() {
        let inactive = tempdir().unwrap();
        let active = tempdir().unwrap();
        let root = inactive.path().canonicalize().unwrap();
        let (bridge, mut requests) = FakeBridge::new(Some(active.path().canonicalize().unwrap()));
        let factory = PathGuardCoordinatorFactory::new(bridge, Duration::from_secs(1));
        let coordinator = factory
            .create(&context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac"))
            .unwrap();

        let permit = coordinator.prepare(&changes()).await.unwrap();
        coordinator.release(permit).await;
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn owned_attempt_memory_is_bounded() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (bridge, _requests) = FakeBridge::new(Some(root.clone()));
        let factory = PathGuardCoordinatorFactory::new(bridge, Duration::from_secs(1));

        for sequence in 1..=(MAX_TRACKED_OWNED_JOBS + 20) {
            let job_id = uuid::Uuid::from_u128(sequence as u128).to_string();
            factory.create(&context(&root, &job_id)).unwrap();
        }

        assert_eq!(factory.tracked_owned_job_count(), MAX_TRACKED_OWNED_JOBS);
    }
}
