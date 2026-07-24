pub mod diff;
pub mod entity;

mod error;

pub use diff::{diff_index_files, diff_upsert_remove, IndexFileDiff};
pub use entity::{
    random_hash, sha1_hex, CheckIndex, CheckIndexFile, Chunk, File, Index, MergeResult,
    TrafficStat,
};
pub use error::RepoError;

pub const UPSTREAM_DEJAVU_COMMIT: &str = "8462fe30163c6e6e95ae2da832cfe76058e0e830";
pub const UPSTREAM_SIYUAN_COMMIT: &str = "eef10568384e2e7cf547adb029ae46a72e43c287";
