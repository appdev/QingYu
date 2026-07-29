mod atomic_file;

#[cfg(test)]
pub(crate) use atomic_file::DurableFileTestFault;

pub use atomic_file::{
    CommitState, DurableFileFailure, DurableFileFailureKind, DurableFileStore, ExpectedFile,
    FileRevision, PreservePrevious, RecoveryOutcome, RecoveryTransactionId, ReplaceOutcome,
    ReplaceRequest, StorageFileName, StoredFile,
};
