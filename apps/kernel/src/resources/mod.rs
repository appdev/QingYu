//! Kernel-owned binary workspace resource domain.
//!
//! Resource storage, identity, streaming, and cleanup remain separate from the
//! UTF-8 Markdown document API. Production capability reporting stays disabled
//! until the service and transport adapters are fully composed.

mod error;
mod href;
mod policy;
mod service;
mod transaction;

pub use error::{ResourceServiceError, ResourceServiceErrorKind};
pub use href::resolve_markdown_href;
pub use service::{
    CreateResourceBatchItem, RetainedResource, WorkspaceInventoryEntry, WorkspaceResourceService,
    MAX_RESOURCE_BATCH_ITEMS, MAX_RESOURCE_BODY_BYTES,
};
