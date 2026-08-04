use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::local_settings::McpLocalSettingsService;

pub const MCP_CONFIG_VERSION: u32 = 1;
const DEFAULT_LIMIT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LIMIT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfirmationPolicy {
    Never,
    DestructiveOnly,
    AllWrites,
}

impl Default for ConfirmationPolicy {
    fn default() -> Self {
        Self::DestructiveOnly
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DryRunPolicy {
    Never,
    HighRisk,
    AllWrites,
}

impl Default for DryRunPolicy {
    fn default() -> Self {
        Self::HighRisk
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeletionPolicy {
    Recoverable,
    Permanent,
}

impl Default for DeletionPolicy {
    fn default() -> Self {
        Self::Recoverable
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncAfterWritePolicy {
    FollowWorkspace,
    Always,
    Never,
}

impl Default for SyncAfterWritePolicy {
    fn default() -> Self {
        Self::FollowWorkspace
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncExecutionPolicy {
    Background,
    Wait,
}

impl Default for SyncExecutionPolicy {
    fn default() -> Self {
        Self::Background
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct AuditPolicy {
    pub enabled: bool,
    pub retention_days: u16,
    pub max_entries: usize,
}

impl AuditPolicy {
    fn normalize(&mut self) {
        self.retention_days = self.retention_days.clamp(1, 365);
        self.max_entries = self.max_entries.clamp(100, 100_000);
    }
}

impl Default for AuditPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 30,
            max_entries: 10_000,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct McpPermissions {
    pub documents_read: bool,
    pub documents_write: bool,
    pub documents_move: bool,
    pub documents_delete: bool,
    pub settings_read: bool,
    pub settings_write: bool,
    pub sync_read: bool,
    pub sync_write: bool,
    pub sync_credentials_write: bool,
    pub sync_run: bool,
}

impl McpPermissions {
    pub fn allows(&self, capability: ToolCapability) -> bool {
        match capability {
            ToolCapability::DocumentsRead => self.documents_read,
            ToolCapability::DocumentsWrite => self.documents_write,
            ToolCapability::DocumentsMove => self.documents_move,
            ToolCapability::DocumentsDelete => self.documents_delete,
            ToolCapability::SettingsRead => self.settings_read,
            ToolCapability::SettingsWrite => self.settings_write,
            ToolCapability::SyncRead => self.sync_read,
            ToolCapability::SyncWrite => self.sync_write,
            ToolCapability::SyncCredentialsWrite => self.sync_credentials_write,
            ToolCapability::SyncRun => self.sync_run,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ToolCapability {
    DocumentsRead,
    DocumentsWrite,
    DocumentsMove,
    DocumentsDelete,
    SettingsRead,
    SettingsWrite,
    SyncRead,
    SyncWrite,
    SyncCredentialsWrite,
    SyncRun,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct McpConfig {
    pub version: u32,
    pub enabled: bool,
    pub permissions: McpPermissions,
    pub confirmation: ConfirmationPolicy,
    pub dry_run: DryRunPolicy,
    pub deletion: DeletionPolicy,
    pub sync_after_write: SyncAfterWritePolicy,
    pub sync_execution: SyncExecutionPolicy,
    pub document_limit_bytes: u64,
    pub request_limit_bytes: u64,
    pub response_limit_bytes: u64,
    pub requests_per_minute: u32,
    pub burst_requests: u32,
    pub concurrent_calls: usize,
    pub tool_timeout_secs: u64,
    pub audit: AuditPolicy,
}

impl McpConfig {
    pub fn normalize(&mut self) {
        self.document_limit_bytes = self.document_limit_bytes.clamp(1, MAX_LIMIT_BYTES);
        self.request_limit_bytes = self.request_limit_bytes.clamp(1, MAX_LIMIT_BYTES);
        self.response_limit_bytes = self.response_limit_bytes.clamp(1, MAX_LIMIT_BYTES);
        self.requests_per_minute = self.requests_per_minute.clamp(1, 600);
        self.burst_requests = self.burst_requests.clamp(1, 100);
        self.concurrent_calls = self.concurrent_calls.clamp(1, 32);
        self.tool_timeout_secs = self.tool_timeout_secs.clamp(5, 600);
        self.audit.normalize();
    }

    fn validate(&self) -> Result<(), McpConfigError> {
        if self.version != MCP_CONFIG_VERSION {
            return Err(McpConfigError::invalid());
        }
        Ok(())
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            version: MCP_CONFIG_VERSION,
            enabled: false,
            permissions: McpPermissions::default(),
            confirmation: ConfirmationPolicy::default(),
            dry_run: DryRunPolicy::default(),
            deletion: DeletionPolicy::default(),
            sync_after_write: SyncAfterWritePolicy::default(),
            sync_execution: SyncExecutionPolicy::default(),
            document_limit_bytes: DEFAULT_LIMIT_BYTES,
            request_limit_bytes: DEFAULT_LIMIT_BYTES,
            response_limit_bytes: DEFAULT_LIMIT_BYTES,
            requests_per_minute: 120,
            burst_requests: 20,
            concurrent_calls: 8,
            tool_timeout_secs: 60,
            audit: AuditPolicy::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpConfigDocument {
    pub config: McpConfig,
    pub revision: String,
}

impl McpConfigDocument {
    pub fn from_config(mut config: McpConfig) -> Result<Self, McpConfigError> {
        config.normalize();
        config.validate()?;
        let bytes = serde_json::to_vec(&config).map_err(|_| McpConfigError::serialize())?;
        let revision = format!("{:x}", Sha256::digest(bytes));
        Ok(Self { config, revision })
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, McpConfigError> {
        let config =
            serde_json::from_slice::<McpConfig>(bytes).map_err(|_| McpConfigError::malformed())?;
        Self::from_config(config)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct McpConfigError {
    pub code: &'static str,
    message: &'static str,
}

impl McpConfigError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    fn invalid() -> Self {
        Self::new("mcp-config-invalid", "The MCP configuration is invalid.")
    }

    fn malformed() -> Self {
        Self::new(
            "mcp-config-malformed",
            "The MCP configuration could not be loaded.",
        )
    }

    fn serialize() -> Self {
        Self::new(
            "mcp-config-serialize-failed",
            "The MCP configuration could not be serialized.",
        )
    }

    pub fn read() -> Self {
        Self::new(
            "mcp-config-read-failed",
            "The MCP configuration could not be read.",
        )
    }

    pub fn write() -> Self {
        Self::new(
            "mcp-config-write-failed",
            "The MCP configuration could not be written.",
        )
    }

    pub fn revision_conflict() -> Self {
        Self::new(
            "revision-conflict",
            "The MCP configuration changed before this update.",
        )
    }
}

impl fmt::Display for McpConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for McpConfigError {}

#[derive(Debug)]
struct McpConfigState {
    document: McpConfigDocument,
}

pub struct McpConfigManager {
    settings: McpLocalSettingsService,
    state: Mutex<McpConfigState>,
    generation: AtomicU64,
}

impl McpConfigManager {
    pub fn load(settings: McpLocalSettingsService) -> Result<Self, McpConfigError> {
        let document = settings.load()?;
        Ok(Self {
            settings,
            state: Mutex::new(McpConfigState { document }),
            generation: AtomicU64::new(1),
        })
    }

    #[cfg(test)]
    pub fn memory_for_test() -> Result<Self, McpConfigError> {
        Self::load(McpLocalSettingsService::memory_for_test())
    }

    pub fn snapshot(&self) -> Result<McpConfigDocument, McpConfigError> {
        self.state
            .lock()
            .map(|state| state.document.clone())
            .map_err(|_| McpConfigError::read())
    }

    pub fn snapshot_with_generation(
        &self,
    ) -> Result<(McpConfigDocument, u64), McpConfigError> {
        self.state
            .lock()
            .map(|state| {
                (
                    state.document.clone(),
                    self.generation.load(Ordering::Acquire),
                )
            })
            .map_err(|_| McpConfigError::read())
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn reload(&self) -> Result<McpConfigDocument, McpConfigError> {
        let document = self.settings.load()?;
        let mut state = self.state.lock().map_err(|_| McpConfigError::read())?;
        if state.document.revision != document.revision {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        state.document = document.clone();
        Ok(document)
    }

    pub fn update(
        &self,
        config: McpConfig,
        expected_revision: &str,
    ) -> Result<McpConfigDocument, McpConfigError> {
        let mut state = self.state.lock().map_err(|_| McpConfigError::read())?;
        if state.document.revision != expected_revision {
            return Err(McpConfigError::revision_conflict());
        }
        let updated = self.settings.write(expected_revision, config)?;
        if updated.revision != state.document.revision {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        state.document = updated.clone();
        Ok(updated)
    }
}
