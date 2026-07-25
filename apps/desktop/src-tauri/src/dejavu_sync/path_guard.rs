use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use qingyu_dejavu::{RepoError, WorkingTreeChange, WorkingTreeCoordinator, WorkingTreePermit};
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::{oneshot, Notify};

use super::repository::WorkingTreeCoordinatorFactory;
use super::service::{RepositoryJobError, SyncAttemptContext};

pub(crate) const SYNC_PATH_GUARD_REQUEST_EVENT: &str = "qingyu://sync-path-guard-request";
pub(crate) const SYNC_PATH_GUARD_RELEASE_EVENT: &str = "qingyu://sync-path-guard-release";
const PRIMARY_EDITOR_WINDOW_LABEL: &str = "main";
const PATH_GUARD_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_TRACKED_OWNED_JOBS: usize = 256;
pub(crate) const SYNC_PATH_GUARDED_ERROR: &str = "sync-path-guarded";

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

#[derive(Default)]
pub(crate) struct NativeWorkingTreeRegistry {
    state: Mutex<NativeWorkingTreeState>,
}

#[derive(Default)]
struct NativeWorkingTreeState {
    next_id: u64,
    mutations: HashMap<u64, NativeMutationRecord>,
    path_blocks: HashMap<u64, NativePathBlockRecord>,
    ownership_leases: HashMap<u64, PrimaryOwnershipConstraint>,
}

struct NativePathBlockRecord {
    authorization: Option<NativePathBlockAuthorization>,
    paths: Vec<PathBuf>,
}

struct NativePathBlockAuthorization {
    owner_window_label: String,
    request_id: String,
}

struct NativeMutationRecord {
    authorization: Option<NativeMutationAuthorization>,
    paths: Vec<PathBuf>,
    completion: Arc<NativeMutationCompletion>,
}

struct NativeMutationAuthorization {
    owner_window_label: String,
    request_id: String,
}

#[derive(Default)]
struct NativeMutationCompletion {
    released: AtomicBool,
    notify: Notify,
}

impl NativeMutationCompletion {
    async fn wait(&self) {
        while !self.released.load(Ordering::Acquire) {
            let notified = self.notify.notified();
            if self.released.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    fn release(&self) {
        self.released.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }
}

#[derive(Clone)]
struct PrimaryOwnershipConstraint {
    root: PathBuf,
    expected_owned: bool,
}

pub(crate) struct NativeMutationLease {
    id: u64,
    registry: Arc<NativeWorkingTreeRegistry>,
    completion: Arc<NativeMutationCompletion>,
}

impl std::fmt::Debug for NativeMutationLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeMutationLease")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Drop for NativeMutationLease {
    fn drop(&mut self) {
        self.registry
            .state
            .lock()
            .unwrap()
            .mutations
            .remove(&self.id);
        self.completion.release();
    }
}

pub(crate) struct NativePathBlock {
    id: u64,
    registry: Arc<NativeWorkingTreeRegistry>,
    existing: Vec<Arc<NativeMutationCompletion>>,
}

impl NativePathBlock {
    async fn wait_for_existing(&self) {
        for completion in &self.existing {
            completion.wait().await;
        }
    }
}

impl Drop for NativePathBlock {
    fn drop(&mut self) {
        self.registry
            .state
            .lock()
            .unwrap()
            .path_blocks
            .remove(&self.id);
    }
}

pub(crate) struct PrimaryOwnershipLease {
    id: u64,
    registry: Arc<NativeWorkingTreeRegistry>,
}

impl Drop for PrimaryOwnershipLease {
    fn drop(&mut self) {
        self.registry
            .state
            .lock()
            .unwrap()
            .ownership_leases
            .remove(&self.id);
    }
}

impl NativeWorkingTreeRegistry {
    pub(crate) fn acquire_mutation(
        self: &Arc<Self>,
        paths: &[PathBuf],
    ) -> Result<NativeMutationLease, String> {
        self.acquire_mutation_with_authorization(paths, None)
    }

    pub(crate) fn acquire_authorized_mutation(
        self: &Arc<Self>,
        paths: &[PathBuf],
        window_label: &str,
        request_id: &str,
    ) -> Result<NativeMutationLease, String> {
        self.acquire_mutation_with_authorization(paths, Some((window_label, request_id)))
    }

    pub(crate) fn acquire_creation_candidate(
        self: &Arc<Self>,
        path: &Path,
    ) -> Result<NativeMutationLease, String> {
        self.acquire_canonical_mutation_paths(vec![canonical_creation_candidate_path(path)?], None)
    }

    fn acquire_mutation_with_authorization(
        self: &Arc<Self>,
        paths: &[PathBuf],
        authorization: Option<(&str, &str)>,
    ) -> Result<NativeMutationLease, String> {
        let paths = paths
            .iter()
            .map(|path| canonical_mutation_path(path))
            .collect::<Result<Vec<_>, _>>()?;
        self.acquire_canonical_mutation_paths(paths, authorization)
    }

    fn acquire_canonical_mutation_paths(
        self: &Arc<Self>,
        paths: Vec<PathBuf>,
        authorization: Option<(&str, &str)>,
    ) -> Result<NativeMutationLease, String> {
        if paths.is_empty() {
            return Err("working-tree mutation path is required".to_string());
        }
        let mut state = self.state.lock().unwrap();
        if state.path_blocks.values().any(|blocked| {
            let intersects = paths.iter().any(|path| {
                blocked
                    .paths
                    .iter()
                    .any(|guarded| paths_intersect(path, guarded))
            });
            if !intersects {
                return false;
            }
            let authorized = authorization.is_some_and(|(window_label, request_id)| {
                blocked.authorization.as_ref().is_some_and(|allowed| {
                    allowed.owner_window_label == window_label
                        && allowed.request_id == request_id
                        && paths
                            .iter()
                            .all(|path| blocked.paths.iter().any(|guarded| path == guarded))
                })
            });
            !authorized
        }) {
            return Err(SYNC_PATH_GUARDED_ERROR.to_string());
        }
        let id = next_registry_id(&mut state);
        let completion = Arc::new(NativeMutationCompletion::default());
        state.mutations.insert(
            id,
            NativeMutationRecord {
                authorization: authorization.map(|(window_label, request_id)| {
                    NativeMutationAuthorization {
                        owner_window_label: window_label.to_owned(),
                        request_id: request_id.to_owned(),
                    }
                }),
                paths,
                completion: Arc::clone(&completion),
            },
        );
        Ok(NativeMutationLease {
            id,
            registry: Arc::clone(self),
            completion,
        })
    }

    pub(crate) fn block_paths(
        self: &Arc<Self>,
        root: &Path,
        relative_paths: &[String],
    ) -> Result<NativePathBlock, RepoError> {
        self.block_paths_with_authorization(root, relative_paths, None)
    }

    pub(crate) fn block_paths_for_request(
        self: &Arc<Self>,
        root: &Path,
        relative_paths: &[String],
        owner_window_label: &str,
        request_id: &str,
    ) -> Result<NativePathBlock, RepoError> {
        self.block_paths_with_authorization(
            root,
            relative_paths,
            Some(NativePathBlockAuthorization {
                owner_window_label: owner_window_label.to_owned(),
                request_id: request_id.to_owned(),
            }),
        )
    }

    fn block_paths_with_authorization(
        self: &Arc<Self>,
        root: &Path,
        relative_paths: &[String],
        authorization: Option<NativePathBlockAuthorization>,
    ) -> Result<NativePathBlock, RepoError> {
        let paths = relative_paths
            .iter()
            .map(|relative| guarded_absolute_path(root, relative))
            .collect::<Result<Vec<_>, _>>()?;
        let mut state = self.state.lock().unwrap();
        let id = next_registry_id(&mut state);
        let existing = state
            .mutations
            .values()
            .filter(|mutation| {
                mutation
                    .paths
                    .iter()
                    .any(|path| paths.iter().any(|guarded| paths_intersect(path, guarded)))
            })
            .map(|mutation| Arc::clone(&mutation.completion))
            .collect();
        state.path_blocks.insert(
            id,
            NativePathBlockRecord {
                authorization,
                paths,
            },
        );
        Ok(NativePathBlock {
            id,
            registry: Arc::clone(self),
            existing,
        })
    }

    pub(crate) fn lease_ownership(
        self: &Arc<Self>,
        root: PathBuf,
        expected_owned: bool,
    ) -> PrimaryOwnershipLease {
        let mut state = self.state.lock().unwrap();
        let id = next_registry_id(&mut state);
        state.ownership_leases.insert(
            id,
            PrimaryOwnershipConstraint {
                root,
                expected_owned,
            },
        );
        PrimaryOwnershipLease {
            id,
            registry: Arc::clone(self),
        }
    }

    pub(crate) fn validate_primary_root(&self, proposed: Option<&Path>) -> Result<(), String> {
        let state = self.state.lock().unwrap();
        let valid = state.ownership_leases.values().all(|lease| {
            let owns_root = proposed.is_some_and(|root| root == lease.root);
            owns_root == lease.expected_owned
        });
        valid
            .then_some(())
            .ok_or_else(|| SYNC_PATH_GUARDED_ERROR.to_string())
    }

    fn consume_request_authorization(&self, window_label: &str, request_id: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.mutations.values().any(|mutation| {
            mutation
                .authorization
                .as_ref()
                .is_some_and(|authorization| {
                    authorization.owner_window_label == window_label
                        && authorization.request_id == request_id
                })
        }) {
            return false;
        }
        let Some(block) = state.path_blocks.values_mut().find(|block| {
            block.authorization.as_ref().is_some_and(|authorization| {
                authorization.owner_window_label == window_label
                    && authorization.request_id == request_id
            })
        }) else {
            return false;
        };
        block.authorization = None;
        true
    }
}

fn next_registry_id(state: &mut NativeWorkingTreeState) -> u64 {
    loop {
        state.next_id = state.next_id.wrapping_add(1);
        if !state.mutations.contains_key(&state.next_id)
            && !state.path_blocks.contains_key(&state.next_id)
            && !state.ownership_leases.contains_key(&state.next_id)
        {
            return state.next_id;
        }
    }
}

fn guarded_absolute_path(root: &Path, relative: &str) -> Result<PathBuf, RepoError> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || has_raw_dot_segment(relative)
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(RepoError::WorkingTreeChanged);
    }
    canonical_mutation_path(&root.join(relative)).map_err(|_| RepoError::WorkingTreeChanged)
}

fn canonical_mutation_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("working-tree mutation path must be absolute".to_string());
    }
    if has_raw_dot_segment(path) {
        return Err("working-tree mutation path contains dot segments".to_string());
    }
    let mut cursor = path;
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(cursor) {
            Ok(_) => {
                let mut canonical = cursor.canonicalize().map_err(|error| error.to_string())?;
                if !missing.is_empty() && !canonical.is_dir() {
                    return Err("working-tree mutation parent is not a directory".to_string());
                }
                for component in missing.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = cursor
                    .file_name()
                    .ok_or_else(|| "working-tree mutation path is invalid".to_string())?;
                missing.push(name.to_os_string());
                cursor = cursor
                    .parent()
                    .ok_or_else(|| "working-tree mutation path is invalid".to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn canonical_creation_candidate_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("working-tree mutation path must be absolute".to_string());
    }
    if has_raw_dot_segment(path) {
        return Err("working-tree mutation path contains dot segments".to_string());
    }
    let name = path
        .file_name()
        .ok_or_else(|| "working-tree mutation path is invalid".to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "working-tree mutation path is invalid".to_string())?;
    let mut canonical_parent = canonical_mutation_path(parent)?;
    if !canonical_parent.is_dir() {
        return Err("working-tree mutation parent is not a directory".to_string());
    }
    canonical_parent.push(name);
    Ok(canonical_parent)
}

fn has_raw_dot_segment(path: &Path) -> bool {
    path.as_os_str()
        .as_encoded_bytes()
        .split(|byte| *byte == b'/' || (cfg!(windows) && *byte == b'\\'))
        .any(|segment| matches!(segment, b"." | b".."))
}

fn paths_intersect(first: &Path, second: &Path) -> bool {
    first == second || first.starts_with(second) || second.starts_with(first)
}

pub(crate) fn native_working_tree_registry() -> &'static Arc<NativeWorkingTreeRegistry> {
    static REGISTRY: OnceLock<Arc<NativeWorkingTreeRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Arc::new(NativeWorkingTreeRegistry::default()))
}

pub(crate) fn acquire_native_working_tree_mutation(
    paths: &[PathBuf],
) -> Result<NativeMutationLease, String> {
    native_working_tree_registry().acquire_mutation(paths)
}

struct PendingRequest {
    owner_window_label: String,
    request: SyncPathGuardRequest,
    acknowledgement: Option<oneshot::Sender<Result<(), RepoError>>>,
}

struct PathGuardFactoryInner {
    bridge: Arc<dyn PathGuardEventBridge>,
    native_registry: Arc<NativeWorkingTreeRegistry>,
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
    #[cfg(test)]
    pub(crate) fn new<Bridge>(bridge: Arc<Bridge>, timeout: Duration) -> Self
    where
        Bridge: PathGuardEventBridge + 'static,
    {
        Self::with_registry(
            bridge,
            timeout,
            Arc::new(NativeWorkingTreeRegistry::default()),
        )
    }

    fn with_registry<Bridge>(
        bridge: Arc<Bridge>,
        timeout: Duration,
        native_registry: Arc<NativeWorkingTreeRegistry>,
    ) -> Self
    where
        Bridge: PathGuardEventBridge + 'static,
    {
        let bridge: Arc<dyn PathGuardEventBridge> = bridge;
        Self {
            inner: Arc::new(PathGuardFactoryInner {
                bridge,
                native_registry,
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
        if valid
            && !self
                .inner
                .native_registry
                .consume_request_authorization(window_label, &input.request_id)
        {
            return Err(RepositoryJobError::WorkingTreeChanged);
        }
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
            return Ok(Arc::new(InactiveWorkingTreeCoordinator {
                cancellation: context.cancellation.clone(),
                inner: Arc::clone(&self.inner),
                notes_root,
            }));
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

struct InactiveWorkingTreeCoordinator {
    cancellation: super::service::JobCancellationToken,
    inner: Arc<PathGuardFactoryInner>,
    notes_root: PathBuf,
}

impl WorkingTreeCoordinator for InactiveWorkingTreeCoordinator {
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
            let relative_paths = validated_relative_paths(changes)?;
            let ownership = self
                .inner
                .native_registry
                .lease_ownership(self.notes_root.clone(), false);
            if self
                .inner
                .bridge
                .primary_notes_root()
                .is_some_and(|root| root == self.notes_root)
            {
                return Err(RepoError::WorkingTreeChanged);
            }
            let path_block = self
                .inner
                .native_registry
                .block_paths(&self.notes_root, &relative_paths)?;
            path_block.wait_for_existing().await;
            if self.cancellation.is_cancelled() {
                return Err(RepoError::Cancelled);
            }
            if self
                .inner
                .bridge
                .primary_notes_root()
                .is_some_and(|root| root == self.notes_root)
            {
                return Err(RepoError::WorkingTreeChanged);
            }
            Ok(WorkingTreePermit::new(ProductPathGuardPermit {
                _ownership: ownership,
                _path_block: path_block,
                _published: None,
            }))
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
            let relative_paths = validated_relative_paths(changes)?;
            let ownership = self
                .inner
                .native_registry
                .lease_ownership(self.notes_root.clone(), true);
            if !self.owner_matches() {
                return Err(RepoError::WorkingTreeChanged);
            }
            let request_id = uuid::Uuid::new_v4().to_string();
            let path_block = self.inner.native_registry.block_paths_for_request(
                &self.notes_root,
                &relative_paths,
                self.inner.bridge.primary_window_label(),
                &request_id,
            )?;
            path_block.wait_for_existing().await;
            if self.cancellation.is_cancelled() {
                return Err(RepoError::Cancelled);
            }
            if !self.owner_matches() {
                return Err(RepoError::WorkingTreeChanged);
            }
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

            Ok(WorkingTreePermit::new(ProductPathGuardPermit {
                _ownership: ownership,
                _path_block: path_block,
                _published: Some(published),
            }))
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

struct ProductPathGuardPermit {
    _ownership: PrimaryOwnershipLease,
    _path_block: NativePathBlock,
    _published: Option<PublishedPathGuard>,
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
    PathGuardCoordinatorFactory::with_registry(
        Arc::new(TauriPathGuardBridge::new(app)),
        PATH_GUARD_TIMEOUT,
        Arc::clone(native_working_tree_registry()),
    )
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
        NativeWorkingTreeRegistry, PathGuardAcknowledgeInput, PathGuardCoordinatorFactory,
        PathGuardEventBridge, SyncPathGuardRelease, SyncPathGuardRequest, MAX_TRACKED_OWNED_JOBS,
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

    #[tokio::test]
    async fn an_inactive_coordinator_rejects_if_its_root_becomes_owned_before_prepare() {
        let inactive = tempdir().unwrap();
        let active = tempdir().unwrap();
        let root = inactive.path().canonicalize().unwrap();
        let (bridge, mut requests) = FakeBridge::new(Some(active.path().canonicalize().unwrap()));
        let factory = PathGuardCoordinatorFactory::new(bridge.clone(), Duration::from_secs(1));
        let attempt = context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac");
        let coordinator = factory.create(&attempt).unwrap();

        bridge.change_owner(Some(root.clone()));

        assert!(matches!(
            coordinator.prepare(&changes()).await,
            Err(RepoError::WorkingTreeChanged)
        ));
        assert!(requests.try_recv().is_err());

        let retry = factory.create(&attempt).unwrap();
        let prepare = tokio::spawn({
            let retry = Arc::clone(&retry);
            async move { retry.prepare(&changes()).await }
        });
        let request = requests.recv().await.unwrap();
        factory
            .acknowledge(
                "main",
                PathGuardAcknowledgeInput {
                    request_id: request.request_id,
                    notes_root: root,
                },
            )
            .unwrap();
        let permit = prepare.await.unwrap().unwrap();
        retry.release(permit).await;
    }

    #[tokio::test]
    async fn prepare_blocks_new_intersections_and_waits_only_for_affected_native_mutations() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let guarded_path = root.join("notes/first.md");
        std::fs::create_dir_all(guarded_path.parent().unwrap()).unwrap();
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let active = registry
            .acquire_mutation(&[guarded_path.clone()])
            .expect("the mutation should start before the guard");
        let (bridge, mut requests) = FakeBridge::new(Some(root.clone()));
        let factory = PathGuardCoordinatorFactory::with_registry(
            bridge,
            Duration::from_secs(1),
            registry.clone(),
        );
        let coordinator = factory
            .create(&context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac"))
            .unwrap();
        let prepare = tokio::spawn({
            let coordinator = Arc::clone(&coordinator);
            async move { coordinator.prepare(&changes()).await }
        });
        tokio::task::yield_now().await;

        assert!(requests.try_recv().is_err());
        assert_eq!(
            registry.acquire_mutation(&[guarded_path]).unwrap_err(),
            "sync-path-guarded"
        );
        let unrelated = registry
            .acquire_mutation(&[root.join("unrelated.md")])
            .expect("an unrelated mutation should continue");
        drop(unrelated);

        drop(active);
        let request = requests.recv().await.unwrap();
        factory
            .acknowledge(
                "main",
                PathGuardAcknowledgeInput {
                    request_id: request.request_id,
                    notes_root: root.clone(),
                },
            )
            .unwrap();
        let permit = prepare.await.unwrap().unwrap();
        assert_eq!(
            registry
                .acquire_mutation(&[root.join("notes")])
                .unwrap_err(),
            "sync-path-guarded"
        );
        coordinator.release(permit).await;
        assert!(registry
            .acquire_mutation(&[root.join("notes/first.md")])
            .is_ok());
    }

    #[tokio::test]
    async fn active_permit_rejects_primary_root_switch_until_release() {
        let owned = tempdir().unwrap();
        let other = tempdir().unwrap();
        let root = owned.path().canonicalize().unwrap();
        let other_root = other.path().canonicalize().unwrap();
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let (bridge, mut requests) = FakeBridge::new(Some(root.clone()));
        let factory = PathGuardCoordinatorFactory::with_registry(
            bridge,
            Duration::from_secs(1),
            registry.clone(),
        );
        let coordinator = factory
            .create(&context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac"))
            .unwrap();
        let prepare = tokio::spawn({
            let coordinator = Arc::clone(&coordinator);
            async move { coordinator.prepare(&changes()).await }
        });
        let request = requests.recv().await.unwrap();
        assert_eq!(
            registry
                .validate_primary_root(Some(&other_root))
                .unwrap_err(),
            "sync-path-guarded"
        );
        factory
            .acknowledge(
                "main",
                PathGuardAcknowledgeInput {
                    request_id: request.request_id,
                    notes_root: root,
                },
            )
            .unwrap();
        let permit = prepare.await.unwrap().unwrap();
        assert_eq!(
            registry
                .validate_primary_root(Some(&other_root))
                .unwrap_err(),
            "sync-path-guarded"
        );
        coordinator.release(permit).await;
        assert!(registry.validate_primary_root(Some(&other_root)).is_ok());
    }

    #[tokio::test]
    async fn acknowledgement_is_rejected_while_the_authorized_flush_is_still_writing() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let guarded = root.join("notes/first.md");
        std::fs::create_dir_all(guarded.parent().unwrap()).unwrap();
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let (bridge, mut requests) = FakeBridge::new(Some(root.clone()));
        let factory = PathGuardCoordinatorFactory::with_registry(
            bridge,
            Duration::from_secs(1),
            registry.clone(),
        );
        let coordinator = factory
            .create(&context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac"))
            .unwrap();
        let prepare = tokio::spawn({
            let coordinator = Arc::clone(&coordinator);
            async move { coordinator.prepare(&changes()).await }
        });
        let request = requests.recv().await.unwrap();
        let request_id = request.request_id.clone();
        let flush = registry
            .acquire_authorized_mutation(&[guarded.clone()], "main", &request_id)
            .unwrap();

        assert!(factory
            .acknowledge(
                "main",
                PathGuardAcknowledgeInput {
                    request_id: request_id.clone(),
                    notes_root: root.clone(),
                },
            )
            .is_err());
        drop(flush);
        factory
            .acknowledge(
                "main",
                PathGuardAcknowledgeInput {
                    request_id: request_id.clone(),
                    notes_root: root,
                },
            )
            .unwrap();
        let permit = prepare.await.unwrap().unwrap();
        assert_eq!(
            registry
                .acquire_authorized_mutation(&[guarded], "main", &request_id)
                .unwrap_err(),
            "sync-path-guarded"
        );
        coordinator.release(permit).await;
    }

    #[tokio::test]
    async fn inactive_permit_rejects_activating_its_root_until_release() {
        let inactive = tempdir().unwrap();
        let active = tempdir().unwrap();
        let root = inactive.path().canonicalize().unwrap();
        let active_root = active.path().canonicalize().unwrap();
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let (bridge, mut requests) = FakeBridge::new(Some(active_root.clone()));
        let factory = PathGuardCoordinatorFactory::with_registry(
            bridge,
            Duration::from_secs(1),
            registry.clone(),
        );
        let coordinator = factory
            .create(&context(&root, "319b5308-1e93-4909-95ac-cd198cc454ac"))
            .unwrap();

        let permit = coordinator.prepare(&changes()).await.unwrap();
        assert_eq!(
            registry.validate_primary_root(Some(&root)).unwrap_err(),
            "sync-path-guarded"
        );
        assert!(registry.validate_primary_root(Some(&active_root)).is_ok());
        assert!(requests.try_recv().is_err());
        coordinator.release(permit).await;
        assert!(registry.validate_primary_root(Some(&root)).is_ok());
    }

    #[test]
    fn native_mutation_paths_reject_dot_segments_before_canonicalizing_missing_suffixes() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let _block = registry
            .block_paths(&root, &["guarded.md".to_string()])
            .unwrap();

        assert_eq!(
            registry
                .acquire_mutation(&[root.join("missing/../guarded.md")])
                .unwrap_err(),
            "working-tree mutation path contains dot segments"
        );
        assert_eq!(
            registry
                .acquire_mutation(&[root.join("./guarded.md")])
                .unwrap_err(),
            "working-tree mutation path contains dot segments"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_guard_and_native_addresses_share_the_same_canonical_target() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let actual = root.join("actual");
        std::fs::create_dir(&actual).unwrap();
        symlink(&actual, root.join("linked")).unwrap();
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let _block = registry
            .block_paths(&root, &["linked/guarded.md".to_string()])
            .unwrap();

        assert_eq!(
            registry
                .acquire_mutation(&[actual.join("guarded.md")])
                .unwrap_err(),
            "sync-path-guarded"
        );
    }

    #[test]
    fn only_the_exact_primary_request_can_flush_its_own_guarded_path() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let guarded = root.join("guarded.md");
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let request_id = "e728a5d6-31ed-490d-bb8a-8f15cb550e74";
        let _block = registry
            .block_paths_for_request(&root, &["guarded.md".to_string()], "main", request_id)
            .unwrap();

        assert!(registry
            .acquire_authorized_mutation(&[guarded.clone()], "main", request_id)
            .is_ok());
        assert_eq!(
            registry.acquire_mutation(&[guarded.clone()]).unwrap_err(),
            "sync-path-guarded"
        );
        assert_eq!(
            registry
                .acquire_authorized_mutation(&[guarded.clone()], "settings", request_id)
                .unwrap_err(),
            "sync-path-guarded"
        );
        assert_eq!(
            registry
                .acquire_authorized_mutation(
                    &[guarded],
                    "main",
                    "a56fc2d3-b85e-4e72-883b-27d52573fda9",
                )
                .unwrap_err(),
            "sync-path-guarded"
        );
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
