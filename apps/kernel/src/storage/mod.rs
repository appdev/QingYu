mod atomic_file;
mod capability;
mod no_replace;

#[cfg(test)]
pub(crate) use atomic_file::DurableFileTestFault;

pub use atomic_file::{
    CommitState, DurableFileFailure, DurableFileFailureKind, DurableFileStore, ExpectedFile,
    FileRevision, PreservePrevious, RecoveryOutcome, RecoveryTransactionId, ReplaceOutcome,
    ReplaceRequest, StorageFileName, StoredFile,
};
pub use capability::{
    ambient_symlink_metadata, create_private_file_options, create_private_replaceable_file_options,
    directory_identity, nonfollowing_read_options, open_canonical_directory_nofollow,
    rename_in_directory, rename_retained_file_in_directory, sync_directory,
    sync_directory_commit_state, unique_regular_file_identity, DirectoryIdentity,
    UniqueRegularFileIdentity,
};
pub use no_replace::rename_noreplace;
