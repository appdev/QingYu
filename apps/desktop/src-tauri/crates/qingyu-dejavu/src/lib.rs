pub mod atomic_write;
pub mod crypto;
pub mod diff;
pub mod entity;
pub mod store;

mod error;

pub use atomic_write::write_file_safer;
pub use crypto::{decrypt, derive_key, encrypt};
pub use diff::{diff_index_files, diff_upsert_remove, IndexFileDiff};
pub use entity::{
    random_hash, sha1_hex, CheckIndex, CheckIndexFile, Chunk, File, Index, MergeResult, TrafficStat,
};
pub use error::RepoError;
pub use store::Store;

pub const UPSTREAM_DEJAVU_COMMIT: &str = "8462fe30163c6e6e95ae2da832cfe76058e0e830";
pub const UPSTREAM_SIYUAN_COMMIT: &str = "eef10568384e2e7cf547adb029ae46a72e43c287";
