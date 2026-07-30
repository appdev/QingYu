//! Kernel-owned sync configuration and execution.

pub mod backend;
pub mod config;
pub mod credentials;
pub mod diagnostics;
pub mod editing;
pub mod engine;
pub mod execution;
pub mod repository;
#[allow(dead_code)] // Staged until the Kernel-owned production executor is composed.
pub(crate) mod s3_backend;
pub mod s3_http;
pub mod scope;
pub mod service;
pub mod settings_scope;
pub mod status;
