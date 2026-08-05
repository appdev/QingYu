//! Desktop-local writer ownership fence for the active child-Kernel runtime.

#![cfg_attr(not(test), allow(dead_code))]

use std::{
    ffi::OsString,
    io,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard},
};

use cap_fs_ext::DirExt as _;
use cap_std::fs::{Dir, File};

struct WorkspaceRootCapability {
    canonical_path: PathBuf,
    directory: Dir,
    identity: crate::storage_capability::DirectoryIdentity,
    parent: Dir,
    parent_identity: crate::storage_capability::DirectoryIdentity,
    name: OsString,
}

#[derive(Clone)]
pub(crate) struct WorkspaceRootIdentity(Arc<WorkspaceRootCapability>);

impl WorkspaceRootIdentity {
    pub(crate) fn open(path: &Path) -> Result<Self, WriterAuthorityError> {
        let canonical_path = path
            .canonicalize()
            .map_err(|_| WriterAuthorityError::WorkspaceRootUnavailable)?;
        let parent_path = canonical_path
            .parent()
            .ok_or(WriterAuthorityError::WorkspaceRootUnavailable)?;
        let name = canonical_path
            .file_name()
            .ok_or(WriterAuthorityError::WorkspaceRootUnavailable)?
            .to_os_string();
        let parent = crate::storage_capability::open_canonical_directory_nofollow(parent_path)
            .map_err(|_| WriterAuthorityError::WorkspaceRootUnavailable)?;
        let parent_identity = crate::storage_capability::directory_identity(&parent)
            .map_err(|_| WriterAuthorityError::WorkspaceRootUnavailable)?;
        let directory = parent
            .open_dir_nofollow(&name)
            .map_err(|_| WriterAuthorityError::WorkspaceRootUnavailable)?;
        let identity = crate::storage_capability::directory_identity(&directory)
            .map_err(|_| WriterAuthorityError::WorkspaceRootUnavailable)?;
        Ok(Self(Arc::new(WorkspaceRootCapability {
            canonical_path,
            directory,
            identity,
            parent,
            parent_identity,
            name,
        })))
    }

    fn is_current(&self) -> bool {
        if !crate::storage_capability::directory_identity(&self.0.directory)
            .is_ok_and(|identity| identity == self.0.identity)
            || !crate::storage_capability::directory_identity(&self.0.parent)
                .is_ok_and(|identity| identity == self.0.parent_identity)
        {
            return false;
        }
        let Some(parent_path) = self.0.canonical_path.parent() else {
            return false;
        };
        let Ok(addressed_parent) =
            crate::storage_capability::open_canonical_directory_nofollow(parent_path)
        else {
            return false;
        };
        if !crate::storage_capability::directory_identity(&addressed_parent)
            .is_ok_and(|identity| identity == self.0.parent_identity)
        {
            return false;
        }
        addressed_parent
            .open_dir_nofollow(&self.0.name)
            .and_then(|addressed| crate::storage_capability::directory_identity(&addressed))
            .is_ok_and(|identity| identity == self.0.identity)
    }

    fn same_current_root(&self, candidate: &Self) -> bool {
        self.is_current()
            && candidate.is_current()
            && self.0.canonical_path == candidate.0.canonical_path
            && self.0.identity == candidate.0.identity
    }
}

impl std::fmt::Debug for WorkspaceRootIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WorkspaceRootIdentity([OPAQUE])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WriterAuthorityState {
    Legacy,
    Transitioning,
    Kernel(KernelGeneration),
    FailedClosed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KernelGeneration(u64);

impl KernelGeneration {
    pub(crate) fn new(value: u64) -> Result<Self, WriterAuthorityError> {
        (value != 0)
            .then_some(Self(value))
            .ok_or(WriterAuthorityError::InvalidGeneration)
    }

    fn is_after(self, previous: Self) -> bool {
        self.0 > previous.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WriterAuthorityError {
    FailedClosed,
    InvalidGeneration,
    InvalidTransition,
    KernelClaimOutstanding,
    KernelGenerationMismatch,
    LegacyWritersActive(usize),
    LegacyWriterRejected,
    NonMonotonicGeneration,
    WorkspaceRootMismatch,
    WorkspaceRootUnavailable,
    Poisoned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WriterAuthoritySnapshot {
    pub(crate) state: WriterAuthorityState,
    pub(crate) active_legacy_writers: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriterAuthorityPhase {
    Legacy,
    Transitioning {
        target: KernelGeneration,
        claim_nonce: Option<u64>,
    },
    Kernel(KernelGeneration),
    FailedClosed,
}

impl WriterAuthorityPhase {
    fn public_state(self) -> WriterAuthorityState {
        match self {
            Self::Legacy => WriterAuthorityState::Legacy,
            Self::Transitioning { .. } => WriterAuthorityState::Transitioning,
            Self::Kernel(generation) => WriterAuthorityState::Kernel(generation),
            Self::FailedClosed => WriterAuthorityState::FailedClosed,
        }
    }
}

struct WriterAuthorityInner {
    phase: WriterAuthorityPhase,
    active_legacy_writers: usize,
    next_claim_nonce: u64,
}

struct WriterAuthorityShared {
    root: WorkspaceRootIdentity,
    inner: Mutex<WriterAuthorityInner>,
    legacy_drained: Condvar,
}

struct WorkspaceOperationFailClosedGuard<'a> {
    shared: &'a WriterAuthorityShared,
    armed: bool,
}

pub(crate) struct WorkspaceMutationRoot<'scope> {
    directory: &'scope Dir,
    // Invariance prevents a scoped capability from being widened to `'static`.
    scope: PhantomData<&'scope mut &'scope ()>,
}

impl<'scope> WorkspaceMutationRoot<'scope> {
    fn new(directory: &'scope Dir) -> Self {
        Self {
            directory,
            scope: PhantomData,
        }
    }

    pub(crate) fn create(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> io::Result<WorkspaceMutationFile<'scope>> {
        self.directory
            .create(relative_path.as_ref())
            .map(WorkspaceMutationFile::new)
    }
}

pub(crate) struct WorkspaceMutationFile<'scope> {
    file: File,
    // The owned OS handle remains branded with the enclosing operation scope.
    scope: PhantomData<&'scope mut &'scope ()>,
}

impl WorkspaceMutationFile<'_> {
    fn new(file: File) -> Self {
        Self {
            file,
            scope: PhantomData,
        }
    }

    pub(crate) fn sync_all(&self) -> io::Result<()> {
        self.file.sync_all()
    }

    pub(crate) fn sync_data(&self) -> io::Result<()> {
        self.file.sync_data()
    }
}

impl io::Write for WorkspaceMutationFile<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        io::Write::write(&mut self.file, buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.file)
    }
}

impl WorkspaceOperationFailClosedGuard<'_> {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for WorkspaceOperationFailClosedGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.shared.fail_closed();
        }
    }
}

impl WriterAuthorityShared {
    fn mark_failed_closed<'a>(
        &self,
        mut inner: MutexGuard<'a, WriterAuthorityInner>,
    ) -> MutexGuard<'a, WriterAuthorityInner> {
        inner.phase = WriterAuthorityPhase::FailedClosed;
        self.legacy_drained.notify_all();
        inner
    }

    fn ensure_root_current(&self) -> Result<(), WriterAuthorityError> {
        if self.root.is_current() {
            return Ok(());
        }
        self.fail_closed();
        Err(WriterAuthorityError::WorkspaceRootUnavailable)
    }

    fn latch_poisoned(&self, inner: MutexGuard<'_, WriterAuthorityInner>) -> WriterAuthorityError {
        drop(self.mark_failed_closed(inner));
        WriterAuthorityError::Poisoned
    }

    fn lock_inner(&self) -> Result<MutexGuard<'_, WriterAuthorityInner>, WriterAuthorityError> {
        self.inner
            .lock()
            .map_err(|poisoned| self.latch_poisoned(poisoned.into_inner()))
    }

    fn fail_closed(&self) {
        match self.inner.lock() {
            Ok(mut inner) => inner.phase = WriterAuthorityPhase::FailedClosed,
            Err(poisoned) => {
                drop(self.mark_failed_closed(poisoned.into_inner()));
            }
        }
        self.legacy_drained.notify_all();
    }

    fn with_retained_workspace_root<Output, OperationError>(
        &self,
        operation: impl for<'scope> FnOnce(
            WorkspaceMutationRoot<'scope>,
        ) -> Result<Output, OperationError>,
    ) -> Result<Output, WorkspaceWriterOperationError<OperationError>> {
        self.ensure_root_current()
            .map_err(WorkspaceWriterOperationError::Authority)?;
        let mut fail_closed = WorkspaceOperationFailClosedGuard {
            shared: self,
            armed: true,
        };
        let result = operation(WorkspaceMutationRoot::new(&self.root.0.directory));
        self.ensure_root_current()
            .map_err(WorkspaceWriterOperationError::Authority)?;
        fail_closed.disarm();
        result.map_err(WorkspaceWriterOperationError::Operation)
    }

    fn ensure_legacy_operation_authorized(&self) -> Result<(), WriterAuthorityError> {
        let mut inner = self.lock_inner()?;
        match inner.phase {
            WriterAuthorityPhase::Legacy | WriterAuthorityPhase::Transitioning { .. }
                if inner.active_legacy_writers != 0 =>
            {
                Ok(())
            }
            WriterAuthorityPhase::FailedClosed => Err(WriterAuthorityError::FailedClosed),
            _ => {
                inner.phase = WriterAuthorityPhase::FailedClosed;
                self.legacy_drained.notify_all();
                Err(WriterAuthorityError::InvalidTransition)
            }
        }
    }

    fn ensure_kernel_operation_authorized(
        &self,
        generation: KernelGeneration,
    ) -> Result<(), WriterAuthorityError> {
        let mut inner = self.lock_inner()?;
        match inner.phase {
            WriterAuthorityPhase::Kernel(current) if current == generation => Ok(()),
            WriterAuthorityPhase::FailedClosed => Err(WriterAuthorityError::FailedClosed),
            WriterAuthorityPhase::Kernel(_) => {
                inner.phase = WriterAuthorityPhase::FailedClosed;
                self.legacy_drained.notify_all();
                Err(WriterAuthorityError::KernelGenerationMismatch)
            }
            _ => {
                inner.phase = WriterAuthorityPhase::FailedClosed;
                self.legacy_drained.notify_all();
                Err(WriterAuthorityError::InvalidTransition)
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct WriterAuthority {
    shared: Arc<WriterAuthorityShared>,
}

#[derive(Debug)]
pub(crate) enum WorkspaceWriterOperationError<OperationError> {
    Authority(WriterAuthorityError),
    Operation(OperationError),
}

fn finish_workspace_writer_operation<Output, OperationError>(
    result: Result<Output, WorkspaceWriterOperationError<OperationError>>,
    post_authorization: Result<(), WriterAuthorityError>,
) -> Result<Output, WorkspaceWriterOperationError<OperationError>> {
    if matches!(result, Err(WorkspaceWriterOperationError::Authority(_))) {
        return result;
    }
    post_authorization.map_err(WorkspaceWriterOperationError::Authority)?;
    result
}

pub(crate) struct LegacyWriterLease {
    shared: Arc<WriterAuthorityShared>,
    active: bool,
}

pub(crate) struct KernelWriterClaim {
    shared: Arc<WriterAuthorityShared>,
    generation: KernelGeneration,
    nonce: u64,
    active: bool,
}

impl std::fmt::Debug for KernelWriterClaim {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("KernelWriterClaim([OPAQUE])")
    }
}

impl KernelWriterClaim {
    pub(crate) fn publish(mut self) -> Result<KernelWriterLease, WriterAuthorityError> {
        self.shared.ensure_root_current()?;
        let result = match self.shared.lock_inner() {
            Ok(mut inner) => match inner.phase {
                WriterAuthorityPhase::Transitioning {
                    target,
                    claim_nonce: Some(nonce),
                } if target == self.generation && nonce == self.nonce => {
                    inner.phase = WriterAuthorityPhase::Kernel(self.generation);
                    Ok(KernelWriterLease {
                        shared: Arc::clone(&self.shared),
                        generation: self.generation,
                        active: true,
                    })
                }
                _ => {
                    inner.phase = WriterAuthorityPhase::FailedClosed;
                    Err(WriterAuthorityError::InvalidTransition)
                }
            },
            Err(error) => Err(error),
        };
        if result.is_ok() {
            self.active = false;
        }
        self.shared.legacy_drained.notify_all();
        result
    }
}

impl Drop for KernelWriterClaim {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.shared.fail_closed();
        self.active = false;
        self.shared.legacy_drained.notify_all();
    }
}

pub(crate) struct KernelWriterLease {
    shared: Arc<WriterAuthorityShared>,
    generation: KernelGeneration,
    active: bool,
}

impl std::fmt::Debug for KernelWriterLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("KernelWriterLease([OPAQUE])")
    }
}

impl KernelWriterLease {
    pub(crate) fn generation(&self) -> KernelGeneration {
        self.generation
    }

    pub(crate) fn with_workspace_root<Output, OperationError>(
        &self,
        operation: impl for<'scope> FnOnce(
            WorkspaceMutationRoot<'scope>,
        ) -> Result<Output, OperationError>,
    ) -> Result<Output, WorkspaceWriterOperationError<OperationError>> {
        self.shared
            .ensure_kernel_operation_authorized(self.generation)
            .map_err(WorkspaceWriterOperationError::Authority)?;
        let result = self.shared.with_retained_workspace_root(operation);
        let post_authorization = self
            .shared
            .ensure_kernel_operation_authorized(self.generation);
        finish_workspace_writer_operation(result, post_authorization)
    }

    pub(crate) fn begin_recovery(
        mut self,
        replacement: KernelGeneration,
    ) -> Result<(), WriterAuthorityError> {
        self.shared.ensure_root_current()?;
        if !replacement.is_after(self.generation) {
            self.shared.fail_closed();
            return Err(WriterAuthorityError::NonMonotonicGeneration);
        }
        let result = match self.shared.lock_inner() {
            Ok(mut inner) => match inner.phase {
                WriterAuthorityPhase::Kernel(current) if current == self.generation => {
                    inner.phase = WriterAuthorityPhase::Transitioning {
                        target: replacement,
                        claim_nonce: None,
                    };
                    Ok(())
                }
                WriterAuthorityPhase::FailedClosed => Err(WriterAuthorityError::FailedClosed),
                _ => {
                    inner.phase = WriterAuthorityPhase::FailedClosed;
                    Err(WriterAuthorityError::InvalidTransition)
                }
            },
            Err(error) => Err(error),
        };
        if result.is_ok() {
            self.active = false;
        }
        self.shared.legacy_drained.notify_all();
        result
    }
}

impl Drop for KernelWriterLease {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.shared.fail_closed();
        self.active = false;
        self.shared.legacy_drained.notify_all();
    }
}

impl std::fmt::Debug for LegacyWriterLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LegacyWriterLease([OPAQUE])")
    }
}

impl LegacyWriterLease {
    pub(crate) fn with_workspace_root<Output, OperationError>(
        &self,
        operation: impl for<'scope> FnOnce(
            WorkspaceMutationRoot<'scope>,
        ) -> Result<Output, OperationError>,
    ) -> Result<Output, WorkspaceWriterOperationError<OperationError>> {
        self.shared
            .ensure_legacy_operation_authorized()
            .map_err(WorkspaceWriterOperationError::Authority)?;
        let result = self.shared.with_retained_workspace_root(operation);
        let post_authorization = self.shared.ensure_legacy_operation_authorized();
        finish_workspace_writer_operation(result, post_authorization)
    }
}

impl Drop for LegacyWriterLease {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut inner) = self.shared.lock_inner() {
            if inner.active_legacy_writers == 0 {
                inner.phase = WriterAuthorityPhase::FailedClosed;
            } else {
                inner.active_legacy_writers -= 1;
            }
        }
        self.active = false;
        self.shared.legacy_drained.notify_all();
    }
}

impl WriterAuthority {
    pub(crate) fn new(root: WorkspaceRootIdentity) -> Self {
        Self {
            shared: Arc::new(WriterAuthorityShared {
                root,
                inner: Mutex::new(WriterAuthorityInner {
                    phase: WriterAuthorityPhase::Legacy,
                    active_legacy_writers: 0,
                    next_claim_nonce: 0,
                }),
                legacy_drained: Condvar::new(),
            }),
        }
    }

    pub(crate) fn snapshot(&self) -> WriterAuthoritySnapshot {
        let inner = match self.shared.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => self.shared.mark_failed_closed(poisoned.into_inner()),
        };
        WriterAuthoritySnapshot {
            state: inner.phase.public_state(),
            active_legacy_writers: inner.active_legacy_writers,
        }
    }

    pub(crate) fn matches_root(&self, root: &WorkspaceRootIdentity) -> bool {
        self.shared.root.same_current_root(root)
    }

    fn validate_root(&self, root: &WorkspaceRootIdentity) -> Result<(), WriterAuthorityError> {
        if !self.shared.root.is_current() {
            self.shared.fail_closed();
            return Err(WriterAuthorityError::WorkspaceRootUnavailable);
        }
        if !root.is_current() {
            return Err(WriterAuthorityError::WorkspaceRootUnavailable);
        }
        if self.shared.root.0.canonical_path != root.0.canonical_path
            || self.shared.root.0.identity != root.0.identity
        {
            return Err(WriterAuthorityError::WorkspaceRootMismatch);
        }
        Ok(())
    }

    pub(crate) fn acquire_legacy_writer(
        &self,
        root: &WorkspaceRootIdentity,
    ) -> Result<LegacyWriterLease, WriterAuthorityError> {
        self.validate_root(root)?;
        let mut inner = self.shared.lock_inner()?;
        if inner.phase != WriterAuthorityPhase::Legacy {
            return Err(WriterAuthorityError::LegacyWriterRejected);
        }
        let Some(active_legacy_writers) = inner.active_legacy_writers.checked_add(1) else {
            inner.phase = WriterAuthorityPhase::FailedClosed;
            self.shared.legacy_drained.notify_all();
            return Err(WriterAuthorityError::InvalidTransition);
        };
        inner.active_legacy_writers = active_legacy_writers;
        Ok(LegacyWriterLease {
            shared: Arc::clone(&self.shared),
            active: true,
        })
    }

    pub(crate) fn begin_kernel_transition(
        &self,
        root: &WorkspaceRootIdentity,
        target: KernelGeneration,
    ) -> Result<(), WriterAuthorityError> {
        self.validate_root(root)?;
        let mut inner = self.shared.lock_inner()?;
        match inner.phase {
            WriterAuthorityPhase::Legacy => {}
            WriterAuthorityPhase::FailedClosed => return Err(WriterAuthorityError::FailedClosed),
            _ => {
                inner.phase = WriterAuthorityPhase::FailedClosed;
                self.shared.legacy_drained.notify_all();
                return Err(WriterAuthorityError::InvalidTransition);
            }
        }
        inner.phase = WriterAuthorityPhase::Transitioning {
            target,
            claim_nonce: None,
        };
        Ok(())
    }

    pub(crate) fn replace_failed_kernel_transition(
        &self,
        root: &WorkspaceRootIdentity,
        failed: KernelGeneration,
        replacement: KernelGeneration,
    ) -> Result<(), WriterAuthorityError> {
        self.validate_root(root)?;
        if !replacement.is_after(failed) {
            self.fail_closed();
            return Err(WriterAuthorityError::NonMonotonicGeneration);
        }
        let mut inner = self.shared.lock_inner()?;
        match inner.phase {
            WriterAuthorityPhase::Transitioning {
                target,
                claim_nonce: None,
            } if target == failed => {
                inner.phase = WriterAuthorityPhase::Transitioning {
                    target: replacement,
                    claim_nonce: None,
                };
                Ok(())
            }
            WriterAuthorityPhase::FailedClosed => Err(WriterAuthorityError::FailedClosed),
            WriterAuthorityPhase::Transitioning { .. } => {
                inner.phase = WriterAuthorityPhase::FailedClosed;
                self.shared.legacy_drained.notify_all();
                Err(WriterAuthorityError::KernelGenerationMismatch)
            }
            _ => {
                inner.phase = WriterAuthorityPhase::FailedClosed;
                self.shared.legacy_drained.notify_all();
                Err(WriterAuthorityError::InvalidTransition)
            }
        }
    }

    pub(crate) fn fail_closed(&self) {
        self.shared.fail_closed();
    }

    fn reserve_kernel_claim(
        &self,
        inner: &mut WriterAuthorityInner,
        expected: KernelGeneration,
    ) -> Result<KernelWriterClaim, WriterAuthorityError> {
        let (target, claim_nonce) = match inner.phase {
            WriterAuthorityPhase::Transitioning {
                target,
                claim_nonce,
            } => (target, claim_nonce),
            WriterAuthorityPhase::FailedClosed => return Err(WriterAuthorityError::FailedClosed),
            _ => {
                inner.phase = WriterAuthorityPhase::FailedClosed;
                self.shared.legacy_drained.notify_all();
                return Err(WriterAuthorityError::InvalidTransition);
            }
        };
        if target != expected {
            inner.phase = WriterAuthorityPhase::FailedClosed;
            self.shared.legacy_drained.notify_all();
            return Err(WriterAuthorityError::KernelGenerationMismatch);
        }
        if claim_nonce.is_some() {
            return Err(WriterAuthorityError::KernelClaimOutstanding);
        }
        if inner.active_legacy_writers != 0 {
            return Err(WriterAuthorityError::LegacyWritersActive(
                inner.active_legacy_writers,
            ));
        }
        let Some(nonce) = inner.next_claim_nonce.checked_add(1) else {
            inner.phase = WriterAuthorityPhase::FailedClosed;
            self.shared.legacy_drained.notify_all();
            return Err(WriterAuthorityError::InvalidTransition);
        };
        inner.next_claim_nonce = nonce;
        inner.phase = WriterAuthorityPhase::Transitioning {
            target,
            claim_nonce: Some(nonce),
        };
        Ok(KernelWriterClaim {
            shared: Arc::clone(&self.shared),
            generation: target,
            nonce,
            active: true,
        })
    }

    pub(crate) fn try_claim_kernel(
        &self,
        expected: KernelGeneration,
    ) -> Result<KernelWriterClaim, WriterAuthorityError> {
        self.shared.ensure_root_current()?;
        let mut inner = self.shared.lock_inner()?;
        self.reserve_kernel_claim(&mut inner, expected)
    }

    pub(crate) fn claim_kernel_after_legacy_drain(
        &self,
        expected: KernelGeneration,
    ) -> Result<KernelWriterClaim, WriterAuthorityError> {
        self.shared.ensure_root_current()?;
        let mut inner = self.shared.lock_inner()?;
        loop {
            if !self.shared.root.is_current() {
                inner.phase = WriterAuthorityPhase::FailedClosed;
                self.shared.legacy_drained.notify_all();
                return Err(WriterAuthorityError::WorkspaceRootUnavailable);
            }
            match inner.phase {
                WriterAuthorityPhase::Transitioning {
                    target,
                    claim_nonce: None,
                } if target == expected => {}
                WriterAuthorityPhase::Transitioning { target, .. } if target != expected => {
                    inner.phase = WriterAuthorityPhase::FailedClosed;
                    self.shared.legacy_drained.notify_all();
                    return Err(WriterAuthorityError::KernelGenerationMismatch);
                }
                WriterAuthorityPhase::Transitioning {
                    claim_nonce: Some(_),
                    ..
                } => return Err(WriterAuthorityError::KernelClaimOutstanding),
                WriterAuthorityPhase::FailedClosed => {
                    return Err(WriterAuthorityError::FailedClosed)
                }
                _ => {
                    inner.phase = WriterAuthorityPhase::FailedClosed;
                    self.shared.legacy_drained.notify_all();
                    return Err(WriterAuthorityError::InvalidTransition);
                }
            }
            if inner.active_legacy_writers == 0 {
                return self.reserve_kernel_claim(&mut inner, expected);
            }
            inner = match self.shared.legacy_drained.wait(inner) {
                Ok(inner) => inner,
                Err(poisoned) => return Err(self.shared.latch_poisoned(poisoned.into_inner())),
            };
        }
    }
}

#[derive(Clone)]
pub(crate) struct KernelWriterPublicationGate {
    authority: WriterAuthority,
    root: WorkspaceRootIdentity,
    lifecycle: Arc<Mutex<KernelWriterPublicationLifecycle>>,
}

enum KernelWriterPublicationLifecycle {
    Legacy,
    Transitioning(KernelGeneration),
    Published(KernelWriterLease),
    FailedClosed,
}

impl KernelWriterPublicationGate {
    pub(crate) fn new(
        authority: WriterAuthority,
        root: WorkspaceRootIdentity,
    ) -> Result<Self, WriterAuthorityError> {
        if !authority.matches_root(&root) {
            authority.fail_closed();
            return Err(WriterAuthorityError::WorkspaceRootMismatch);
        }
        Ok(Self {
            authority,
            root,
            lifecycle: Arc::new(Mutex::new(KernelWriterPublicationLifecycle::Legacy)),
        })
    }

    pub(crate) fn begin_initial(&self, value: u64) -> Result<(), WriterAuthorityError> {
        let generation = self.generation_or_fail_closed(value)?;
        let mut lifecycle = self.lock_lifecycle()?;
        if !matches!(*lifecycle, KernelWriterPublicationLifecycle::Legacy) {
            return Err(self.fail_invalid_transition(&mut lifecycle));
        }
        if let Err(error) = self
            .authority
            .begin_kernel_transition(&self.root, generation)
        {
            *lifecycle = KernelWriterPublicationLifecycle::FailedClosed;
            self.authority.fail_closed();
            return Err(error);
        }
        *lifecycle = KernelWriterPublicationLifecycle::Transitioning(generation);
        Ok(())
    }

    pub(crate) fn advance_recovery(&self, value: u64) -> Result<(), WriterAuthorityError> {
        let replacement = self.generation_or_fail_closed(value)?;
        let mut lifecycle = self.lock_lifecycle()?;
        let previous = std::mem::replace(
            &mut *lifecycle,
            KernelWriterPublicationLifecycle::FailedClosed,
        );
        let result = match previous {
            KernelWriterPublicationLifecycle::Published(lease) => lease.begin_recovery(replacement),
            KernelWriterPublicationLifecycle::Transitioning(failed) => self
                .authority
                .replace_failed_kernel_transition(&self.root, failed, replacement),
            KernelWriterPublicationLifecycle::Legacy
            | KernelWriterPublicationLifecycle::FailedClosed => {
                self.authority.fail_closed();
                Err(WriterAuthorityError::InvalidTransition)
            }
        };
        match result {
            Ok(()) => {
                *lifecycle = KernelWriterPublicationLifecycle::Transitioning(replacement);
                Ok(())
            }
            Err(error) => {
                self.authority.fail_closed();
                Err(error)
            }
        }
    }

    pub(crate) fn try_publish(&self, value: u64) -> Result<bool, WriterAuthorityError> {
        let expected = self.generation_or_fail_closed(value)?;
        let mut lifecycle = self.lock_lifecycle()?;
        if !matches!(
            *lifecycle,
            KernelWriterPublicationLifecycle::Transitioning(target) if target == expected
        ) {
            return Err(self.fail_invalid_transition(&mut lifecycle));
        }
        let claim = match self.authority.try_claim_kernel(expected) {
            Ok(claim) => claim,
            Err(WriterAuthorityError::LegacyWritersActive(_)) => return Ok(false),
            Err(error) => {
                *lifecycle = KernelWriterPublicationLifecycle::FailedClosed;
                self.authority.fail_closed();
                return Err(error);
            }
        };
        match claim.publish() {
            Ok(lease) => {
                *lifecycle = KernelWriterPublicationLifecycle::Published(lease);
                Ok(true)
            }
            Err(error) => {
                *lifecycle = KernelWriterPublicationLifecycle::FailedClosed;
                self.authority.fail_closed();
                Err(error)
            }
        }
    }

    pub(crate) fn fail_closed(&self) {
        self.authority.fail_closed();
        match self.lifecycle.lock() {
            Ok(mut lifecycle) => {
                *lifecycle = KernelWriterPublicationLifecycle::FailedClosed;
            }
            Err(poisoned) => {
                *poisoned.into_inner() = KernelWriterPublicationLifecycle::FailedClosed;
            }
        }
    }

    fn lock_lifecycle(
        &self,
    ) -> Result<MutexGuard<'_, KernelWriterPublicationLifecycle>, WriterAuthorityError> {
        self.lifecycle.lock().map_err(|poisoned| {
            self.authority.fail_closed();
            *poisoned.into_inner() = KernelWriterPublicationLifecycle::FailedClosed;
            WriterAuthorityError::Poisoned
        })
    }

    fn generation_or_fail_closed(
        &self,
        value: u64,
    ) -> Result<KernelGeneration, WriterAuthorityError> {
        KernelGeneration::new(value).inspect_err(|_| self.fail_closed())
    }

    fn fail_invalid_transition(
        &self,
        lifecycle: &mut KernelWriterPublicationLifecycle,
    ) -> WriterAuthorityError {
        *lifecycle = KernelWriterPublicationLifecycle::FailedClosed;
        self.authority.fail_closed();
        WriterAuthorityError::InvalidTransition
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WriterSurfaceDisposition {
    RequiresWorkspaceFence,
    HostOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WriterSurfaceIntegration {
    Unwired,
    Guarded,
    Independent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LegacyWriterSurface {
    pub(crate) name: &'static str,
    pub(crate) disposition: WriterSurfaceDisposition,
    pub(crate) integration: WriterSurfaceIntegration,
    pub(crate) entry_points: &'static [&'static str],
}

const LEGACY_WRITER_SURFACES: &[LegacyWriterSurface] = &[
    LegacyWriterSurface {
        name: "desktop-workspace-authority",
        disposition: WriterSurfaceDisposition::RequiresWorkspaceFence,
        integration: WriterSurfaceIntegration::Unwired,
        entry_points: &[
            "acknowledge_path_guard",
            "discard_prepared_desktop_notebook_target",
            "prepare_desktop_notebook_target",
            "write_primary_workspace_state",
        ],
    },
    LegacyWriterSurface {
        name: "desktop-document-resource-writers",
        disposition: WriterSurfaceDisposition::RequiresWorkspaceFence,
        integration: WriterSurfaceIntegration::Unwired,
        entry_points: &[
            "create_markdown_tree_file",
            "create_markdown_tree_folder",
            "delete_markdown_template_file",
            "delete_markdown_tree_file",
            "export_pandoc_file",
            "export_pdf_file",
            "import_local_file",
            "move_markdown_tree_file",
            "rename_markdown_tree_file",
            "save_clipboard_attachment",
            "save_clipboard_image",
            "trash_markdown_assets",
            "trash_workspace_resources",
            "write_markdown_export_file",
            "write_markdown_file",
            "write_markdown_template_file",
            "write_standalone_document_cas",
        ],
    },
    LegacyWriterSurface {
        name: "desktop-settings-and-sync-domain-writers",
        disposition: WriterSurfaceDisposition::RequiresWorkspaceFence,
        integration: WriterSurfaceIntegration::Unwired,
        entry_points: &[
            "enable_sync_config",
            "patch_sync_config",
            "recover_sync_config",
            "reset_sync_config",
        ],
    },
    LegacyWriterSurface {
        name: "desktop-dejavu-execution",
        disposition: WriterSurfaceDisposition::RequiresWorkspaceFence,
        integration: WriterSurfaceIntegration::Unwired,
        entry_points: &[
            "bind_dejavu_repository",
            "change_global_key",
            "delete_remote_repository",
            "initialize_dejavu_global_key",
            "purge_remote_repository",
            "rebuild_local_repository",
            "stop_repository_sync",
        ],
    },
    LegacyWriterSurface {
        name: "mcp-kernel-adapter-writers",
        disposition: WriterSurfaceDisposition::RequiresWorkspaceFence,
        integration: WriterSurfaceIntegration::Guarded,
        entry_points: &[
            "mcp_document_create",
            "mcp_document_delete",
            "mcp_document_move",
            "mcp_document_update",
            "mcp_settings_update",
            "mcp_sync_after_write",
            "mcp_sync_run",
            "mcp_sync_update_config",
            "mcp_sync_update_credentials",
        ],
    },
    LegacyWriterSurface {
        name: "background-writer-triggers",
        disposition: WriterSurfaceDisposition::RequiresWorkspaceFence,
        integration: WriterSurfaceIntegration::Unwired,
        entry_points: &[
            "DejavuSchedulerOwner::record_file_change",
            "DejavuSchedulerOwner::trigger_startup",
            "handle_native_sync_exit",
            "install_production_graph",
            "unwatch_markdown_file",
            "unwatch_markdown_tree",
            "watch_markdown_file",
            "watch_markdown_tree",
        ],
    },
    LegacyWriterSurface {
        name: "desktop-host-ui-coordination",
        disposition: WriterSurfaceDisposition::HostOnly,
        integration: WriterSurfaceIntegration::Independent,
        entry_points: &[
            "cancel_sync_config_apply",
            "initialize_desktop_kernel_workspace",
            "request_sync_config_apply",
            "retry_desktop_kernel_workspace",
            "switch_desktop_kernel_workspace",
            "set_sync_config_editing",
            "settle_kernel_sync_config_apply",
        ],
    },
    LegacyWriterSurface {
        name: "desktop-host-only-writers",
        disposition: WriterSurfaceDisposition::HostOnly,
        integration: WriterSurfaceIntegration::Independent,
        entry_points: &[
            "cancel_theme_activation",
            "clear_mcp_audit_entries",
            "commit_theme_activation",
            "delete_theme",
            "export_markdown_file",
            "import_theme_file",
            "install_shell_command",
            "mcp::initialize",
            "prepare_theme_activation",
            "release_theme_activation",
            "release_theme_activation_for_window",
            "replace_theme_file",
            "set_editor_window_restore_state",
            "update_mcp_settings",
            "uninstall_shell_command",
            "save_settings_file",
        ],
    },
];

pub(crate) fn legacy_writer_surface_inventory() -> &'static [LegacyWriterSurface] {
    LEGACY_WRITER_SURFACES
}

const NORMAL_DESKTOP_HOST_READS_AND_ACTIONS: &[&str] = &[
    "canonical_local_file_path",
    "check_pandoc_available",
    "detect_pandoc_path",
    "destroy_current_editor_window",
    "download_web_image",
    "get_mcp_health",
    "get_mcp_settings",
    "get_shell_command_status",
    "hide_settings_window",
    "initialize_desktop_kernel_workspace",
    "install_application_menu",
    "list_editor_window_restore_states",
    "list_mcp_audit_entries",
    "list_system_font_families",
    "list_themes",
    "load_sync_config_editing",
    "mark_settings_window_ready",
    "minimize_current_window",
    "open_blank_editor_window",
    "open_containing_folder",
    "open_primary_workspace_containing_folder",
    "open_log_folder",
    "open_markdown_attachment",
    "open_markdown_file_in_new_window",
    "open_markdown_folder_in_new_window",
    "open_settings_window",
    "read_clipboard_content",
    "read_clipboard_text",
    "read_desktop_kernel_startup_state",
    "read_local_image_file",
    "read_native_kernel_bootstrap",
    "read_primary_workspace_state",
    "read_standalone_document",
    "read_text_file",
    "read_theme_css",
    "request_primary_notebook_switch",
    "retry_desktop_kernel_workspace",
    "switch_desktop_kernel_workspace",
    "show_native_app_about",
    "take_opened_markdown_paths",
    "theme_directory_path",
];

pub(crate) fn normal_desktop_command_is_allowed(command: &str) -> bool {
    if let Some(surface) = LEGACY_WRITER_SURFACES
        .iter()
        .find(|surface| surface.entry_points.contains(&command))
    {
        return surface.disposition == WriterSurfaceDisposition::HostOnly;
    }
    NORMAL_DESKTOP_HOST_READS_AND_ACTIONS.contains(&command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        io::Write as _,
        sync::{mpsc, Barrier, OnceLock},
        thread,
        time::{Duration, Instant},
    };

    macro_rules! assert_not_impl_any {
        ($type:ty: $($trait:path),+ $(,)?) => {
            const _: fn() = || {
                trait AmbiguousIfImpl<Marker> {
                    fn marker() {}
                }
                impl<Value: ?Sized> AmbiguousIfImpl<()> for Value {}
                $({
                    struct EscapeTrait;
                    impl<Value: ?Sized + $trait> AmbiguousIfImpl<EscapeTrait> for Value {}
                })+
                let _ = <$type as AmbiguousIfImpl<_>>::marker;
            };
        };
    }

    assert_not_impl_any!(
        WorkspaceMutationRoot<'static>:
            Clone,
            std::ops::Deref,
            std::convert::AsRef<Dir>,
            std::borrow::Borrow<Dir>
    );
    assert_not_impl_any!(
        WorkspaceMutationFile<'static>:
            Clone,
            std::ops::Deref,
            std::convert::AsRef<File>,
            std::borrow::Borrow<File>,
            std::convert::Into<File>
    );

    fn root_identity() -> WorkspaceRootIdentity {
        let root = tempfile::tempdir().expect("workspace root should be created");
        let identity = WorkspaceRootIdentity::open(root.path())
            .expect("workspace capability identity should be valid");
        static RETAINED_ROOTS: OnceLock<Mutex<Vec<tempfile::TempDir>>> = OnceLock::new();
        RETAINED_ROOTS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .expect("test root registry should remain healthy")
            .push(root);
        identity
    }

    fn generation(value: u64) -> KernelGeneration {
        KernelGeneration::new(value).expect("test generation should be non-zero")
    }

    #[test]
    fn mutation_facade_static_api_has_no_raw_capability_escape_hatch() {
        // A unit test cannot retain source that is intentionally rejected by the compiler. The
        // TDD probes exercised both old escapes before this audit pinned the safe public surface.
        let source = include_str!("writer_authority.rs");
        let root_start = source
            .find("impl<'scope> WorkspaceMutationRoot<'scope>")
            .expect("root facade impl should remain explicit");
        let file_start = source
            .find("pub(crate) struct WorkspaceMutationFile<'scope>")
            .expect("file facade should remain explicit");
        let file_impl_start = source
            .find("impl WorkspaceMutationFile<'_>")
            .expect("file facade impl should remain explicit");
        let write_impl_start = source
            .find("impl io::Write for WorkspaceMutationFile<'_>")
            .expect("file facade should expose only the narrow Write contract");
        let root_impl = &source[root_start..file_start];
        let file_impl = &source[file_impl_start..write_impl_start];
        let root_public_methods = root_impl
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("pub(crate) fn "))
            .collect::<Vec<_>>();
        let file_public_methods = file_impl
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("pub(crate) fn "))
            .collect::<Vec<_>>();

        assert_eq!(root_public_methods, ["pub(crate) fn create("]);
        assert_eq!(
            file_public_methods,
            [
                "pub(crate) fn sync_all(&self) -> io::Result<()> {",
                "pub(crate) fn sync_data(&self) -> io::Result<()> {",
            ]
        );
        let higher_ranked_operation = ["operation: impl ", "for<'scope> FnOnce("].concat();
        assert_eq!(
            source.matches(&higher_ranked_operation).count(),
            3,
            "shared, Legacy, and Kernel operations must all preserve the HRTB brand"
        );
        let raw_directory_operation = ["operation: impl FnOnce(", "&Dir)"].concat();
        assert!(!source.contains(&raw_directory_operation));
        let invariant_scope = ["scope: PhantomData<", "&'scope mut &'scope ()>"].concat();
        assert_eq!(
            source.matches(&invariant_scope).count(),
            2,
            "root and file facades must remain invariant over the operation scope"
        );
    }

    #[test]
    fn writer_authority_starts_in_legacy_for_one_opaque_workspace_root() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());

        assert_eq!(authority.snapshot().state, WriterAuthorityState::Legacy);
        assert_eq!(authority.snapshot().active_legacy_writers, 0);
        assert!(authority.matches_root(&root));
    }

    #[test]
    fn publication_gate_invalid_generation_latches_failed_closed() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let gate = KernelWriterPublicationGate::new(authority.clone(), root)
            .expect("matching authority and root should form a publication gate");

        assert_eq!(
            gate.begin_initial(0).unwrap_err(),
            WriterAuthorityError::InvalidGeneration
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn legacy_writer_leases_are_counted_and_released_by_drop() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());

        let first = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit a writer");
        let second = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit concurrent writers");
        assert_eq!(authority.snapshot().active_legacy_writers, 2);

        drop(first);
        assert_eq!(authority.snapshot().active_legacy_writers, 1);
        drop(second);
        assert_eq!(authority.snapshot().active_legacy_writers, 0);
    }

    #[test]
    fn legacy_writer_operation_uses_the_retained_root_capability() {
        let temporary = tempfile::tempdir().expect("workspace parent should be created");
        let root_path = temporary.path().join("notes");
        std::fs::create_dir(&root_path).expect("workspace root should be created");
        let root =
            WorkspaceRootIdentity::open(&root_path).expect("retained root identity should open");
        let authority = WriterAuthority::new(root.clone());
        let lease = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit the writer");

        lease
            .with_workspace_root(|retained_root| -> std::io::Result<()> {
                let mut file = retained_root.create("legacy-relative.md")?;
                file.write_all(b"retained legacy write")?;
                file.sync_data()?;
                file.sync_all()
            })
            .expect("the retained capability write should complete");

        assert_eq!(
            std::fs::read_to_string(root_path.join("legacy-relative.md"))
                .expect("relative write should exist under the workspace"),
            "retained legacy write"
        );
        assert_eq!(authority.snapshot().state, WriterAuthorityState::Legacy);
    }

    #[test]
    fn failed_closed_authority_rejects_a_legacy_operation_before_it_runs() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let lease = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit the writer");
        authority.fail_closed();
        let operation_ran = Cell::new(false);

        let result = lease.with_workspace_root(|_| {
            operation_ran.set(true);
            Ok::<_, std::convert::Infallible>(())
        });

        assert!(matches!(
            result,
            Err(WorkspaceWriterOperationError::Authority(
                WriterAuthorityError::FailedClosed
            ))
        ));
        assert!(!operation_ran.get());
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn panicking_legacy_operation_permanently_fails_closed() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let lease = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit the writer");

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = lease.with_workspace_root(|_| -> Result<(), std::convert::Infallible> {
                panic!("abort capability-relative operation");
            });
        }));

        assert!(panic.is_err());
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn legacy_lease_counter_overflow_fails_closed() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        authority
            .shared
            .inner
            .lock()
            .expect("authority should start healthy")
            .active_legacy_writers = usize::MAX;

        assert_eq!(
            authority.acquire_legacy_writer(&root).unwrap_err(),
            WriterAuthorityError::InvalidTransition
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn transition_fences_late_legacy_writers_but_retains_existing_leases_for_drain() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let admitted = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit the in-flight writer");

        authority
            .begin_kernel_transition(&root, generation(7))
            .expect("the Kernel transition should fence new writers");

        assert_eq!(
            authority.snapshot(),
            WriterAuthoritySnapshot {
                state: WriterAuthorityState::Transitioning,
                active_legacy_writers: 1,
            }
        );
        assert_eq!(
            authority.acquire_legacy_writer(&root).unwrap_err(),
            WriterAuthorityError::LegacyWriterRejected
        );
        drop(admitted);
        assert_eq!(authority.snapshot().active_legacy_writers, 0);
    }

    #[test]
    fn a_writer_for_a_different_root_is_rejected_without_changing_authority() {
        let root = root_identity();
        let other_root = root_identity();
        let authority = WriterAuthority::new(root);

        assert_eq!(
            authority.acquire_legacy_writer(&other_root).unwrap_err(),
            WriterAuthorityError::WorkspaceRootMismatch
        );
        assert_eq!(
            authority.snapshot(),
            WriterAuthoritySnapshot {
                state: WriterAuthorityState::Legacy,
                active_legacy_writers: 0,
            }
        );
    }

    #[test]
    fn kernel_claim_requires_the_exact_generation_and_a_drained_legacy_set() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let legacy = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit the in-flight writer");
        let target = generation(11);
        authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");

        assert_eq!(
            authority.try_claim_kernel(target).unwrap_err(),
            WriterAuthorityError::LegacyWritersActive(1)
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::Transitioning
        );

        drop(legacy);
        let claim = authority
            .try_claim_kernel(target)
            .expect("drained transition should issue one claim");
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::Transitioning
        );

        let _kernel = claim
            .publish()
            .expect("the exact claimed generation should publish");
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::Kernel(target)
        );
        assert_eq!(
            authority.acquire_legacy_writer(&root).unwrap_err(),
            WriterAuthorityError::LegacyWriterRejected
        );
    }

    #[test]
    fn blocking_claim_waits_for_every_legacy_lease_to_drain() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let legacy = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit the in-flight writer");
        let target = generation(12);
        authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");
        let waiter_authority = authority.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (claim_tx, claim_rx) = mpsc::channel();
        let waiter = thread::spawn(move || {
            started_tx.send(()).expect("start should be observed");
            let claim = waiter_authority.claim_kernel_after_legacy_drain(target);
            claim_tx
                .send(claim)
                .expect("claim result should be observed");
        });
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("waiter should start");

        assert!(claim_rx.recv_timeout(Duration::from_millis(30)).is_err());
        let release_started = Instant::now();
        drop(legacy);
        let claim = claim_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the final lease drop should wake the waiter")
            .expect("the drained transition should issue a claim");
        assert!(release_started.elapsed() < Duration::from_secs(1));
        let _kernel = claim.publish().expect("claim should publish");
        waiter.join().expect("waiter should finish");
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::Kernel(target)
        );
    }

    #[test]
    fn mismatched_generation_claim_fails_closed_before_publication() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        authority
            .begin_kernel_transition(&root, generation(20))
            .expect("transition should begin");

        assert_eq!(
            authority.try_claim_kernel(generation(21)).unwrap_err(),
            WriterAuthorityError::KernelGenerationMismatch
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn dropping_an_unpublished_kernel_claim_fails_closed() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let target = generation(30);
        authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");

        let claim = authority
            .try_claim_kernel(target)
            .expect("drained transition should issue one claim");
        drop(claim);

        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
        assert_eq!(
            authority.acquire_legacy_writer(&root).unwrap_err(),
            WriterAuthorityError::LegacyWriterRejected
        );
    }

    #[test]
    fn a_failed_unclaimed_child_can_be_replaced_without_reopening_legacy() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let failed = generation(40);
        let replacement = generation(41);
        authority
            .begin_kernel_transition(&root, failed)
            .expect("transition should begin");

        authority
            .replace_failed_kernel_transition(&root, failed, replacement)
            .expect("an unclaimed failed child should be replaceable");

        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::Transitioning
        );
        assert_eq!(
            authority.acquire_legacy_writer(&root).unwrap_err(),
            WriterAuthorityError::LegacyWriterRejected
        );
        let _kernel = authority
            .try_claim_kernel(replacement)
            .expect("the replacement generation should claim")
            .publish()
            .expect("the replacement generation should publish");
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::Kernel(replacement)
        );
    }

    #[test]
    fn kernel_writer_operation_uses_the_retained_root_capability() {
        let temporary = tempfile::tempdir().expect("workspace parent should be created");
        let root_path = temporary.path().join("notes");
        std::fs::create_dir(&root_path).expect("workspace root should be created");
        let root =
            WorkspaceRootIdentity::open(&root_path).expect("retained root identity should open");
        let authority = WriterAuthority::new(root.clone());
        let generation = generation(49);
        authority
            .begin_kernel_transition(&root, generation)
            .expect("transition should begin");
        let kernel = authority
            .try_claim_kernel(generation)
            .expect("generation should claim")
            .publish()
            .expect("generation should publish");

        kernel
            .with_workspace_root(|retained_root| -> std::io::Result<()> {
                let mut file = retained_root.create("kernel-relative.md")?;
                file.write_all(b"retained Kernel write")?;
                file.sync_all()
            })
            .expect("the retained capability write should complete");

        assert_eq!(
            std::fs::read_to_string(root_path.join("kernel-relative.md"))
                .expect("relative write should exist under the workspace"),
            "retained Kernel write"
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::Kernel(generation)
        );
    }

    #[test]
    fn failed_closed_authority_rejects_a_kernel_operation_before_it_runs() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let generation = generation(50);
        authority
            .begin_kernel_transition(&root, generation)
            .expect("transition should begin");
        let kernel = authority
            .try_claim_kernel(generation)
            .expect("generation should claim")
            .publish()
            .expect("generation should publish");
        authority.fail_closed();
        let operation_ran = Cell::new(false);

        let result = kernel.with_workspace_root(|_| {
            operation_ran.set(true);
            Ok::<_, std::convert::Infallible>(())
        });

        assert!(matches!(
            result,
            Err(WorkspaceWriterOperationError::Authority(
                WriterAuthorityError::FailedClosed
            ))
        ));
        assert!(!operation_ran.get());
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn published_kernel_recovery_advances_generation_without_returning_to_legacy() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let first = generation(50);
        let second = generation(51);
        authority
            .begin_kernel_transition(&root, first)
            .expect("transition should begin");
        let first_kernel = authority
            .try_claim_kernel(first)
            .expect("first generation should claim")
            .publish()
            .expect("first generation should publish");
        assert_eq!(first_kernel.generation(), first);

        first_kernel
            .begin_recovery(second)
            .expect("the retained Kernel lease should begin recovery");

        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::Transitioning
        );
        assert_eq!(
            authority.acquire_legacy_writer(&root).unwrap_err(),
            WriterAuthorityError::LegacyWriterRejected
        );
        let _second_kernel = authority
            .try_claim_kernel(second)
            .expect("replacement should claim only after recovery begins")
            .publish()
            .expect("replacement should publish");
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::Kernel(second)
        );
    }

    #[test]
    fn losing_a_published_kernel_lease_fails_closed_instead_of_restoring_legacy() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let target = generation(60);
        authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");
        let kernel = authority
            .try_claim_kernel(target)
            .expect("generation should claim")
            .publish()
            .expect("generation should publish");

        drop(kernel);

        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
        assert_eq!(
            authority.begin_kernel_transition(&root, generation(61)),
            Err(WriterAuthorityError::FailedClosed)
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn invalid_or_non_monotonic_transitions_latch_failed_closed() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let target = generation(70);
        authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");

        assert_eq!(
            authority.begin_kernel_transition(&root, generation(71)),
            Err(WriterAuthorityError::InvalidTransition)
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );

        let recovery_authority = WriterAuthority::new(root.clone());
        recovery_authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");
        let kernel = recovery_authority
            .try_claim_kernel(target)
            .expect("generation should claim")
            .publish()
            .expect("generation should publish");
        assert_eq!(
            kernel.begin_recovery(target),
            Err(WriterAuthorityError::NonMonotonicGeneration)
        );
        assert_eq!(
            recovery_authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn explicit_failure_is_monotonic_and_wakes_a_drain_waiter() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let legacy = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit the in-flight writer");
        let target = generation(80);
        authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");
        let waiter_authority = authority.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let waiter = thread::spawn(move || {
            result_tx
                .send(waiter_authority.claim_kernel_after_legacy_drain(target))
                .expect("claim result should be observed");
        });

        authority.fail_closed();

        assert_eq!(
            result_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("failure should wake the waiter")
                .unwrap_err(),
            WriterAuthorityError::FailedClosed
        );
        drop(legacy);
        waiter.join().expect("waiter should finish");
        authority.fail_closed();
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn poisoned_authority_rejects_writers_and_latches_failed_closed() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let poisoned_shared = Arc::clone(&authority.shared);
        let poisoner = thread::spawn(move || {
            let _guard = poisoned_shared
                .inner
                .lock()
                .expect("mutex should start healthy");
            panic!("poison writer authority");
        });
        assert!(poisoner.join().is_err());

        assert_eq!(
            authority.acquire_legacy_writer(&root).unwrap_err(),
            WriterAuthorityError::Poisoned
        );
        let poisoned = match authority.shared.inner.lock() {
            Ok(_) => panic!("mutex should remain poisoned"),
            Err(poisoned) => poisoned,
        };
        assert_eq!(
            poisoned.into_inner().phase,
            WriterAuthorityPhase::FailedClosed
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn condvar_poison_wakes_a_drain_waiter_into_failed_closed() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());
        let legacy = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit the in-flight writer");
        let target = generation(90);
        authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");
        let waiter_authority = authority.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let waiter = thread::spawn(move || {
            started_tx.send(()).expect("waiter should report startup");
            result_tx
                .send(waiter_authority.claim_kernel_after_legacy_drain(target))
                .expect("claim result should be observed");
        });
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("waiter should start");
        thread::sleep(Duration::from_millis(30));
        let poisoned_shared = Arc::clone(&authority.shared);
        let poisoner = thread::spawn(move || {
            let _guard = poisoned_shared
                .inner
                .lock()
                .expect("waiter should release the mutex while blocked");
            panic!("poison drain mutex");
        });
        assert!(poisoner.join().is_err());
        authority.shared.legacy_drained.notify_all();

        assert_eq!(
            result_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("poison should wake the waiter")
                .unwrap_err(),
            WriterAuthorityError::Poisoned
        );
        let poisoned = match authority.shared.inner.lock() {
            Ok(_) => panic!("mutex should remain poisoned"),
            Err(poisoned) => poisoned,
        };
        assert_eq!(
            poisoned.into_inner().phase,
            WriterAuthorityPhase::FailedClosed
        );
        drop(legacy);
        waiter.join().expect("waiter should finish");
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn live_directory_identity_ignores_same_root_metadata_changes() {
        let temporary = tempfile::tempdir().expect("workspace parent should be created");
        let root_path = temporary.path().join("notes");
        std::fs::create_dir(&root_path).expect("workspace root should be created");
        let first = WorkspaceRootIdentity::open(&root_path)
            .expect("first retained root identity should open");
        let authority = WriterAuthority::new(first);
        std::fs::write(root_path.join("changed.md"), "# changed")
            .expect("workspace metadata should change");
        let same_root = WorkspaceRootIdentity::open(&root_path)
            .expect("same retained root identity should reopen");

        let _lease = authority
            .acquire_legacy_writer(&same_root)
            .expect("metadata changes must not change directory identity");
        assert!(authority.matches_root(&same_root));
    }

    #[cfg(unix)]
    #[test]
    fn addressed_root_replacement_fails_closed_even_with_the_old_identity_token() {
        let temporary = tempfile::tempdir().expect("workspace parent should be created");
        let root_path = temporary.path().join("notes");
        let retired_path = temporary.path().join("retired-notes");
        std::fs::create_dir(&root_path).expect("workspace root should be created");
        let root =
            WorkspaceRootIdentity::open(&root_path).expect("retained root identity should open");
        let authority = WriterAuthority::new(root.clone());
        std::fs::rename(&root_path, &retired_path).expect("old root should be displaced");
        std::fs::create_dir(&root_path).expect("replacement root should be created");

        assert_eq!(
            authority.acquire_legacy_writer(&root).unwrap_err(),
            WriterAuthorityError::WorkspaceRootUnavailable
        );
        assert!(!authority.matches_root(&root));
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[cfg(unix)]
    #[test]
    fn legacy_writer_operation_never_follows_a_replaced_addressed_root() {
        let temporary = tempfile::tempdir().expect("workspace parent should be created");
        let root_path = temporary.path().join("notes");
        let retired_path = temporary.path().join("retired-notes");
        std::fs::create_dir(&root_path).expect("workspace root should be created");
        let root =
            WorkspaceRootIdentity::open(&root_path).expect("retained root identity should open");
        let authority = WriterAuthority::new(root.clone());
        let lease = authority
            .acquire_legacy_writer(&root)
            .expect("Legacy should admit the writer");
        let operation_entered = Arc::new(Barrier::new(2));
        let replacement_published = Arc::new(Barrier::new(2));
        let replacer_entered = Arc::clone(&operation_entered);
        let replacer_published = Arc::clone(&replacement_published);
        let replacement_root = root_path.clone();
        let retired_root = retired_path.clone();
        let replacer = thread::spawn(move || {
            replacer_entered.wait();
            std::fs::rename(&replacement_root, &retired_root)
                .expect("old root should be displaced");
            std::fs::create_dir(&replacement_root).expect("replacement root should be created");
            replacer_published.wait();
        });

        let result = lease.with_workspace_root(|retained_root| -> std::io::Result<()> {
            operation_entered.wait();
            replacement_published.wait();
            let mut file = retained_root.create("retained-only.md")?;
            file.write_all(b"old retained root")
        });
        replacer.join().expect("root replacer should finish");

        assert!(matches!(
            result,
            Err(WorkspaceWriterOperationError::Authority(
                WriterAuthorityError::WorkspaceRootUnavailable
            ))
        ));
        assert_eq!(
            std::fs::read_to_string(retired_path.join("retained-only.md"))
                .expect("the in-flight write may finish only on the retained root"),
            "old retained root"
        );
        assert!(!root_path.join("retained-only.md").exists());
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[cfg(unix)]
    #[test]
    fn root_replacement_between_drain_and_claim_fails_closed() {
        let temporary = tempfile::tempdir().expect("workspace parent should be created");
        let root_path = temporary.path().join("notes");
        let retired_path = temporary.path().join("retired-notes");
        std::fs::create_dir(&root_path).expect("workspace root should be created");
        let root =
            WorkspaceRootIdentity::open(&root_path).expect("retained root identity should open");
        let authority = WriterAuthority::new(root.clone());
        let target = generation(100);
        authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");
        std::fs::rename(&root_path, &retired_path).expect("old root should be displaced");
        std::fs::create_dir(&root_path).expect("replacement root should be created");

        assert_eq!(
            authority.try_claim_kernel(target).unwrap_err(),
            WriterAuthorityError::WorkspaceRootUnavailable
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[cfg(unix)]
    #[test]
    fn root_replacement_between_claim_and_publication_fails_closed() {
        let temporary = tempfile::tempdir().expect("workspace parent should be created");
        let root_path = temporary.path().join("notes");
        let retired_path = temporary.path().join("retired-notes");
        std::fs::create_dir(&root_path).expect("workspace root should be created");
        let root =
            WorkspaceRootIdentity::open(&root_path).expect("retained root identity should open");
        let authority = WriterAuthority::new(root.clone());
        let target = generation(101);
        authority
            .begin_kernel_transition(&root, target)
            .expect("transition should begin");
        let claim = authority
            .try_claim_kernel(target)
            .expect("drained transition should issue a claim");
        std::fs::rename(&root_path, &retired_path).expect("old root should be displaced");
        std::fs::create_dir(&root_path).expect("replacement root should be created");

        assert_eq!(
            claim.publish().unwrap_err(),
            WriterAuthorityError::WorkspaceRootUnavailable
        );
        assert_eq!(
            authority.snapshot().state,
            WriterAuthorityState::FailedClosed
        );
    }

    #[test]
    fn phase_two_inventory_marks_only_kernel_backed_mcp_writers_guarded() {
        let expected_groups = [
            "desktop-workspace-authority",
            "desktop-document-resource-writers",
            "desktop-settings-and-sync-domain-writers",
            "desktop-dejavu-execution",
            "mcp-kernel-adapter-writers",
            "background-writer-triggers",
        ];
        let inventory = legacy_writer_surface_inventory();
        let actual_groups = inventory
            .iter()
            .filter(|entry| entry.disposition == WriterSurfaceDisposition::RequiresWorkspaceFence)
            .map(|entry| entry.name)
            .collect::<Vec<_>>();

        assert_eq!(actual_groups, expected_groups);
        for entry in inventory
            .iter()
            .filter(|entry| entry.disposition == WriterSurfaceDisposition::RequiresWorkspaceFence)
        {
            assert!(!entry.entry_points.is_empty());
            if entry.name == "mcp-kernel-adapter-writers" {
                assert_eq!(entry.integration, WriterSurfaceIntegration::Guarded);
            } else {
                assert_eq!(entry.integration, WriterSurfaceIntegration::Unwired);
            }
        }
    }

    #[test]
    fn writer_surface_inventory_has_no_ambiguous_or_duplicate_entry_points() {
        let inventory = legacy_writer_surface_inventory();
        let mut seen = std::collections::BTreeSet::new();
        for entry in inventory {
            assert!(!entry.name.is_empty());
            for entry_point in entry.entry_points {
                assert!(
                    seen.insert(*entry_point),
                    "{entry_point} appears in more than one writer-authority inventory group"
                );
            }
        }
        assert!(
            inventory.iter().any(|entry| {
                entry.disposition == WriterSurfaceDisposition::HostOnly
                    && entry.integration == WriterSurfaceIntegration::Independent
            }),
            "host-only writes must stay separate from workspace ownership claims"
        );
    }

    #[test]
    fn normal_desktop_allows_native_settings_exports_as_host_only_writes() {
        let surface = legacy_writer_surface_inventory()
            .iter()
            .find(|surface| surface.entry_points.contains(&"save_settings_file"))
            .expect("settings export writer surface");

        assert_eq!(surface.disposition, WriterSurfaceDisposition::HostOnly);
        assert_eq!(surface.integration, WriterSurfaceIntegration::Independent);
        assert!(normal_desktop_command_is_allowed("save_settings_file"));
    }

    #[test]
    fn normal_desktop_does_not_expose_generic_text_writes() {
        assert!(!normal_desktop_command_is_allowed("write_text_file"));
    }

    #[test]
    fn normal_desktop_rejects_every_workspace_fenced_legacy_entry_point() {
        for surface in legacy_writer_surface_inventory() {
            for entry_point in surface.entry_points {
                assert_eq!(
                    normal_desktop_command_is_allowed(entry_point),
                    surface.disposition == WriterSurfaceDisposition::HostOnly,
                    "normal desktop classification drifted for {entry_point}"
                );
            }
        }
        assert!(normal_desktop_command_is_allowed(
            "read_native_kernel_bootstrap"
        ));
        assert!(normal_desktop_command_is_allowed(
            "request_primary_notebook_switch"
        ));
        assert!(!normal_desktop_command_is_allowed(
            "read_markdown_template_file"
        ));
        assert!(!normal_desktop_command_is_allowed(
            "future_unclassified_workspace_writer"
        ));
    }
}
