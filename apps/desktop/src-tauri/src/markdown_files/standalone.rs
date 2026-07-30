use std::{
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::trusted_file::{
    replace_document_atomic, stage_document_contents, sync_directory, trusted_parent,
};

const STANDALONE_CONFLICT: &str = "standalone-document-conflict";
const STANDALONE_UNAVAILABLE: &str = "standalone-document-unavailable";
const REVISION_PREFIX: &str = "native-v2-";

static STANDALONE_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StandaloneDocumentSnapshot {
    pub(crate) contents: String,
    pub(crate) display_name: String,
    pub(crate) revision: String,
}

fn unavailable() -> String {
    STANDALONE_UNAVAILABLE.to_string()
}

fn conflict() -> String {
    STANDALONE_CONFLICT.to_string()
}

#[cfg(unix)]
fn file_identity(file: &std::fs::File) -> Result<Vec<u8>, String> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata().map_err(|_| unavailable())?;
    Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()).into_bytes())
}

#[cfg(windows)]
fn file_identity(file: &std::fs::File) -> Result<Vec<u8>, String> {
    use std::{mem, os::windows::io::AsRawHandle};
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
    };

    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { mem::zeroed() };
    let result =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut information) };
    if result == 0 {
        return Err(unavailable());
    }
    let file_index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Ok(format!("windows:{}:{file_index}", information.dwVolumeSerialNumber).into_bytes())
}

#[cfg(not(any(unix, windows)))]
fn file_identity(_file: &std::fs::File) -> Result<Vec<u8>, String> {
    Err(unavailable())
}

fn revision_for(identity: &[u8], contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"qingyu-native-standalone-v2\0");
    hasher.update((identity.len() as u64).to_be_bytes());
    hasher.update(identity);
    hasher.update(contents);
    let digest = hasher.finalize();
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{REVISION_PREFIX}{encoded}")
}

fn read_from_directory(
    directory: &Dir,
    name: &OsStr,
) -> Result<StandaloneDocumentSnapshot, String> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory
        .open_with(Path::new(name), &options)
        .map_err(|_| unavailable())?
        .into_std();
    let metadata = file.metadata().map_err(|_| unavailable())?;
    if !metadata.is_file() {
        return Err(unavailable());
    }
    let identity = file_identity(&file)?;
    let mut contents = String::new();
    (&file)
        .read_to_string(&mut contents)
        .map_err(|_| unavailable())?;
    let revision = revision_for(&identity, contents.as_bytes());
    Ok(StandaloneDocumentSnapshot {
        contents,
        display_name: name.to_string_lossy().to_string(),
        revision,
    })
}

#[tauri::command]
pub(crate) fn read_standalone_document(path: String) -> Result<StandaloneDocumentSnapshot, String> {
    let path = PathBuf::from(path);
    let (directory, _parent, name) = trusted_parent(&path).map_err(|_| unavailable())?;
    read_from_directory(&directory, &name)
}

fn cleanup_staging(directory: &Dir, staging_name: &str) {
    let _cleanup_result = directory.remove_file(staging_name);
}

#[tauri::command]
pub(crate) fn write_standalone_document_cas(
    path: String,
    expected_revision: String,
    contents: String,
) -> Result<StandaloneDocumentSnapshot, String> {
    // This serializes cooperative CAS writers in this application. An external process that does
    // not share this lock can still race in the final OS scheduling window after revalidation and
    // before the atomic replacement; the command deliberately does not claim an inter-process lock.
    let write_lock = STANDALONE_WRITE_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = write_lock.lock().map_err(|_| unavailable())?;
    let path = PathBuf::from(path);
    let (directory, parent, name) = trusted_parent(&path).map_err(|_| unavailable())?;
    let staging_name =
        stage_document_contents(&directory, contents.as_bytes()).map_err(|_| unavailable())?;

    let current = match read_from_directory(&directory, &name) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            cleanup_staging(&directory, &staging_name);
            return Err(error);
        }
    };
    if current.revision != expected_revision {
        cleanup_staging(&directory, &staging_name);
        return Err(conflict());
    }

    let staging_path = parent.join(&staging_name);
    if replace_document_atomic(&directory, &staging_name, &name, &staging_path, &path).is_err() {
        cleanup_staging(&directory, &staging_name);
        return Err(unavailable());
    }
    let _sync_result = sync_directory(&directory);
    read_from_directory(&directory, &name)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use tempfile::tempdir;

    use super::{read_standalone_document, write_standalone_document_cas};

    #[test]
    fn read_and_cas_write_share_one_opaque_sha256_revision_contract() {
        let directory = tempdir().expect("fixture directory should be created");
        let note = directory.path().join("private.md");
        std::fs::write(&note, "initial").expect("fixture should be written");
        let path = note.to_string_lossy().to_string();

        let initial = read_standalone_document(path.clone()).expect("fixture should be readable");
        assert_eq!(initial.contents, "initial");
        assert_eq!(initial.display_name, "private.md");
        assert!(initial.revision.starts_with("native-v2-"));
        assert_eq!(initial.revision.len(), "native-v2-".len() + 64);
        assert!(!initial.revision.contains("initial"));
        assert!(!initial.revision.contains(&path));

        let saved = write_standalone_document_cas(
            path.clone(),
            initial.revision.clone(),
            "saved".to_string(),
        )
        .expect("matching revision should save");
        assert_eq!(saved.contents, "saved");
        assert_ne!(saved.revision, initial.revision);
        assert_eq!(
            read_standalone_document(path)
                .expect("saved fixture should be readable")
                .revision,
            saved.revision
        );
    }

    #[test]
    fn cas_rejects_a_stale_content_revision_without_disclosing_path_or_contents() {
        let directory = tempdir().expect("fixture directory should be created");
        let note = directory.path().join("secret.md");
        std::fs::write(&note, "initial-secret").expect("fixture should be written");
        let path = note.to_string_lossy().to_string();
        let initial = read_standalone_document(path.clone()).expect("fixture should be readable");
        std::fs::write(&note, "external-secret").expect("external update should be written");

        let error = write_standalone_document_cas(
            path.clone(),
            initial.revision,
            "local-secret".to_string(),
        )
        .expect_err("stale revision must conflict");

        assert_eq!(error, "standalone-document-conflict");
        assert!(!error.contains(&path));
        assert!(!error.contains("external-secret"));
        assert!(!error.contains("local-secret"));
        assert_eq!(
            std::fs::read_to_string(note).expect("fixture should remain readable"),
            "external-secret"
        );
    }

    #[test]
    fn cas_rejects_replaced_file_identity_even_when_contents_are_identical() {
        let directory = tempdir().expect("fixture directory should be created");
        let note = directory.path().join("identity.md");
        let replacement = directory.path().join("replacement.md");
        std::fs::write(&note, "identical").expect("fixture should be written");
        std::fs::write(&replacement, "identical").expect("replacement should be written");
        let path = note.to_string_lossy().to_string();
        let initial = read_standalone_document(path.clone()).expect("fixture should be readable");
        std::fs::rename(&replacement, &note).expect("fixture identity should be replaced");

        assert_eq!(
            write_standalone_document_cas(path, initial.revision, "must-not-save".to_string(),)
                .expect_err("replaced identity must conflict"),
            "standalone-document-conflict"
        );
        assert_eq!(
            std::fs::read_to_string(note).expect("fixture should remain readable"),
            "identical"
        );
    }

    #[test]
    fn one_of_two_application_cas_writers_with_the_same_revision_conflicts() {
        let directory = tempdir().expect("fixture directory should be created");
        let note = directory.path().join("concurrent.md");
        std::fs::write(&note, "initial").expect("fixture should be written");
        let path = note.to_string_lossy().to_string();
        let initial = read_standalone_document(path.clone()).expect("fixture should be readable");
        let barrier = Arc::new(Barrier::new(3));

        let writers = ["first", "second"].map(|contents| {
            let barrier = Arc::clone(&barrier);
            let path = path.clone();
            let revision = initial.revision.clone();
            std::thread::spawn(move || {
                barrier.wait();
                write_standalone_document_cas(path, revision, contents.to_string())
            })
        });
        barrier.wait();
        let outcomes = writers.map(|writer| writer.join().expect("writer should not panic"));

        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.as_ref().err())
                .collect::<Vec<_>>(),
            vec![&"standalone-document-conflict".to_string()]
        );
        assert!(matches!(
            std::fs::read_to_string(note)
                .expect("fixture should remain readable")
                .as_str(),
            "first" | "second"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn no_follow_read_and_write_reject_a_symlink_target_generically() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("fixture directory should be created");
        let target = directory.path().join("target.md");
        let link = directory.path().join("link.md");
        std::fs::write(&target, "target-secret").expect("target should be written");
        symlink(&target, &link).expect("symlink should be created");
        let path = link.to_string_lossy().to_string();

        assert_eq!(
            read_standalone_document(path.clone()).expect_err("symlink read must fail"),
            "standalone-document-unavailable"
        );
        assert_eq!(
            write_standalone_document_cas(
                path,
                "native-v2-invalid".to_string(),
                "must-not-save".to_string(),
            )
            .expect_err("symlink write must fail"),
            "standalone-document-unavailable"
        );
        assert_eq!(
            std::fs::read_to_string(target).expect("target should remain readable"),
            "target-secret"
        );
    }
}
