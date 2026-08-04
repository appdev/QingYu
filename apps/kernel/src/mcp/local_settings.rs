use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use serde_json::Value;


use super::config::{McpConfig, McpConfigDocument, McpConfigError};
use crate::storage::{
    create_private_replaceable_file_options, nonfollowing_read_options,
    open_canonical_directory_nofollow, rename_retained_file_in_directory, sync_directory,
    unique_regular_file_identity,
};

const MCP_CONFIG_FILE: &str = "mcp.json";
const MAX_MCP_CONFIG_BYTES: u64 = 1024 * 1024;
pub const MCP_POLICY_CHANGED_EVENT: &str = "qingyu://settings-mcp-changed";

pub trait McpSettingsBackend: Send + Sync {
    fn read(&self) -> Result<Option<Value>, McpConfigError>;
    fn write(&self, value: &Value) -> Result<(), McpConfigError>;
}

pub trait McpPolicyEventSink: Send + Sync {
    fn emit(&self, config: &McpConfig) -> Result<(), McpConfigError>;
}
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopMcpPolicyEventSink;

impl McpPolicyEventSink for NoopMcpPolicyEventSink {
    fn emit(&self, _config: &McpConfig) -> Result<(), McpConfigError> {
        Ok(())
    }
}


struct FileMcpSettingsBackend {
    config_root: PathBuf,
}

impl FileMcpSettingsBackend {
    fn at(config_root: PathBuf) -> Self {
        Self { config_root }
    }
}

impl McpSettingsBackend for FileMcpSettingsBackend {
    fn read(&self) -> Result<Option<Value>, McpConfigError> {
        let directory = match open_canonical_directory_nofollow(&self.config_root) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(McpConfigError::read()),
        };
        let metadata = match directory.symlink_metadata(MCP_CONFIG_FILE) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(McpConfigError::read()),
        };
        let Some(identity) = unique_regular_file_identity(&metadata) else {
            return Err(McpConfigError::read());
        };
        if metadata.len() > MAX_MCP_CONFIG_BYTES {
            return Err(McpConfigError::read());
        }
        let file = directory
            .open_with(MCP_CONFIG_FILE, &nonfollowing_read_options())
            .map_err(|_| McpConfigError::read())?;
        if !identity.matches_retained_regular_file(
            &file.metadata().map_err(|_| McpConfigError::read())?,
            false,
        ) {
            return Err(McpConfigError::read());
        }
        let mut bytes = Vec::new();
        file.take(MAX_MCP_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| McpConfigError::read())?;
        if bytes.len() as u64 > MAX_MCP_CONFIG_BYTES {
            return Err(McpConfigError::read());
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| McpConfigError::read())
    }

    fn write(&self, value: &Value) -> Result<(), McpConfigError> {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        fs::create_dir_all(&self.config_root).map_err(|_| McpConfigError::write())?;
        let bytes = serde_json::to_vec(value).map_err(|_| McpConfigError::write())?;
        if bytes.len() as u64 > MAX_MCP_CONFIG_BYTES {
            return Err(McpConfigError::write());
        }
        let directory = open_canonical_directory_nofollow(&self.config_root)
            .map_err(|_| McpConfigError::write())?;
        match directory.symlink_metadata(MCP_CONFIG_FILE) {
            Ok(metadata) if unique_regular_file_identity(&metadata).is_none() => {
                return Err(McpConfigError::write());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(McpConfigError::write()),
        }
        let staged = (0..1000)
            .find_map(|_| {
                let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let name = format!(".mcp-{}-{sequence}.tmp", std::process::id());
                match directory.open_with(&name, &create_private_replaceable_file_options()) {
                    Ok(file) => Some(Ok((name, file))),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(_) => Some(Err(McpConfigError::write())),
                }
            })
            .ok_or_else(McpConfigError::write)??;
        let (staged_name, mut staged_file) = staged;
        let publication = staged_file
            .write_all(&bytes)
            .and_then(|()| staged_file.sync_all());
        if publication.is_err() {
            drop(staged_file);
            let _cleanup = directory.remove_file(&staged_name);
            return Err(McpConfigError::write());
        }
        let Some(identity) = staged_file
            .metadata()
            .ok()
            .and_then(|metadata| unique_regular_file_identity(&metadata))
        else {
            drop(staged_file);
            let _cleanup = directory.remove_file(&staged_name);
            return Err(McpConfigError::write());
        };
        if rename_retained_file_in_directory(
            &directory,
            &staged_file,
            &staged_name,
            identity,
            MCP_CONFIG_FILE,
            true,
        )
        .is_err()
        {
            drop(staged_file);
            let _cleanup = directory.remove_file(&staged_name);
            return Err(McpConfigError::write());
        }
        sync_directory(&directory).map_err(|_| McpConfigError::write())
    }
}



#[derive(Clone)]
pub struct McpLocalSettingsService {
    backend: Arc<dyn McpSettingsBackend>,
    events: Option<Arc<dyn McpPolicyEventSink>>,
    transaction: Arc<Mutex<()>>,
}

impl McpLocalSettingsService {
    pub fn at_config_root(config_root: PathBuf) -> Self {
        Self {
            backend: Arc::new(FileMcpSettingsBackend::at(config_root)),
            events: None,
            transaction: Arc::new(Mutex::new(())),
        }
    }


    pub fn at_config_root_with_events(
        config_root: PathBuf,
        events: Arc<dyn McpPolicyEventSink>,
    ) -> Self {
        Self {
            backend: Arc::new(FileMcpSettingsBackend::at(config_root)),
            events: Some(events),
            transaction: Arc::new(Mutex::new(())),
        }
    }



    #[cfg(test)]
    pub fn new_for_test(
        backend: Arc<dyn McpSettingsBackend>,
        events: Option<Arc<dyn McpPolicyEventSink>>,
    ) -> Self {
        Self {
            backend,
            events,
            transaction: Arc::new(Mutex::new(())),
        }
    }

    #[cfg(test)]
    pub fn memory_for_test() -> Self {
        Self::new_for_test(Arc::new(MemoryMcpSettingsBackend::default()), None)
    }

    #[cfg(test)]
    fn at_config_root_for_test(config_root: PathBuf) -> Self {
        Self::at_config_root(config_root)
    }

    pub fn load(&self) -> Result<McpConfigDocument, McpConfigError> {
        let _guard = self
            .transaction
            .lock()
            .map_err(|_| McpConfigError::read())?;
        self.load_unlocked()
    }

    pub fn write(
        &self,
        expected_revision: &str,
        config: McpConfig,
    ) -> Result<McpConfigDocument, McpConfigError> {
        let _guard = self
            .transaction
            .lock()
            .map_err(|_| McpConfigError::read())?;
        let current = self.load_unlocked()?;
        if current.revision != expected_revision {
            return Err(McpConfigError::revision_conflict());
        }
        let updated = McpConfigDocument::from_config(config)?;
        let value = serde_json::to_value(&updated.config).map_err(|_| McpConfigError::write())?;
        self.backend.write(&value)?;
        if let Some(events) = &self.events {
            let _event_result = events.emit(&updated.config);
        }
        Ok(updated)
    }

    fn load_unlocked(&self) -> Result<McpConfigDocument, McpConfigError> {
        match self.backend.read()? {
            Some(value) => normalized_document(&value).ok_or_else(McpConfigError::read),
            None => Ok(default_document()),
        }
    }
}

fn normalized_document(value: &Value) -> Option<McpConfigDocument> {
    serde_json::from_value::<McpConfig>(value.clone())
        .ok()
        .and_then(|config| McpConfigDocument::from_config(config).ok())
}

fn default_document() -> McpConfigDocument {
    McpConfigDocument::from_config(McpConfig::default())
        .expect("the default MCP configuration must be valid")
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryMcpSettingsBackend {
    fail_writes: AtomicUsize,
    value: Mutex<Option<Value>>,
}

#[cfg(test)]
impl MemoryMcpSettingsBackend {
    fn fail_next_write(&self) {
        self.fail_writes.fetch_add(1, Ordering::Relaxed);
    }

    fn value(&self) -> Option<Value> {
        self.value.lock().expect("MCP memory settings").clone()
    }
}

#[cfg(test)]
impl McpSettingsBackend for MemoryMcpSettingsBackend {
    fn read(&self) -> Result<Option<Value>, McpConfigError> {
        Ok(self.value())
    }

    fn write(&self, value: &Value) -> Result<(), McpConfigError> {
        if self
            .fail_writes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(McpConfigError::write());
        }
        *self.value.lock().map_err(|_| McpConfigError::write())? = Some(value.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{McpLocalSettingsService, MemoryMcpSettingsBackend};
    use crate::mcp::config::McpConfig;

    #[test]
    fn dedicated_document_write_does_not_read_or_mutate_other_config_files() {
        let config_root = tempfile::tempdir().expect("temporary config root");
        let settings = config_root.path().join("settings.json");
        let local_state = config_root.path().join("local-state.json");
        std::fs::write(&settings, br#"{"language":"fr"}"#).expect("settings sentinel");
        std::fs::write(&local_state, br#"{"primaryWorkspace":"sentinel"}"#)
            .expect("local-state sentinel");
        let service =
            McpLocalSettingsService::at_config_root_for_test(config_root.path().to_path_buf());
        let initial = service.load().expect("default MCP config");

        let updated = service
            .write(
                &initial.revision,
                McpConfig {
                    enabled: true,
                    ..initial.config
                },
            )
            .expect("write MCP config");

        assert!(updated.config.enabled);
        let replaced = service
            .write(
                &updated.revision,
                McpConfig {
                    enabled: false,
                    ..updated.config
                },
            )
            .expect("replace MCP config");
        assert!(!replaced.config.enabled);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(config_root.path().join("mcp.json")).expect("MCP document"),
            )
            .expect("MCP JSON"),
            serde_json::to_value(&replaced.config).expect("normalized MCP config"),
        );
        assert_eq!(
            std::fs::read(settings).expect("settings sentinel remains"),
            br#"{"language":"fr"}"#,
        );
        assert_eq!(
            std::fs::read(local_state).expect("local-state sentinel remains"),
            br#"{"primaryWorkspace":"sentinel"}"#,
        );
    }

    #[test]
    fn write_normalizes_the_dedicated_config_and_enforces_revision() {
        let backend = Arc::new(MemoryMcpSettingsBackend::default());
        let service = McpLocalSettingsService::new_for_test(backend.clone(), None);
        let initial = service.load().expect("default MCP config");
        let mut requested = initial.config.clone();
        requested.enabled = true;
        requested.concurrent_calls = usize::MAX;

        let updated = service
            .write(&initial.revision, requested)
            .expect("write normalized config");

        assert_eq!(updated.config.concurrent_calls, 32);
        assert_eq!(backend.value(), serde_json::to_value(updated.config).ok());
        assert_eq!(
            service
                .write(&initial.revision, McpConfig::default())
                .expect_err("stale revision")
                .code,
            "revision-conflict",
        );
    }

    #[test]
    fn failed_dedicated_document_write_preserves_the_previous_value() {
        let backend = Arc::new(MemoryMcpSettingsBackend::default());
        let service = McpLocalSettingsService::new_for_test(backend.clone(), None);
        let initial = service.load().expect("default MCP config");
        backend.fail_next_write();

        let error = service
            .write(
                &initial.revision,
                McpConfig {
                    enabled: true,
                    ..initial.config
                },
            )
            .expect_err("injected write failure");

        assert_eq!(error.code, "mcp-config-write-failed");
        assert_eq!(backend.value(), None);
    }

    #[test]
    fn malformed_dedicated_document_fails_closed() {
        let backend = Arc::new(MemoryMcpSettingsBackend::default());
        *backend.value.lock().expect("MCP memory settings") = Some(json!({ "version": 99 }));
        let service = McpLocalSettingsService::new_for_test(backend, None);

        assert_eq!(
            service.load().expect_err("invalid MCP config").code,
            "mcp-config-read-failed",
        );
    }
}
