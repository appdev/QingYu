//! Compatibility import for the Kernel-owned remote synchronization boundary.

pub(crate) use qingyu_kernel::sync::backend::{
    notebook_name_available_on_current_platform, sync_state_key, RemoteSyncBackend,
    RemoteSyncDiagnostic, RemoteSyncError, RemoteSyncFile, SyncFailureCategory,
    SyncProviderOperation, ValidRemoteRoot,
};
