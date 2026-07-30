//! Compatibility import for the Kernel-owned workspace control-path policy.

pub(crate) use qingyu_kernel::protected_paths::{
    is_protected_sync_relative_path, is_qingyu_control_directory_name,
    path_contains_qingyu_control_directory, LEGACY_SYNC_DIR, QINGYU_CONTROL_DIR,
    SYNC_MUTATION_STAGING_PREFIX,
};
