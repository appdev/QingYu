use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;
use tauri::{Emitter, Runtime};
use tauri_plugin_store::StoreExt;

use super::config::{McpConfig, McpConfigDocument, McpConfigError};

const LOCAL_STATE_STORE_PATH: &str = "local-state.json";
const SETTINGS_STORE_PATH: &str = "settings.json";
const LOCAL_STATE_SCHEMA_VERSION_KEY: &str = "schemaVersion";
const LOCAL_STATE_SCHEMA_VERSION: u64 = 2;
const MCP_SETTINGS_KEY: &str = "mcp";
pub(crate) const MCP_POLICY_CHANGED_EVENT: &str = "qingyu://settings-mcp-changed";

pub(crate) trait McpSettingsBackend: Send + Sync {
    fn get_local(&self, key: &str) -> Result<Option<Value>, McpConfigError>;
    fn set_local(&self, key: &str, value: Value) -> Result<(), McpConfigError>;
    fn delete_local(&self, key: &str) -> Result<(), McpConfigError>;
    fn save_local(&self) -> Result<(), McpConfigError>;
    fn get_legacy(&self, key: &str) -> Result<Option<Value>, McpConfigError>;
    fn set_legacy(&self, key: &str, value: Value) -> Result<(), McpConfigError>;
    fn delete_legacy(&self, key: &str) -> Result<(), McpConfigError>;
    fn save_legacy(&self) -> Result<(), McpConfigError>;
}

pub(crate) trait McpPolicyEventSink: Send + Sync {
    fn emit(&self, config: &McpConfig) -> Result<(), McpConfigError>;
}

struct StoreMcpSettingsBackend<R: Runtime> {
    legacy: Arc<tauri_plugin_store::Store<R>>,
    local: Arc<tauri_plugin_store::Store<R>>,
}

impl<R: Runtime> McpSettingsBackend for StoreMcpSettingsBackend<R> {
    fn get_local(&self, key: &str) -> Result<Option<Value>, McpConfigError> {
        Ok(self.local.get(key))
    }

    fn set_local(&self, key: &str, value: Value) -> Result<(), McpConfigError> {
        self.local.set(key, value);
        Ok(())
    }

    fn delete_local(&self, key: &str) -> Result<(), McpConfigError> {
        self.local.delete(key);
        Ok(())
    }

    fn save_local(&self) -> Result<(), McpConfigError> {
        self.local.save().map_err(|_| McpConfigError::write())
    }

    fn get_legacy(&self, key: &str) -> Result<Option<Value>, McpConfigError> {
        Ok(self.legacy.get(key))
    }

    fn set_legacy(&self, key: &str, value: Value) -> Result<(), McpConfigError> {
        self.legacy.set(key, value);
        Ok(())
    }

    fn delete_legacy(&self, key: &str) -> Result<(), McpConfigError> {
        self.legacy.delete(key);
        Ok(())
    }

    fn save_legacy(&self) -> Result<(), McpConfigError> {
        self.legacy.save().map_err(|_| McpConfigError::write())
    }
}

struct TauriMcpPolicyEventSink<R: Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: Runtime> McpPolicyEventSink for TauriMcpPolicyEventSink<R> {
    fn emit(&self, config: &McpConfig) -> Result<(), McpConfigError> {
        self.app
            .emit(
                MCP_POLICY_CHANGED_EVENT,
                serde_json::json!({ "config": config }),
            )
            .map_err(|_| McpConfigError::write())
    }
}

#[derive(Clone)]
pub(crate) struct McpLocalSettingsService {
    backend: Arc<dyn McpSettingsBackend>,
    events: Option<Arc<dyn McpPolicyEventSink>>,
}

impl McpLocalSettingsService {
    pub(crate) fn from_app<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Self, McpConfigError> {
        let local = app
            .store_builder(LOCAL_STATE_STORE_PATH)
            .disable_auto_save()
            .build()
            .map_err(|_| McpConfigError::read())?;
        let legacy = app
            .store_builder(SETTINGS_STORE_PATH)
            .disable_auto_save()
            .build()
            .map_err(|_| McpConfigError::read())?;
        Ok(Self {
            backend: Arc::new(StoreMcpSettingsBackend { legacy, local }),
            events: Some(Arc::new(TauriMcpPolicyEventSink { app: app.clone() })),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        backend: Arc<dyn McpSettingsBackend>,
        events: Option<Arc<dyn McpPolicyEventSink>>,
    ) -> Self {
        Self { backend, events }
    }

    #[cfg(test)]
    pub(crate) fn memory_for_test() -> Self {
        Self::new_for_test(Arc::new(MemoryMcpSettingsBackend::default()), None)
    }

    pub(crate) fn load_migrated(&self) -> Result<McpConfigDocument, McpConfigError> {
        let _guard = transaction_lock()
            .lock()
            .map_err(|_| McpConfigError::read())?;
        self.load_migrated_unlocked()
    }

    pub(crate) fn write(
        &self,
        expected_revision: &str,
        config: McpConfig,
    ) -> Result<McpConfigDocument, McpConfigError> {
        let _guard = transaction_lock()
            .lock()
            .map_err(|_| McpConfigError::read())?;
        let current = self.load_migrated_unlocked()?;
        if current.revision != expected_revision {
            return Err(McpConfigError::revision_conflict());
        }
        let updated = McpConfigDocument::from_config(config)?;
        let value = serde_json::to_value(&updated.config).map_err(|_| McpConfigError::write())?;
        let previous_config = self.backend.get_local(MCP_SETTINGS_KEY)?;
        let previous_schema = self.backend.get_local(LOCAL_STATE_SCHEMA_VERSION_KEY)?;
        self.backend.set_local(MCP_SETTINGS_KEY, value)?;
        self.backend.set_local(
            LOCAL_STATE_SCHEMA_VERSION_KEY,
            Value::from(LOCAL_STATE_SCHEMA_VERSION),
        )?;
        if let Err(error) = self.backend.save_local() {
            self.restore_local(MCP_SETTINGS_KEY, previous_config);
            self.restore_local(LOCAL_STATE_SCHEMA_VERSION_KEY, previous_schema);
            return Err(error);
        }
        if let Some(events) = &self.events {
            let _event_result = events.emit(&updated.config);
        }
        Ok(updated)
    }

    fn load_migrated_unlocked(&self) -> Result<McpConfigDocument, McpConfigError> {
        let local = self.backend.get_local(MCP_SETTINGS_KEY)?;
        let legacy = self.backend.get_legacy(MCP_SETTINGS_KEY)?;
        let document = match local.as_ref() {
            Some(value) => normalized_document(value).unwrap_or_else(default_document),
            None => legacy
                .as_ref()
                .and_then(normalized_document)
                .unwrap_or_else(default_document),
        };
        let canonical =
            serde_json::to_value(&document.config).map_err(|_| McpConfigError::write())?;
        let previous_schema = self.backend.get_local(LOCAL_STATE_SCHEMA_VERSION_KEY)?;
        let needs_local_save = local.as_ref() != Some(&canonical)
            || previous_schema.as_ref() != Some(&Value::from(LOCAL_STATE_SCHEMA_VERSION));
        let local_is_durable = if needs_local_save {
            self.backend.set_local(MCP_SETTINGS_KEY, canonical)?;
            self.backend.set_local(
                LOCAL_STATE_SCHEMA_VERSION_KEY,
                Value::from(LOCAL_STATE_SCHEMA_VERSION),
            )?;
            match self.backend.save_local() {
                Ok(()) => true,
                Err(error) => {
                    self.restore_local(MCP_SETTINGS_KEY, local);
                    self.restore_local(LOCAL_STATE_SCHEMA_VERSION_KEY, previous_schema);
                    eprintln!("QingYu MCP local migration skipped: {}", error.code);
                    false
                }
            }
        } else {
            true
        };
        if local_is_durable {
            if let Some(legacy) = legacy {
                self.backend.delete_legacy(MCP_SETTINGS_KEY)?;
                if let Err(error) = self.backend.save_legacy() {
                    let _restore_result = self.backend.set_legacy(MCP_SETTINGS_KEY, legacy);
                    eprintln!("QingYu MCP legacy cleanup skipped: {}", error.code);
                }
            }
        }
        Ok(document)
    }

    fn restore_local(&self, key: &str, value: Option<Value>) {
        let _restore_result = match value {
            Some(value) => self.backend.set_local(key, value),
            None => self.backend.delete_local(key),
        };
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

fn transaction_lock() -> &'static Mutex<()> {
    static TRANSACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TRANSACTION_LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct MemoryMcpSettingsBackend {
    fail_legacy_saves: std::sync::atomic::AtomicUsize,
    fail_local_saves: std::sync::atomic::AtomicUsize,
    legacy: Mutex<std::collections::BTreeMap<String, Value>>,
    local: Mutex<std::collections::BTreeMap<String, Value>>,
    operations: Mutex<Vec<&'static str>>,
}

#[cfg(test)]
impl MemoryMcpSettingsBackend {
    fn with_legacy(value: Value) -> Self {
        let backend = Self::default();
        backend
            .legacy
            .lock()
            .unwrap()
            .insert(MCP_SETTINGS_KEY.to_string(), value);
        backend
    }

    fn with_local_and_legacy(local: Value, legacy: Value) -> Self {
        let backend = Self::with_legacy(legacy);
        backend
            .local
            .lock()
            .unwrap()
            .insert(MCP_SETTINGS_KEY.to_string(), local);
        backend
    }

    fn with_local_entry(key: &str, value: Value) -> Self {
        let backend = Self::default();
        backend.local.lock().unwrap().insert(key.to_string(), value);
        backend
    }

    fn fail_next_local_save(&self) {
        self.fail_local_saves
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn fail_next_legacy_save(&self) {
        self.fail_legacy_saves
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn local(&self, key: &str) -> Option<Value> {
        self.local.lock().unwrap().get(key).cloned()
    }

    fn legacy(&self, key: &str) -> Option<Value> {
        self.legacy.lock().unwrap().get(key).cloned()
    }

    fn local_save_happened_before_legacy_delete(&self) -> bool {
        let operations = self.operations.lock().unwrap();
        operations
            .iter()
            .position(|operation| *operation == "save-local")
            < operations
                .iter()
                .position(|operation| *operation == "delete-legacy")
    }

    fn consume_failure(counter: &std::sync::atomic::AtomicUsize) -> bool {
        counter
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |remaining| remaining.checked_sub(1),
            )
            .is_ok()
    }
}

#[cfg(test)]
impl McpSettingsBackend for MemoryMcpSettingsBackend {
    fn get_local(&self, key: &str) -> Result<Option<Value>, McpConfigError> {
        Ok(self.local(key))
    }

    fn set_local(&self, key: &str, value: Value) -> Result<(), McpConfigError> {
        self.local.lock().unwrap().insert(key.to_string(), value);
        Ok(())
    }

    fn delete_local(&self, key: &str) -> Result<(), McpConfigError> {
        self.local.lock().unwrap().remove(key);
        Ok(())
    }

    fn save_local(&self) -> Result<(), McpConfigError> {
        self.operations.lock().unwrap().push("save-local");
        if Self::consume_failure(&self.fail_local_saves) {
            Err(McpConfigError::write())
        } else {
            Ok(())
        }
    }

    fn get_legacy(&self, key: &str) -> Result<Option<Value>, McpConfigError> {
        Ok(self.legacy(key))
    }

    fn set_legacy(&self, key: &str, value: Value) -> Result<(), McpConfigError> {
        self.legacy.lock().unwrap().insert(key.to_string(), value);
        Ok(())
    }

    fn delete_legacy(&self, key: &str) -> Result<(), McpConfigError> {
        self.operations.lock().unwrap().push("delete-legacy");
        self.legacy.lock().unwrap().remove(key);
        Ok(())
    }

    fn save_legacy(&self) -> Result<(), McpConfigError> {
        self.operations.lock().unwrap().push("save-legacy");
        if Self::consume_failure(&self.fail_legacy_saves) {
            Err(McpConfigError::write())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{McpLocalSettingsService, MemoryMcpSettingsBackend};
    use crate::mcp::config::McpConfig;

    #[test]
    fn legacy_policy_is_normalized_saved_locally_then_deleted() {
        let backend = Arc::new(MemoryMcpSettingsBackend::with_legacy(json!({
            "version": 1,
            "enabled": true,
            "permissions": { "documentsRead": true }
        })));
        let service = McpLocalSettingsService::new_for_test(backend.clone(), None);

        let document = service.load_migrated().unwrap();

        assert!(document.config.enabled);
        assert!(document.config.permissions.documents_read);
        assert_eq!(document.config.recycle_bin_retention_days, 30);
        assert_eq!(backend.local("schemaVersion"), Some(json!(2)));
        assert!(backend.local("mcp").is_some());
        assert_eq!(backend.legacy("mcp"), None);
        assert!(backend.local_save_happened_before_legacy_delete());
    }

    #[test]
    fn local_policy_wins_over_a_different_legacy_policy() {
        let backend = Arc::new(MemoryMcpSettingsBackend::with_local_and_legacy(
            json!({ "version": 1, "enabled": false }),
            json!({ "version": 1, "enabled": true }),
        ));
        let service = McpLocalSettingsService::new_for_test(backend.clone(), None);

        let document = service.load_migrated().unwrap();

        assert!(!document.config.enabled);
        assert_eq!(backend.legacy("mcp"), None);
    }

    #[test]
    fn failed_local_save_preserves_the_legacy_policy() {
        let legacy = json!({ "version": 1, "enabled": true });
        let backend = Arc::new(MemoryMcpSettingsBackend::with_legacy(legacy.clone()));
        backend.fail_next_local_save();
        let service = McpLocalSettingsService::new_for_test(backend.clone(), None);

        let document = service.load_migrated().unwrap();

        assert!(document.config.enabled);
        assert_eq!(backend.local("mcp"), None);
        assert_eq!(backend.legacy("mcp"), Some(legacy));
    }

    #[test]
    fn failed_legacy_cleanup_is_retried_idempotently() {
        let backend = Arc::new(MemoryMcpSettingsBackend::with_legacy(json!({
            "version": 1,
            "enabled": true
        })));
        backend.fail_next_legacy_save();
        let service = McpLocalSettingsService::new_for_test(backend.clone(), None);

        assert!(service.load_migrated().unwrap().config.enabled);
        assert!(backend.local("mcp").is_some());
        assert!(backend.legacy("mcp").is_some());

        assert!(service.load_migrated().unwrap().config.enabled);
        assert_eq!(backend.legacy("mcp"), None);
    }

    #[test]
    fn malformed_legacy_policy_falls_back_to_the_default() {
        let backend = Arc::new(MemoryMcpSettingsBackend::with_legacy(json!({
            "version": "broken"
        })));
        let service = McpLocalSettingsService::new_for_test(backend.clone(), None);

        let document = service.load_migrated().unwrap();

        assert_eq!(document.config, McpConfig::default());
        assert_eq!(
            backend.local("mcp"),
            serde_json::to_value(McpConfig::default()).ok()
        );
        assert_eq!(backend.legacy("mcp"), None);
    }

    #[test]
    fn local_policy_write_preserves_unrelated_local_state_entries() {
        let backend = Arc::new(MemoryMcpSettingsBackend::with_local_entry(
            "primaryWorkspace",
            json!({ "desktopPath": "/Notes" }),
        ));
        let service = McpLocalSettingsService::new_for_test(backend.clone(), None);
        let initial = service.load_migrated().unwrap();

        let updated = service
            .write(
                &initial.revision,
                McpConfig {
                    enabled: true,
                    ..initial.config
                },
            )
            .unwrap();

        assert!(updated.config.enabled);
        assert_eq!(
            backend.local("primaryWorkspace"),
            Some(json!({ "desktopPath": "/Notes" }))
        );
        assert_eq!(backend.local("schemaVersion"), Some(json!(2)));
    }

    #[test]
    fn failed_policy_write_restores_the_previous_local_values() {
        let backend = Arc::new(MemoryMcpSettingsBackend::default());
        let service = McpLocalSettingsService::new_for_test(backend.clone(), None);
        let initial = service.load_migrated().unwrap();
        let before = backend.local("mcp");
        backend.fail_next_local_save();

        let error = service
            .write(
                &initial.revision,
                McpConfig {
                    enabled: true,
                    ..initial.config
                },
            )
            .unwrap_err();

        assert_eq!(error.code, "mcp-config-write-failed");
        assert_eq!(backend.local("mcp"), before);
    }
}
