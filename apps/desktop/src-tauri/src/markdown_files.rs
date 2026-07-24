mod asset;
#[cfg(desktop)]
pub(crate) mod attachment;
pub(crate) mod document;
#[cfg(desktop)]
pub(crate) mod export;
pub(crate) mod history;
mod ignore_rules;
pub(crate) mod image;
#[cfg(desktop)]
pub(crate) mod open;
pub(crate) mod path;
#[cfg(desktop)]
pub(crate) mod resource;
mod resource_writer;
pub(crate) mod search;
#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[allow(dead_code)]
mod service;
#[cfg(desktop)]
pub(crate) mod template;
pub(crate) mod tree;
mod trusted_file;
mod types;

pub(crate) use ignore_rules::MarkdownIgnoreRules;
#[cfg(desktop)]
pub(crate) use path::markdown_open_path_for_path;
#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[allow(unused_imports)]
pub(crate) use service::{
    CreateDocument, DeleteDocument, DocumentScope, DocumentService, MoveDocument, MutationOptions,
    SyncRequest, UpdateDocument,
};
pub(crate) use tree::MarkdownTreeLoadState;
