use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use qingyu_kernel::mcp::McpLocalSettingsService;
pub(crate) use qingyu_kernel::mcp::{
    McpConfig, McpConfigDocument, McpConfigError,
    AuditPolicy, ConfirmationPolicy, DeletionPolicy, DryRunPolicy,
    McpPermissions, SyncAfterWritePolicy, SyncExecutionPolicy, ToolCapability,
};

pub(crate) const MCP_CONFIG_VERSION: u32 = 1;

#[derive(Debug)]
struct McpConfigState {
    document: McpConfigDocument,
}

pub(crate) struct McpConfigManager {
    settings: McpLocalSettingsService,
    state: Mutex<McpConfigState>,
    generation: AtomicU64,
}

impl McpConfigManager {
    pub(crate) fn load(settings: McpLocalSettingsService) -> Result<Self, McpConfigError> {
        let document = settings.load()?;
        Ok(Self {
            settings,
            state: Mutex::new(McpConfigState { document }),
            generation: AtomicU64::new(1),
        })
    }

    #[cfg(test)]
    pub(crate) fn memory_for_test() -> Result<Self, McpConfigError> {
        Self::load(McpLocalSettingsService::memory_for_test())
    }

    pub(crate) fn snapshot(&self) -> Result<McpConfigDocument, McpConfigError> {
        self.state
            .lock()
            .map(|state| state.document.clone())
            .map_err(|_| McpConfigError::read())
    }

    pub(crate) fn snapshot_with_generation(
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

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn reload(&self) -> Result<McpConfigDocument, McpConfigError> {
        let document = self.settings.load()?;
        let mut state = self.state.lock().map_err(|_| McpConfigError::read())?;
        if state.document.revision != document.revision {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        state.document = document.clone();
        Ok(document)
    }

    pub(crate) fn update(
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
