use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::scope::RemoteSyncScope;
use crate::app_settings::SettingsPublicationEvent;
use crate::storage_capability::{
    create_private_file_options, nonfollowing_read_options, open_canonical_directory_nofollow,
    rename_in_directory, sync_directory, unique_regular_file_identity, UniqueRegularFileIdentity,
};

const SETTINGS_FILE_NAME: &str = "settings.json";
const SETTINGS_FILE_MAX_BYTES: u64 = 16 * 1024 * 1024;
const PENDING_FILE_NAME: &str = "portable-settings-pending.json";
const PENDING_FILE_MAX_BYTES: u64 = 48 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SettingsFileState {
    bytes: Option<Vec<u8>>,
    hash: Option<String>,
    identity: Option<(u64, u64, u64)>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PortableSettingsJournalPhase {
    Prepared,
    Reconcile,
    Publication,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct PortableSettingsJournal {
    pub(crate) expected_portable_revision: String,
    pub(crate) prepared_manifest_revision: Option<String>,
    pub(crate) applied_portable_revision: Option<String>,
    pub(crate) phase: PortableSettingsJournalPhase,
    staged_base64: Option<String>,
    pub(crate) expected_local_hash: Option<String>,
    pub(crate) publication_events: Vec<SettingsPublicationEvent>,
}

impl PortableSettingsJournal {
    pub(crate) fn prepared(revision: impl Into<String>, staged: Option<&[u8]>) -> Self {
        Self {
            expected_portable_revision: revision.into(),
            prepared_manifest_revision: None,
            applied_portable_revision: None,
            phase: PortableSettingsJournalPhase::Prepared,
            staged_base64: staged.map(|bytes| STANDARD_NO_PAD.encode(bytes)),
            expected_local_hash: None,
            publication_events: Vec::new(),
        }
    }

    pub(crate) fn staged_bytes(&self) -> Result<Option<Vec<u8>>, String> {
        let bytes = self.decoded_staged_bytes()?;
        if let Some(bytes) = &bytes {
            if bytes.len() as u64 > SETTINGS_FILE_MAX_BYTES
                || crate::app_settings::validate_portable_settings_bytes(bytes).is_err()
            {
                return Err("settings-state-invalid: The settings state is invalid.".to_string());
            }
        }
        Ok(bytes)
    }

    fn decoded_staged_bytes(&self) -> Result<Option<Vec<u8>>, String> {
        self.staged_base64
            .as_deref()
            .map(|encoded| {
                STANDARD_NO_PAD.decode(encoded).map_err(|_| {
                    "settings-state-invalid: The settings state is invalid.".to_string()
                })
            })
            .transpose()
    }

    pub(crate) fn set_staged_bytes(&mut self, staged: Option<&[u8]>) {
        self.staged_base64 = staged.map(|bytes| STANDARD_NO_PAD.encode(bytes));
    }
}

pub(crate) fn capture_portable_settings_manifest_revision(
    scope: &RemoteSyncScope,
) -> Result<Option<String>, String> {
    let state = scope.open_state_root()?;
    let metadata = match state.symlink_metadata(scope.manifest_name()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(
                "settings-state-unavailable: The settings state is unavailable.".to_string(),
            )
        }
    };
    let identity = unique_regular_file_identity(&metadata)
        .filter(|identity| identity.revision_parts().2 <= SETTINGS_FILE_MAX_BYTES)
        .ok_or_else(|| "settings-state-unsafe: The settings state is unsafe.".to_string())?;
    let file = state
        .open_with(scope.manifest_name(), &nonfollowing_read_options())
        .map_err(|_| "settings-state-unsafe: The settings state is unsafe.".to_string())?;
    let retained = file
        .metadata()
        .map_err(|_| "settings-state-unsafe: The settings state is unsafe.".to_string())?;
    if !identity.matches_retained_regular_file(&retained, false) {
        return Err("settings-state-unsafe: The settings state is unsafe.".to_string());
    }
    let mut bytes = Vec::new();
    file.take(SETTINGS_FILE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            "settings-state-unavailable: The settings state is unavailable.".to_string()
        })?;
    if bytes.len() as u64 != identity.revision_parts().2 {
        return Err("settings-state-unsafe: The settings state is unsafe.".to_string());
    }
    Ok(Some(format!("sha256:{:x}", Sha256::digest(bytes))))
}

impl SettingsFileState {
    #[cfg(test)]
    pub(crate) fn is_missing(&self) -> bool {
        self.bytes.is_none()
    }

    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }

    pub(crate) fn matches_hash(&self, expected: Option<&str>) -> bool {
        self.hash.as_deref() == expected
    }

    #[cfg(test)]
    pub(crate) fn hash(&self) -> Option<&str> {
        self.hash.as_deref()
    }
}

pub(crate) fn capture_settings_file_state(app_data: &Path) -> Result<SettingsFileState, String> {
    let directory = open_canonical_directory_nofollow(app_data).map_err(|_| {
        "settings-state-unavailable: The settings state is unavailable.".to_string()
    })?;
    let addressed = match directory.symlink_metadata(SETTINGS_FILE_NAME) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(SettingsFileState {
                bytes: None,
                hash: None,
                identity: None,
            })
        }
        Err(_) => {
            return Err(
                "settings-state-unavailable: The settings state is unavailable.".to_string(),
            )
        }
    };
    let identity = unique_regular_file_identity(&addressed)
        .filter(|identity| identity.revision_parts().2 <= SETTINGS_FILE_MAX_BYTES)
        .ok_or_else(|| "settings-state-unsafe: The settings file is unsafe.".to_string())?;
    let mut file = directory
        .open_with(SETTINGS_FILE_NAME, &nonfollowing_read_options())
        .map_err(|_| "settings-state-unsafe: The settings file is unsafe.".to_string())?;
    let retained = file
        .metadata()
        .map_err(|_| "settings-state-unsafe: The settings file is unsafe.".to_string())?;
    if !identity.matches_retained_regular_file(&retained, false) {
        return Err("settings-state-unsafe: The settings file is unsafe.".to_string());
    }
    let mut bytes = Vec::with_capacity(retained.len() as usize);
    file.read_to_end(&mut bytes).map_err(|_| {
        "settings-state-unavailable: The settings state is unavailable.".to_string()
    })?;
    let final_metadata = file
        .metadata()
        .map_err(|_| "settings-state-unsafe: The settings file is unsafe.".to_string())?;
    if !identity.matches_retained_regular_file(&final_metadata, false)
        || bytes.len() as u64 != identity.revision_parts().2
    {
        return Err("settings-state-unsafe: The settings file is unsafe.".to_string());
    }
    let hash = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(SettingsFileState {
        bytes: Some(bytes),
        hash: Some(hash),
        identity: Some(identity.revision_parts()),
    })
}

pub(crate) fn replace_portable_settings_stage(
    scope: &RemoteSyncScope,
    bytes: Option<&[u8]>,
) -> Result<(), String> {
    replace_control_file(scope, SETTINGS_FILE_NAME, bytes)
}

pub(crate) fn read_portable_settings_pending(
    scope: &RemoteSyncScope,
) -> Result<Option<PortableSettingsJournal>, String> {
    let journal = read_portable_settings_pending_raw(scope)?;
    if let Some(journal) = &journal {
        journal.staged_bytes()?;
    }
    Ok(journal)
}

pub(crate) fn portable_settings_pending_contains_legacy_mcp(
    scope: &RemoteSyncScope,
) -> Result<bool, String> {
    let Some(journal) = read_portable_settings_pending_raw(scope)? else {
        return Ok(false);
    };
    if journal
        .publication_events
        .iter()
        .any(|event| event.event_name() == "qingyu://settings-mcp-changed")
    {
        return Ok(true);
    }
    let Some(bytes) = journal.decoded_staged_bytes()? else {
        return Ok(false);
    };
    if bytes.len() as u64 > SETTINGS_FILE_MAX_BYTES {
        return Ok(false);
    }
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(false);
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(false);
    };
    if object.remove("mcp").is_none() {
        return Ok(false);
    }
    let sanitized = serde_json::to_vec(&value)
        .map_err(|_| "settings-state-invalid: The settings state is invalid.".to_string())?;
    Ok(crate::app_settings::validate_portable_settings_bytes(&sanitized).is_ok())
}

fn read_portable_settings_pending_raw(
    scope: &RemoteSyncScope,
) -> Result<Option<PortableSettingsJournal>, String> {
    let directory = scope.open_source_root()?;
    let metadata = match directory.symlink_metadata(PENDING_FILE_NAME) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(
                "settings-state-unavailable: The settings state is unavailable.".to_string(),
            )
        }
    };
    let identity = unique_regular_file_identity(&metadata)
        .filter(|identity| identity.revision_parts().2 <= PENDING_FILE_MAX_BYTES)
        .ok_or_else(|| "settings-state-unsafe: The settings state is unsafe.".to_string())?;
    let file = directory
        .open_with(PENDING_FILE_NAME, &nonfollowing_read_options())
        .map_err(|_| "settings-state-unsafe: The settings state is unsafe.".to_string())?;
    let retained = file
        .metadata()
        .map_err(|_| "settings-state-unsafe: The settings state is unsafe.".to_string())?;
    if !identity.matches_retained_regular_file(&retained, false) {
        return Err("settings-state-unsafe: The settings state is unsafe.".to_string());
    }
    let mut bytes = Vec::new();
    file.take(PENDING_FILE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            "settings-state-unavailable: The settings state is unavailable.".to_string()
        })?;
    if bytes.len() as u64 != identity.revision_parts().2 {
        return Err("settings-state-unsafe: The settings state is unsafe.".to_string());
    }
    let journal = serde_json::from_slice::<PortableSettingsJournal>(&bytes)
        .map_err(|_| "settings-state-invalid: The settings state is invalid.".to_string())?;
    Ok(Some(journal))
}

pub(crate) fn write_portable_settings_pending(
    scope: &RemoteSyncScope,
    revision: &PortableSettingsJournal,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(revision).map_err(|_| {
        "settings-state-unavailable: The settings state is unavailable.".to_string()
    })?;
    replace_control_file(scope, PENDING_FILE_NAME, Some(&bytes))
}

pub(crate) fn clear_portable_settings_pending(scope: &RemoteSyncScope) -> Result<(), String> {
    replace_control_file(scope, PENDING_FILE_NAME, None)
}

pub(crate) fn clear_portable_settings_manifest(scope: &RemoteSyncScope) -> Result<(), String> {
    let directory = scope.open_state_root()?;
    let existing = capture_control_file_identity(&directory, scope.manifest_name())?;
    if existing.is_none() {
        return Ok(());
    }
    scope.open_state_root()?;
    if capture_control_file_identity(&directory, scope.manifest_name())? != existing {
        return Err("settings-state-changed: The settings state changed.".to_string());
    }
    directory.remove_file(scope.manifest_name()).map_err(|_| {
        "settings-state-unavailable: The settings state is unavailable.".to_string()
    })?;
    sync_directory(&directory)
        .map_err(|_| "settings-state-unavailable: The settings state is unavailable.".to_string())
}

fn replace_control_file(
    scope: &RemoteSyncScope,
    file_name: &str,
    bytes: Option<&[u8]>,
) -> Result<(), String> {
    let directory = scope.open_source_root()?;
    let existing = capture_control_file_identity(&directory, file_name)?;
    let Some(bytes) = bytes else {
        if existing.is_some() {
            scope.open_source_root()?;
            if capture_control_file_identity(&directory, file_name)? != existing {
                return Err("settings-state-changed: The settings state changed.".to_string());
            }
            directory.remove_file(file_name).map_err(|_| {
                "settings-state-unavailable: The settings state is unavailable.".to_string()
            })?;
            sync_directory(&directory).map_err(|_| {
                "settings-state-unavailable: The settings state is unavailable.".to_string()
            })?;
        }
        return Ok(());
    };
    let (staged_name, mut staged) = create_control_staging_file(&directory)?;
    if staged
        .write_all(bytes)
        .and_then(|()| staged.sync_all())
        .is_err()
    {
        drop(staged);
        let _cleanup = directory.remove_file(&staged_name);
        return Err("settings-state-unavailable: The settings state is unavailable.".to_string());
    }
    drop(staged);
    scope.open_source_root()?;
    if capture_control_file_identity(&directory, file_name)? != existing {
        let _cleanup = directory.remove_file(&staged_name);
        return Err("settings-state-changed: The settings state changed.".to_string());
    }
    if let Err(error) = rename_in_directory(&directory, &staged_name, file_name, existing.is_some())
    {
        let _cleanup = directory.remove_file(&staged_name);
        return Err(error.to_string());
    }
    sync_directory(&directory)
        .map_err(|_| "settings-state-unavailable: The settings state is unavailable.".to_string())
}

fn capture_control_file_identity(
    directory: &cap_std::fs::Dir,
    file_name: &str,
) -> Result<Option<UniqueRegularFileIdentity>, String> {
    match directory.symlink_metadata(file_name) {
        Ok(metadata) => unique_regular_file_identity(&metadata)
            .map(Some)
            .ok_or_else(|| "settings-state-unsafe: The settings state is unsafe.".to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("settings-state-unavailable: The settings state is unavailable.".to_string()),
    }
}

fn create_control_staging_file(
    directory: &cap_std::fs::Dir,
) -> Result<(String, cap_std::fs::File), String> {
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..1000 {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".portable-settings-{}-{sequence}.tmp", std::process::id());
        match directory.open_with(&name, &create_private_file_options()) {
            Ok(file) => return Ok((name, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => break,
        }
    }
    Err("settings-state-unavailable: The settings state is unavailable.".to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        capture_settings_file_state, clear_portable_settings_manifest,
        portable_settings_pending_contains_legacy_mcp, read_portable_settings_pending,
        write_portable_settings_pending, PortableSettingsJournal,
    };
    use crate::app_settings::SettingsPublicationEvent;
    use crate::remote_sync::scope::RemoteSyncScope;

    #[test]
    fn settings_file_state_detects_content_changes_and_deletion() {
        let app_data = tempdir().unwrap();
        let path = app_data.path().join("settings.json");
        fs::write(&path, br#"{"appearanceMode":"light"}"#).unwrap();
        let first = capture_settings_file_state(app_data.path()).unwrap();

        fs::write(&path, br#"{"appearanceMode":"dark"}"#).unwrap();
        let second = capture_settings_file_state(app_data.path()).unwrap();
        assert_ne!(first, second);

        fs::remove_file(path).unwrap();
        let missing = capture_settings_file_state(app_data.path()).unwrap();
        assert_ne!(second, missing);
        assert!(missing.is_missing());
    }

    #[test]
    fn settings_file_state_detects_same_content_file_replacement() {
        let app_data = tempdir().unwrap();
        let path = app_data.path().join("settings.json");
        fs::write(&path, br#"{"appearanceMode":"light"}"#).unwrap();
        let first = capture_settings_file_state(app_data.path()).unwrap();

        let replacement = app_data.path().join("replacement.json");
        fs::write(&replacement, br#"{"appearanceMode":"light"}"#).unwrap();
        fs::rename(replacement, path).unwrap();
        let second = capture_settings_file_state(app_data.path()).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn pending_journal_round_trips_the_exact_staged_settings_bytes() {
        let app_data = tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let scope = RemoteSyncScope::portable_settings(
            &app_data_root,
            app_data_root.join("sync-state/settings-journal"),
            "manifest.json",
        )
        .unwrap();
        let staged = b"{\n  \"language\": \"zh-CN\"\n}\n";
        let journal = PortableSettingsJournal::prepared("portable-revision", Some(staged));

        write_portable_settings_pending(&scope, &journal).unwrap();
        let restored = read_portable_settings_pending(&scope).unwrap().unwrap();

        assert_eq!(
            restored.staged_bytes().unwrap().as_deref(),
            Some(staged.as_slice())
        );
    }

    #[test]
    fn pending_journal_detects_the_legacy_mcp_schema() {
        let app_data = tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let scope = RemoteSyncScope::portable_settings(
            &app_data_root,
            app_data_root.join("sync-state/settings-journal-legacy-mcp"),
            "manifest.json",
        )
        .unwrap();
        let staged = br#"{"language":"zh-CN","mcp":{"enabled":true}}"#;
        let journal = PortableSettingsJournal::prepared("legacy-revision", Some(staged));

        write_portable_settings_pending(&scope, &journal).unwrap();

        assert!(portable_settings_pending_contains_legacy_mcp(&scope).unwrap());
    }

    #[test]
    fn invalid_non_mcp_pending_settings_are_not_classified_as_legacy_mcp() {
        let app_data = tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let scope = RemoteSyncScope::portable_settings(
            &app_data_root,
            app_data_root.join("sync-state/settings-journal-invalid"),
            "manifest.json",
        )
        .unwrap();
        let staged = br#"{"language":7,"mcp":{"enabled":true}}"#;
        let journal = PortableSettingsJournal::prepared("legacy-revision", Some(staged));

        write_portable_settings_pending(&scope, &journal).unwrap();

        assert!(!portable_settings_pending_contains_legacy_mcp(&scope).unwrap());
        assert!(read_portable_settings_pending(&scope).is_err());
    }

    #[test]
    fn pending_mcp_publication_event_marks_an_old_schema_journal() {
        let app_data = tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let scope = RemoteSyncScope::portable_settings(
            &app_data_root,
            app_data_root.join("sync-state/settings-journal-mcp-event"),
            "manifest.json",
        )
        .unwrap();
        let mut journal =
            PortableSettingsJournal::prepared("legacy-revision", Some(br#"{"language":"zh-CN"}"#));
        journal.publication_events = vec![SettingsPublicationEvent::new(
            "qingyu://settings-mcp-changed",
            serde_json::json!({ "config": { "enabled": true } }),
        )];

        write_portable_settings_pending(&scope, &journal).unwrap();

        assert!(portable_settings_pending_contains_legacy_mcp(&scope).unwrap());
    }

    #[test]
    fn clearing_the_portable_manifest_preserves_sibling_note_state() {
        let app_data = tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let settings_state = app_data_root.join("sync-state/settings");
        let notes_state = app_data_root.join("sync-state/notes");
        fs::create_dir_all(&settings_state).unwrap();
        fs::create_dir_all(&notes_state).unwrap();
        fs::write(notes_state.join("manifest.json"), b"notes-manifest").unwrap();
        let scope = RemoteSyncScope::portable_settings(
            &app_data_root,
            settings_state.clone(),
            "manifest.json",
        )
        .unwrap();
        fs::write(
            settings_state.join("engine/manifest.json"),
            b"settings-manifest",
        )
        .unwrap();

        clear_portable_settings_manifest(&scope).unwrap();

        assert!(!settings_state.join("engine/manifest.json").exists());
        assert_eq!(
            fs::read(notes_state.join("manifest.json")).unwrap(),
            b"notes-manifest"
        );
    }

    #[cfg(unix)]
    #[test]
    fn settings_file_state_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let app_data = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("settings.json"), b"{}").unwrap();
        symlink(
            outside.path().join("settings.json"),
            app_data.path().join("settings.json"),
        )
        .unwrap();

        assert!(capture_settings_file_state(app_data.path()).is_err());
    }
}
