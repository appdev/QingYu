//! Settings persistence adapter boundary.

use std::{collections::BTreeMap, fmt, sync::Mutex};

use serde_json::{Map, Value};

use crate::{
    app_config::model::{APP_CONFIG_MAX_BYTES, APP_CONFIG_VERSION, APP_CONFIG_VERSION_KEY},
    settings::model::PORTABLE_SETTINGS_KEYS,
    storage::{
        CommitState, DurableFileFailure, DurableFileFailureKind, DurableFileStore, ExpectedFile,
        FileRevision, PreservePrevious, RecoveryOutcome, ReplaceRequest, StorageFileName,
    },
};

/// Kernel settings persistence boundary.
///
/// Production desktop, server, and mobile hosts provide this through the
/// Kernel durable-file adapter rooted at their configuration capability.
pub trait SettingsStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Value>, SettingsStoreError>;
    fn set(&self, key: &str, value: Value) -> Result<(), SettingsStoreError>;
    fn delete(&self, key: &str) -> Result<(), SettingsStoreError>;
    fn save(&self) -> Result<(), SettingsStoreError>;
    fn replace_portable_atomically(
        &self,
        desired: &Map<String, Value>,
    ) -> Result<(), SettingsStoreError>;
}

/// Kernel settings storage over the durable configuration-file boundary.
pub struct AtomicJsonSettingsStore {
    durable: DurableFileStore,
    target: StorageFileName,
    state: Mutex<AtomicJsonState>,
}

struct AtomicJsonState {
    values: BTreeMap<String, Value>,
    revision: Option<FileRevision>,
    recovery_required: bool,
}

impl AtomicJsonSettingsStore {
    pub fn new(durable: DurableFileStore) -> Result<Self, SettingsStoreError> {
        let recovery = durable.recover().map_err(map_durable_failure)?;
        if recovery
            .iter()
            .any(|outcome| matches!(outcome, RecoveryOutcome::ManualInterventionRequired { .. }))
        {
            return Err(SettingsStoreError::unavailable());
        }
        let target = StorageFileName::parse("settings.json").map_err(map_durable_failure)?;
        let stored = durable
            .read(&target, APP_CONFIG_MAX_BYTES as u64)
            .map_err(map_durable_failure)?;
        let (values, revision) = match stored {
            Some(stored) => {
                let values = serde_json::from_slice::<Value>(&stored.bytes)
                    .ok()
                    .and_then(|value| value.as_object().cloned())
                    .ok_or_else(SettingsStoreError::unavailable)?;
                if values.get(APP_CONFIG_VERSION_KEY) != Some(&Value::from(APP_CONFIG_VERSION)) {
                    return Err(SettingsStoreError::unavailable());
                }
                (values.into_iter().collect(), Some(stored.revision.clone()))
            }
            None => (
                BTreeMap::from([(
                    APP_CONFIG_VERSION_KEY.to_string(),
                    Value::from(APP_CONFIG_VERSION),
                )]),
                None,
            ),
        };
        Ok(Self {
            durable,
            target,
            state: Mutex::new(AtomicJsonState {
                values,
                revision,
                recovery_required: false,
            }),
        })
    }

    fn persist_locked(
        &self,
        state: &mut AtomicJsonState,
        bytes: &[u8],
    ) -> Result<(), SettingsStoreError> {
        if state.recovery_required {
            return Err(SettingsStoreError::unavailable());
        }
        let expected = match state.revision.as_ref() {
            Some(revision) => ExpectedFile::Revision(revision),
            None => ExpectedFile::Absent,
        };
        match self.durable.replace(ReplaceRequest {
            target: &self.target,
            bytes,
            expected,
            preserve_previous: PreservePrevious::None,
        }) {
            Ok(outcome) => {
                state.revision = Some(outcome.installed_revision);
                if outcome.commit_state == CommitState::PublishedDurabilityUncertain {
                    state.recovery_required = true;
                    Err(SettingsStoreError::publish_uncertain())
                } else {
                    Ok(())
                }
            }
            Err(error) if error.kind() == DurableFileFailureKind::PublishStateUncertain => {
                state.recovery_required = true;
                state.revision = None;
                Err(SettingsStoreError::publish_uncertain())
            }
            Err(error) => Err(map_durable_failure(error)),
        }
    }
}

impl SettingsStore for AtomicJsonSettingsStore {
    fn get(&self, key: &str) -> Result<Option<Value>, SettingsStoreError> {
        let state = self
            .state
            .lock()
            .map_err(|_| SettingsStoreError::unavailable())?;
        if state.recovery_required {
            return Err(SettingsStoreError::unavailable());
        }
        Ok(state.values.get(key).cloned())
    }

    fn set(&self, key: &str, value: Value) -> Result<(), SettingsStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SettingsStoreError::unavailable())?;
        if state.recovery_required {
            return Err(SettingsStoreError::unavailable());
        }
        state.values.insert(key.to_string(), value);
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SettingsStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SettingsStoreError::unavailable())?;
        if state.recovery_required {
            return Err(SettingsStoreError::unavailable());
        }
        state.values.remove(key);
        Ok(())
    }

    fn save(&self) -> Result<(), SettingsStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SettingsStoreError::unavailable())?;
        state.values.insert(
            APP_CONFIG_VERSION_KEY.to_string(),
            Value::from(APP_CONFIG_VERSION),
        );
        let bytes =
            serde_json::to_vec(&state.values).map_err(|_| SettingsStoreError::unavailable())?;
        self.persist_locked(&mut state, &bytes)
    }

    fn replace_portable_atomically(
        &self,
        desired: &Map<String, Value>,
    ) -> Result<(), SettingsStoreError> {
        if desired
            .keys()
            .any(|key| !PORTABLE_SETTINGS_KEYS.contains(&key.as_str()))
        {
            return Err(SettingsStoreError::unavailable());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| SettingsStoreError::unavailable())?;
        let mut next = state.values.clone();
        for key in PORTABLE_SETTINGS_KEYS {
            next.remove(key);
        }
        next.extend(
            desired
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        next.insert(
            APP_CONFIG_VERSION_KEY.to_string(),
            Value::from(APP_CONFIG_VERSION),
        );
        let bytes = serde_json::to_vec(&next).map_err(|_| SettingsStoreError::unavailable())?;
        let result = self.persist_locked(&mut state, &bytes);
        if result.is_ok()
            || result
                .as_ref()
                .is_err_and(|error| error.kind() == SettingsStoreErrorKind::PublishUncertain)
        {
            state.values = next;
        }
        result
    }
}

impl fmt::Debug for AtomicJsonSettingsStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AtomicJsonSettingsStore(..)")
    }
}

fn map_durable_failure(error: DurableFileFailure) -> SettingsStoreError {
    if error.kind() == DurableFileFailureKind::PublishStateUncertain {
        SettingsStoreError::publish_uncertain()
    } else {
        SettingsStoreError::unavailable()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsStoreErrorKind {
    Unavailable,
    PublishUncertain,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SettingsStoreError {
    kind: SettingsStoreErrorKind,
}

impl SettingsStoreError {
    pub const fn unavailable() -> Self {
        Self {
            kind: SettingsStoreErrorKind::Unavailable,
        }
    }

    pub const fn publish_uncertain() -> Self {
        Self {
            kind: SettingsStoreErrorKind::PublishUncertain,
        }
    }

    pub const fn kind(self) -> SettingsStoreErrorKind {
        self.kind
    }
}

impl fmt::Debug for SettingsStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsStoreError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for SettingsStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("settings storage is unavailable")
    }
}

impl std::error::Error for SettingsStoreError {}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::{config::KernelConfig, paths::KernelPaths};

    #[test]
    fn atomic_store_reads_fail_closed_while_recovery_is_required() {
        let root = tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let app_data = root.path().join("app-data");
        let cache = root.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let config = KernelConfig::generate().unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let durable =
            DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
        let store = AtomicJsonSettingsStore::new(durable).unwrap();
        let mut state = store.state.lock().unwrap();
        state
            .values
            .insert("language".to_string(), serde_json::json!("fr"));
        state.recovery_required = true;
        drop(state);

        let error = store.get("language").unwrap_err();

        assert_eq!(error.kind(), SettingsStoreErrorKind::Unavailable);
    }
}
