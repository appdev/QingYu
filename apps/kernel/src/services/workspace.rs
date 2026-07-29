//! Workspace service composition boundary.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Weak,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    contract::{
        DomainEvent, ErrorCode, ErrorDetails, InstanceId, ResourceRefDto, Revision, WorkspaceDto,
        WorkspaceGeneration, WorkspaceId, WorkspaceReadiness,
    },
    events::{EventPublication, EventSink},
    runtime::{
        KernelRuntime, MutationCoordinator, PreparedWorkspaceAuthority, ServiceFailure,
        WorkspaceApiService, WorkspaceAuthorityError, WorkspaceAuthorityErrorKind,
    },
    workspace::{
        managed::{ManagedWorkspaceCollection, ManagedWorkspaceError, ManagedWorkspaceErrorKind},
        primary::{
            AtomicHostWorkspaceCommitErrorKind, AtomicHostWorkspaceTransaction,
            PrimaryWorkspaceStore, PrimaryWorkspaceStoreError,
        },
    },
};

const PRIMARY_WORKSPACE_SCHEMA_VERSION: u64 = 1;

pub struct WorkspaceService {
    runtime: Weak<KernelRuntime>,
    store: Arc<dyn PrimaryWorkspaceStore>,
    managed: ManagedWorkspaceCollection,
    events: Arc<dyn EventSink>,
    mutation_coordinator: Arc<MutationCoordinator>,
    current: Mutex<WorkspaceDto>,
    uncertain_host_commit: AtomicBool,
}

impl WorkspaceService {
    pub fn new(
        runtime: &Arc<KernelRuntime>,
        store: Arc<dyn PrimaryWorkspaceStore>,
        managed: ManagedWorkspaceCollection,
        events: Arc<dyn EventSink>,
        initial_display_name: impl Into<String>,
    ) -> Result<Self, WorkspaceServiceError> {
        verify_runtime(runtime)?;
        let previous = store.load()?;
        let persisted = match previous.clone() {
            Some(value) => serde_json::from_value::<PersistedPrimaryWorkspace>(value)
                .map_err(|_| WorkspaceServiceError::unavailable())?,
            None => {
                let persisted = PersistedPrimaryWorkspace {
                    schema_version: PRIMARY_WORKSPACE_SCHEMA_VERSION,
                    revision_seed: Uuid::new_v4().to_string(),
                    display_name: initial_display_name.into(),
                };
                validate_persisted_workspace(&persisted)?;
                store.replace(Some(
                    serde_json::to_value(&persisted)
                        .map_err(|_| WorkspaceServiceError::unavailable())?,
                ))?;
                if let Err(error) = store.save() {
                    let _restore_result = store.replace(previous);
                    return Err(error.into());
                }
                persisted
            }
        };
        validate_persisted_workspace(&persisted)?;
        let current = workspace_dto(runtime.instance_id(), &persisted)?;
        Ok(Self {
            runtime: Arc::downgrade(runtime),
            store,
            managed,
            events,
            mutation_coordinator: runtime.mutation_coordinator().clone(),
            current: Mutex::new(current),
            uncertain_host_commit: AtomicBool::new(false),
        })
    }

    pub fn current(&self) -> Result<WorkspaceDto, WorkspaceServiceError> {
        self.verified_runtime()?;
        self.current
            .lock()
            .map(|current| current.clone())
            .map_err(|_| WorkspaceServiceError::unavailable())
    }

    pub async fn compare_and_set_host_workspace(
        &self,
        expected_revision: &Revision,
        prepared: PreparedWorkspaceAuthority,
        safe_display_name: impl Into<String>,
    ) -> Result<WorkspaceDto, WorkspaceServiceError> {
        let safe_display_name = safe_display_name.into();
        let _mutation = self.mutation_coordinator.lock().await;
        let runtime = self.verified_runtime()?;
        let mut current = self
            .current
            .lock()
            .map_err(|_| WorkspaceServiceError::unavailable())?;
        if &current.revision != expected_revision {
            return Err(WorkspaceServiceError::revision_conflict(
                current.revision.clone(),
            ));
        }
        validate_display_name(&safe_display_name)?;

        let previous_store_value = self.store.load()?;
        let persisted = PersistedPrimaryWorkspace {
            schema_version: PRIMARY_WORKSPACE_SCHEMA_VERSION,
            revision_seed: Uuid::new_v4().to_string(),
            display_name: safe_display_name,
        };
        let next_store_value =
            serde_json::to_value(&persisted).map_err(|_| WorkspaceServiceError::unavailable())?;
        self.store.replace(Some(next_store_value))?;
        if let Err(error) = self.store.save() {
            let _restore_result = self.store.replace(previous_store_value);
            return Err(error.into());
        }

        if let Err(error) = runtime.commit_host_workspace_authority(prepared) {
            self.restore_persisted_value(previous_store_value)?;
            return Err(error.into());
        }

        let committed = workspace_dto(runtime.instance_id(), &persisted)?;
        *current = committed.clone();
        drop(current);
        let publication = EventPublication {
            resource: ResourceRefDto::Workspace { id: committed.id },
            revision: committed.revision.clone(),
            event: DomainEvent::WorkspaceChanged {
                workspace: committed.clone(),
            },
        };
        let _publication_result = self.events.publish(&publication);
        Ok(committed)
    }

    /// Commits one host-selected workspace using an atomic transaction over
    /// the host's existing durable primary-workspace record.
    ///
    /// The transaction is consumed and has no compensating rollback. Unlike
    /// `compare_and_set_host_workspace`, this path never performs a second
    /// `PrimaryWorkspaceStore::save`; the transaction must codec the supplied
    /// canonical value into the same record observed by `self.store`.
    ///
    /// This is an uncomposed staging seam. It must not be connected to live
    /// document traffic until the runtime-owner cut unifies authority and DTO
    /// snapshots; that later boundary removes the current cross-lock snapshot
    /// window.
    #[doc(hidden)]
    pub async fn compare_and_set_host_workspace_transaction(
        &self,
        expected_revision: &Revision,
        prepared: PreparedWorkspaceAuthority,
        safe_display_name: impl Into<String>,
        host_transaction: Box<dyn AtomicHostWorkspaceTransaction>,
    ) -> Result<WorkspaceDto, WorkspaceServiceError> {
        let safe_display_name = safe_display_name.into();
        let _mutation = self.mutation_coordinator.lock().await;
        let runtime = self.verified_runtime()?;
        let mut current = self
            .current
            .lock()
            .map_err(|_| WorkspaceServiceError::unavailable())?;
        if &current.revision != expected_revision {
            return Err(WorkspaceServiceError::revision_conflict(
                current.revision.clone(),
            ));
        }
        validate_display_name(&safe_display_name)?;
        let installation = runtime.pin_host_workspace_authority_installation(prepared)?;

        let previous_store_value = self.store.load()?;
        let persisted = PersistedPrimaryWorkspace {
            schema_version: PRIMARY_WORKSPACE_SCHEMA_VERSION,
            revision_seed: Uuid::new_v4().to_string(),
            display_name: safe_display_name,
        };
        let next_store_value =
            serde_json::to_value(&persisted).map_err(|_| WorkspaceServiceError::unavailable())?;
        let committed = workspace_dto(runtime.instance_id(), &persisted)?;
        if let Err(error) = host_transaction
            .compare_and_commit(previous_store_value.as_ref(), next_store_value.clone())
        {
            return match error.kind() {
                AtomicHostWorkspaceCommitErrorKind::Conflict => {
                    Err(WorkspaceServiceError::persistence_unavailable())
                }
                AtomicHostWorkspaceCommitErrorKind::NoCommit => {
                    Err(WorkspaceServiceError::persistence_unavailable())
                }
                AtomicHostWorkspaceCommitErrorKind::OutcomeUnknown => {
                    self.uncertain_host_commit.store(true, Ordering::SeqCst);
                    Err(WorkspaceServiceError::persistence_unavailable())
                }
            };
        }
        match self.store.load() {
            Ok(observed) if observed == Some(next_store_value) => {}
            Ok(_) | Err(_) => {
                self.uncertain_host_commit.store(true, Ordering::SeqCst);
                return Err(WorkspaceServiceError::persistence_unavailable());
            }
        }

        if installation.install().is_err() {
            self.uncertain_host_commit.store(true, Ordering::SeqCst);
            return Err(WorkspaceServiceError::persistence_unavailable());
        }
        *current = committed.clone();
        drop(current);
        let publication = EventPublication {
            resource: ResourceRefDto::Workspace { id: committed.id },
            revision: committed.revision.clone(),
            event: DomainEvent::WorkspaceChanged {
                workspace: committed.clone(),
            },
        };
        let _publication_result = self.events.publish(&publication);
        Ok(committed)
    }

    pub async fn create_managed_workspace(
        &self,
        name: &str,
    ) -> Result<String, WorkspaceServiceError> {
        let _mutation = self.mutation_coordinator.lock().await;
        self.verified_runtime()?;
        self.managed
            .create(name)
            .map_err(WorkspaceServiceError::from)
    }

    pub fn list_managed_workspaces(&self) -> Result<Vec<String>, WorkspaceServiceError> {
        self.verified_runtime()?;
        self.managed.list().map_err(WorkspaceServiceError::from)
    }

    fn verified_runtime(&self) -> Result<Arc<KernelRuntime>, WorkspaceServiceError> {
        if self.uncertain_host_commit.load(Ordering::SeqCst) {
            return Err(WorkspaceServiceError::unavailable());
        }
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(WorkspaceServiceError::unavailable)?;
        verify_runtime(&runtime)?;
        Ok(runtime)
    }

    fn restore_persisted_value(
        &self,
        previous: Option<serde_json::Value>,
    ) -> Result<(), WorkspaceServiceError> {
        self.store.replace(previous)?;
        self.store.save()?;
        Ok(())
    }
}

fn verify_runtime(runtime: &KernelRuntime) -> Result<(), WorkspaceServiceError> {
    runtime
        .verify_instance_lock()
        .map_err(|_| WorkspaceServiceError::unavailable())?;
    runtime
        .active_workspace_authority()
        .verify_held_directory()
        .map_err(WorkspaceServiceError::from)
}

#[async_trait]
impl WorkspaceApiService for WorkspaceService {
    async fn get_workspace(&self) -> Result<WorkspaceDto, ServiceFailure> {
        self.current().map_err(service_failure)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PersistedPrimaryWorkspace {
    schema_version: u64,
    revision_seed: String,
    display_name: String,
}

fn workspace_dto(
    instance_id: InstanceId,
    persisted: &PersistedPrimaryWorkspace,
) -> Result<WorkspaceDto, WorkspaceServiceError> {
    let persisted_bytes =
        serde_json::to_vec(persisted).map_err(|_| WorkspaceServiceError::unavailable())?;
    let revision = Revision::parse(format!("{:x}", Sha256::digest(persisted_bytes)))
        .map_err(|_| WorkspaceServiceError::unavailable())?;
    Ok(WorkspaceDto {
        id: WorkspaceId::new(logical_uuid(instance_id, b"workspace-id", persisted)?),
        generation: WorkspaceGeneration::parse(
            logical_uuid(instance_id, b"workspace-generation", persisted)?.to_string(),
        )
        .map_err(|_| WorkspaceServiceError::unavailable())?,
        display_name: persisted.display_name.clone(),
        readiness: WorkspaceReadiness::Ready,
        revision,
    })
}

fn logical_uuid(
    instance_id: InstanceId,
    purpose: &[u8],
    persisted: &PersistedPrimaryWorkspace,
) -> Result<Uuid, WorkspaceServiceError> {
    let mut hasher = Sha256::new();
    hasher.update(instance_id.as_uuid().as_bytes());
    hasher.update(purpose);
    hasher.update(serde_json::to_vec(persisted).map_err(|_| WorkspaceServiceError::unavailable())?);
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(Uuid::from_bytes(bytes))
}

fn validate_persisted_workspace(
    persisted: &PersistedPrimaryWorkspace,
) -> Result<(), WorkspaceServiceError> {
    if persisted.schema_version != PRIMARY_WORKSPACE_SCHEMA_VERSION
        || persisted.revision_seed.is_empty()
    {
        return Err(WorkspaceServiceError::unavailable());
    }
    validate_display_name(&persisted.display_name).map_err(|_| WorkspaceServiceError::unavailable())
}

fn validate_display_name(value: &str) -> Result<(), WorkspaceServiceError> {
    if value.is_empty()
        || value.chars().count() > 255
        || value.chars().any(char::is_control)
        || value.contains(['/', '\\'])
    {
        return Err(WorkspaceServiceError::invalid_workspace());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceServiceErrorKind {
    InvalidWorkspace,
    RevisionConflict,
    PersistenceUnavailable,
    WorkspaceLocked,
    WorkspaceUnavailable,
    UnsafeManagedWorkspace,
    UnsupportedProfile,
    PreparedAuthorityMismatch,
}

#[derive(Clone, Eq, PartialEq)]
pub struct WorkspaceServiceError {
    kind: WorkspaceServiceErrorKind,
    current_revision: Option<Revision>,
}

impl WorkspaceServiceError {
    pub const fn unavailable() -> Self {
        Self {
            kind: WorkspaceServiceErrorKind::WorkspaceUnavailable,
            current_revision: None,
        }
    }

    pub const fn invalid_workspace() -> Self {
        Self {
            kind: WorkspaceServiceErrorKind::InvalidWorkspace,
            current_revision: None,
        }
    }

    fn revision_conflict(current_revision: Revision) -> Self {
        Self {
            kind: WorkspaceServiceErrorKind::RevisionConflict,
            current_revision: Some(current_revision),
        }
    }

    pub const fn prepared_authority_mismatch() -> Self {
        Self {
            kind: WorkspaceServiceErrorKind::PreparedAuthorityMismatch,
            current_revision: None,
        }
    }

    const fn persistence_unavailable() -> Self {
        Self {
            kind: WorkspaceServiceErrorKind::PersistenceUnavailable,
            current_revision: None,
        }
    }

    pub const fn kind(&self) -> WorkspaceServiceErrorKind {
        self.kind
    }

    pub const fn current_revision(&self) -> Option<&Revision> {
        self.current_revision.as_ref()
    }
}

impl From<PrimaryWorkspaceStoreError> for WorkspaceServiceError {
    fn from(_error: PrimaryWorkspaceStoreError) -> Self {
        Self {
            kind: WorkspaceServiceErrorKind::PersistenceUnavailable,
            current_revision: None,
        }
    }
}

impl From<ManagedWorkspaceError> for WorkspaceServiceError {
    fn from(error: ManagedWorkspaceError) -> Self {
        let kind = match error.kind() {
            ManagedWorkspaceErrorKind::InvalidName => WorkspaceServiceErrorKind::InvalidWorkspace,
            ManagedWorkspaceErrorKind::UnsafeEntry => {
                WorkspaceServiceErrorKind::UnsafeManagedWorkspace
            }
            ManagedWorkspaceErrorKind::Unavailable => {
                WorkspaceServiceErrorKind::WorkspaceUnavailable
            }
        };
        Self {
            kind,
            current_revision: None,
        }
    }
}

impl From<WorkspaceAuthorityError> for WorkspaceServiceError {
    fn from(error: WorkspaceAuthorityError) -> Self {
        let kind = match error.kind() {
            WorkspaceAuthorityErrorKind::UnsupportedProfile => {
                WorkspaceServiceErrorKind::UnsupportedProfile
            }
            WorkspaceAuthorityErrorKind::PreparedAuthorityMismatch => {
                WorkspaceServiceErrorKind::PreparedAuthorityMismatch
            }
            WorkspaceAuthorityErrorKind::WorkspaceLocked => {
                WorkspaceServiceErrorKind::WorkspaceLocked
            }
            WorkspaceAuthorityErrorKind::WorkspaceUnavailable
            | WorkspaceAuthorityErrorKind::OverlappingRoots
            | WorkspaceAuthorityErrorKind::UnsafeEntry => {
                WorkspaceServiceErrorKind::WorkspaceUnavailable
            }
        };
        Self {
            kind,
            current_revision: None,
        }
    }
}

impl std::fmt::Debug for WorkspaceServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkspaceServiceError")
            .field("kind", &self.kind)
            .field("current_revision", &self.current_revision)
            .finish()
    }
}

impl std::fmt::Display for WorkspaceServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the workspace service is unavailable")
    }
}

impl std::error::Error for WorkspaceServiceError {}

fn service_failure(error: WorkspaceServiceError) -> ServiceFailure {
    let (code, details) = match error.kind() {
        WorkspaceServiceErrorKind::InvalidWorkspace => (ErrorCode::InvalidWorkspacePath, None),
        WorkspaceServiceErrorKind::RevisionConflict => (
            ErrorCode::RevisionConflict,
            Some(ErrorDetails::RevisionConflict {
                current_revision: error.current_revision().cloned(),
            }),
        ),
        WorkspaceServiceErrorKind::WorkspaceLocked => (ErrorCode::WorkspaceLocked, None),
        WorkspaceServiceErrorKind::UnsupportedProfile => (ErrorCode::HostNotAllowed, None),
        WorkspaceServiceErrorKind::PersistenceUnavailable
        | WorkspaceServiceErrorKind::WorkspaceUnavailable
        | WorkspaceServiceErrorKind::UnsafeManagedWorkspace
        | WorkspaceServiceErrorKind::PreparedAuthorityMismatch => {
            (ErrorCode::WorkspaceUnavailable, None)
        }
    };
    ServiceFailure::new(code, details)
        .expect("workspace service errors use compatible public error details")
}
