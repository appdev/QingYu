//! Workspace service composition boundary.

use std::sync::{Arc, Weak};

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
        WorkspaceInitializationError, WorkspaceInitializationErrorKind,
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
}

impl WorkspaceService {
    pub fn new(
        runtime: &Arc<KernelRuntime>,
        store: Arc<dyn PrimaryWorkspaceStore>,
        managed: ManagedWorkspaceCollection,
        events: Arc<dyn EventSink>,
        initial_display_name: impl Into<String>,
    ) -> Result<Self, WorkspaceServiceError> {
        runtime
            .verify_instance_lock()
            .map_err(|_| WorkspaceServiceError::unavailable())?;
        let mutation = runtime
            .mutation_coordinator()
            .try_lock()
            .map_err(|_| WorkspaceServiceError::unavailable())?;
        let repository_binding = store.repository_binding();
        let initialization = runtime
            .workspace_initialization(&repository_binding, &mutation)
            .map_err(workspace_initialization_error)?;
        runtime.verify_workspace_initialization(&initialization)?;
        let previous = store.load()?;
        let (persisted, wrote_primary) = match previous.clone() {
            Some(value) => match serde_json::from_value::<PersistedPrimaryWorkspace>(value) {
                Ok(persisted) => (persisted, false),
                Err(_) => {
                    let _recovery =
                        runtime.enter_workspace_initialization_recovery(&initialization, &mutation);
                    return Err(WorkspaceServiceError::unavailable());
                }
            },
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
                    if store
                        .replace(previous.clone())
                        .and_then(|()| store.save())
                        .is_err()
                    {
                        let _recovery = runtime
                            .enter_workspace_initialization_recovery(&initialization, &mutation);
                    }
                    return Err(error.into());
                }
                (persisted, true)
            }
        };
        if validate_persisted_workspace(&persisted).is_err() {
            let _recovery =
                runtime.enter_workspace_initialization_recovery(&initialization, &mutation);
            return Err(WorkspaceServiceError::unavailable());
        }
        let current = match workspace_dto(runtime.instance_id(), &persisted) {
            Ok(current) => current,
            Err(error) => {
                let _recovery =
                    runtime.enter_workspace_initialization_recovery(&initialization, &mutation);
                return Err(error);
            }
        };
        if let Err(error) = runtime.verify_workspace_initialization(&initialization) {
            if wrote_primary {
                let _rollback = store.replace(previous.clone()).and_then(|()| store.save());
            }
            let _recovery =
                runtime.enter_workspace_initialization_recovery(&initialization, &mutation);
            return Err(error.into());
        }
        if let Err(error) = runtime.initialize_workspace_snapshot(
            &initialization,
            current,
            repository_binding,
            &mutation,
        ) {
            let _recovery =
                runtime.enter_workspace_initialization_recovery(&initialization, &mutation);
            return Err(workspace_initialization_error(error));
        }
        Ok(Self {
            runtime: Arc::downgrade(runtime),
            store,
            managed,
            events,
            mutation_coordinator: runtime.mutation_coordinator().clone(),
        })
    }

    pub fn current(&self) -> Result<WorkspaceDto, WorkspaceServiceError> {
        Ok(self
            .verified_runtime()?
            .active_workspace_snapshot()?
            .workspace()
            .clone())
    }

    pub async fn compare_and_set_host_workspace(
        &self,
        expected_revision: &Revision,
        prepared: PreparedWorkspaceAuthority,
        safe_display_name: impl Into<String>,
    ) -> Result<WorkspaceDto, WorkspaceServiceError> {
        let safe_display_name = safe_display_name.into();
        let mutation = self.mutation_coordinator.lock().await;
        let runtime = self.verified_runtime()?;
        let current = runtime.active_workspace_snapshot()?;
        if &current.workspace().revision != expected_revision {
            return Err(WorkspaceServiceError::revision_conflict(
                current.workspace().revision.clone(),
            ));
        }
        validate_display_name(&safe_display_name)?;
        if !current.matches_repository_binding(&self.store.repository_binding()) {
            return Err(WorkspaceServiceError::persistence_unavailable());
        }

        let previous_store_value = self.store.load()?;
        if !store_matches_workspace(
            runtime.instance_id(),
            previous_store_value.as_ref(),
            current.workspace(),
        ) {
            let _recovery = runtime.enter_workspace_recovery(&current, &prepared, &mutation);
            return Err(WorkspaceServiceError::persistence_unavailable());
        }
        runtime.verify_prepared_host_workspace_authority(&prepared)?;
        let persisted = PersistedPrimaryWorkspace {
            schema_version: PRIMARY_WORKSPACE_SCHEMA_VERSION,
            revision_seed: Uuid::new_v4().to_string(),
            display_name: safe_display_name,
        };
        let next_store_value =
            serde_json::to_value(&persisted).map_err(|_| WorkspaceServiceError::unavailable())?;
        self.store.replace(Some(next_store_value))?;
        if let Err(error) = self.store.save() {
            if self
                .restore_persisted_value(previous_store_value.clone())
                .is_err()
            {
                let _recovery = runtime.enter_workspace_recovery(&current, &prepared, &mutation);
            }
            return Err(error.into());
        }

        let committed = workspace_dto(runtime.instance_id(), &persisted)?;
        if let Err(error) = runtime.commit_workspace_snapshot(
            &current,
            &prepared,
            committed.clone(),
            current.repository_binding(),
            &mutation,
        ) {
            if self.restore_persisted_value(previous_store_value).is_err() {
                let _recovery = runtime.enter_workspace_recovery(&current, &prepared, &mutation);
                return Err(WorkspaceServiceError::persistence_unavailable());
            }
            return Err(error.into());
        }
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
    /// This remains an uncomposed staging seam. The runtime owner now commits
    /// authority and DTO as one snapshot and latches recovery process-wide,
    /// but production Tauri and server transports are intentionally not wired
    /// to this adapter in this phase.
    #[doc(hidden)]
    pub async fn compare_and_set_host_workspace_transaction(
        &self,
        expected_revision: &Revision,
        prepared: PreparedWorkspaceAuthority,
        safe_display_name: impl Into<String>,
        host_transaction: Box<dyn AtomicHostWorkspaceTransaction>,
    ) -> Result<WorkspaceDto, WorkspaceServiceError> {
        let safe_display_name = safe_display_name.into();
        let mutation = self.mutation_coordinator.lock().await;
        let runtime = self.verified_runtime()?;
        let current = runtime.active_workspace_snapshot()?;
        if &current.workspace().revision != expected_revision {
            return Err(WorkspaceServiceError::revision_conflict(
                current.workspace().revision.clone(),
            ));
        }
        validate_display_name(&safe_display_name)?;
        let repository_binding = self.store.repository_binding();
        if !current.matches_repository_binding(&repository_binding)
            || !current.matches_repository_binding(&host_transaction.repository_binding())
            || !prepared
                .binding()
                .matches(&host_transaction.authority_binding())
        {
            return Err(WorkspaceServiceError::persistence_unavailable());
        }

        let previous_store_value = self.store.load()?;
        if !store_matches_workspace(
            runtime.instance_id(),
            previous_store_value.as_ref(),
            current.workspace(),
        ) {
            let _recovery = runtime.enter_workspace_recovery(&current, &prepared, &mutation);
            return Err(WorkspaceServiceError::persistence_unavailable());
        }
        runtime.verify_prepared_host_workspace_authority(&prepared)?;
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
                    let _recovery =
                        runtime.enter_workspace_recovery(&current, &prepared, &mutation);
                    Err(WorkspaceServiceError::persistence_unavailable())
                }
            };
        }
        match self.store.load() {
            Ok(observed) if observed == Some(next_store_value) => {}
            Ok(_) | Err(_) => {
                let _recovery = runtime.enter_workspace_recovery(&current, &prepared, &mutation);
                return Err(WorkspaceServiceError::persistence_unavailable());
            }
        }

        if runtime
            .verify_prepared_host_workspace_authority(&prepared)
            .is_err()
        {
            let _recovery = runtime.enter_workspace_recovery(&current, &prepared, &mutation);
            return Err(WorkspaceServiceError::persistence_unavailable());
        }
        if runtime
            .commit_workspace_snapshot(
                &current,
                &prepared,
                committed.clone(),
                repository_binding,
                &mutation,
            )
            .is_err()
        {
            let _recovery = runtime.enter_workspace_recovery(&current, &prepared, &mutation);
            return Err(WorkspaceServiceError::persistence_unavailable());
        }
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
        .active_workspace_snapshot()
        .map(|_| ())
        .map_err(WorkspaceServiceError::from)
}

fn store_matches_workspace(
    instance_id: InstanceId,
    persisted: Option<&serde_json::Value>,
    current: &WorkspaceDto,
) -> bool {
    persisted
        .cloned()
        .and_then(|value| serde_json::from_value::<PersistedPrimaryWorkspace>(value).ok())
        .filter(|value| validate_persisted_workspace(value).is_ok())
        .and_then(|value| workspace_dto(instance_id, &value).ok())
        .as_ref()
        .is_some_and(|observed| observed == current)
}

fn workspace_initialization_error(error: WorkspaceInitializationError) -> WorkspaceServiceError {
    match error.kind() {
        WorkspaceInitializationErrorKind::ForeignRepository
        | WorkspaceInitializationErrorKind::ChangedCanonical => {
            WorkspaceServiceError::persistence_unavailable()
        }
        WorkspaceInitializationErrorKind::RecoveryRequired
        | WorkspaceInitializationErrorKind::Unavailable => WorkspaceServiceError::unavailable(),
    }
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
