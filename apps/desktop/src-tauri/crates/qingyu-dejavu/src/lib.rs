pub mod atomic_write;
pub mod chunker;
pub mod cloud;
pub mod crypto;
pub mod diff;
pub mod entity;
pub mod history;
mod indexer;
mod lifecycle;
mod path_security;
pub mod purge;
pub mod ref_store;
pub mod repo;
pub mod store;
pub mod sync;
pub mod sync_lock;
pub mod working_tree;

mod error;

pub use atomic_write::write_file_safer;
pub use chunker::{ChunkBoundary, RabinChunker};
pub use cloud::{Cloud, CloudError, CloudObject, CloudOperation, LocalCloud};
pub use crypto::{decrypt, derive_key, encrypt};
pub use diff::{diff_index_files, diff_upsert_remove, IndexFileDiff};
pub use entity::{
    random_hash, sha1_hex, CheckIndex, CheckIndexFile, Chunk, File, Index, MergeResult, TrafficStat,
};
pub use error::RepoError;
pub use history::History;
pub use purge::PurgeStat;
pub use ref_store::RefStore;
pub use repo::{Device, Repo, RepoOptions, RepoPaths};
pub use store::{RawObjectKind, Store};
pub use sync_lock::{RemoteLockGuard, RemoteLockHealthError};
pub use working_tree::{
    with_working_tree_permit, ExpectedRevision, NoopWorkingTreeCoordinator, RepositoryRelativePath,
    WorkingTreeAction, WorkingTreeChange, WorkingTreeCoordinator, WorkingTreePermit,
};

pub const UPSTREAM_DEJAVU_COMMIT: &str = "8462fe30163c6e6e95ae2da832cfe76058e0e830";
pub const UPSTREAM_SIYUAN_COMMIT: &str = "eef10568384e2e7cf547adb029ae46a72e43c287";
