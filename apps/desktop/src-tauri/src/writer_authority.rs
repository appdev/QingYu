//! Desktop-local writer ownership fence for the future atomic Kernel cutover.

#![cfg_attr(not(test), allow(dead_code))]

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard},
};

use cap_fs_ext::DirExt as _;
use cap_std::fs::Dir;

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
}

#[derive(Clone)]
pub(crate) struct WriterAuthority {
    shared: Arc<WriterAuthorityShared>,
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
            "write_text_file",
        ],
    },
    LegacyWriterSurface {
        name: "desktop-settings-and-sync-control",
        disposition: WriterSurfaceDisposition::RequiresWorkspaceFence,
        integration: WriterSurfaceIntegration::Unwired,
        entry_points: &[
            "cancel_sync_config_apply",
            "commit_desktop_runtime_store_changes",
            "enable_sync_config",
            "patch_exposed_app_settings",
            "patch_sync_config",
            "recover_sync_config",
            "replace_portable_app_settings",
            "request_sync_config_apply",
            "reset_sync_config",
            "set_sync_config_editing",
            "write_app_settings_group",
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
            "sync_application",
        ],
    },
    LegacyWriterSurface {
        name: "mcp-direct-writers",
        disposition: WriterSurfaceDisposition::RequiresWorkspaceFence,
        integration: WriterSurfaceIntegration::Unwired,
        entry_points: &[
            "clear_mcp_audit_entries",
            "mcp::initialize",
            "mcp_document_create",
            "mcp_document_delete",
            "mcp_document_move",
            "mcp_document_update",
            "mcp_settings_update",
            "mcp_sync_run",
            "mcp_sync_update_config",
            "mcp_sync_update_credentials",
            "set_mcp_primary_workspace",
            "update_mcp_settings",
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
            "mcp_sync_after_write",
            "unwatch_markdown_file",
            "unwatch_markdown_tree",
            "watch_markdown_file",
            "watch_markdown_tree",
        ],
    },
    LegacyWriterSurface {
        name: "desktop-host-only-writers",
        disposition: WriterSurfaceDisposition::HostOnly,
        integration: WriterSurfaceIntegration::Independent,
        entry_points: &[
            "cancel_theme_activation",
            "commit_theme_activation",
            "delete_theme",
            "import_theme_file",
            "install_shell_command",
            "prepare_theme_activation",
            "release_theme_activation",
            "release_theme_activation_for_window",
            "replace_theme_file",
            "set_editor_window_restore_state",
            "uninstall_shell_command",
        ],
    },
];

pub(crate) fn legacy_writer_surface_inventory() -> &'static [LegacyWriterSurface] {
    LEGACY_WRITER_SURFACES
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{mpsc, OnceLock},
        thread,
        time::{Duration, Instant},
    };

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
    fn writer_authority_starts_in_legacy_for_one_opaque_workspace_root() {
        let root = root_identity();
        let authority = WriterAuthority::new(root.clone());

        assert_eq!(authority.snapshot().state, WriterAuthorityState::Legacy);
        assert_eq!(authority.snapshot().active_legacy_writers, 0);
        assert!(authority.matches_root(&root));
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
    fn phase_one_inventory_keeps_every_overlapping_writer_explicitly_unwired() {
        let expected_groups = [
            "desktop-workspace-authority",
            "desktop-document-resource-writers",
            "desktop-settings-and-sync-control",
            "desktop-dejavu-execution",
            "mcp-direct-writers",
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
            assert_eq!(entry.integration, WriterSurfaceIntegration::Unwired);
            assert!(!entry.entry_points.is_empty());
            assert_ne!(entry.integration, WriterSurfaceIntegration::Guarded);
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
}
