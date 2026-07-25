use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};

use cap_std::fs::Dir;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::RepoError;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct DirectoryIdentity {
    volume: u64,
    file: u64,
}

static REPOSITORY_LIFECYCLES: OnceLock<StdMutex<HashMap<DirectoryIdentity, Weak<Mutex<()>>>>> =
    OnceLock::new();

#[derive(Clone)]
pub(crate) struct LifecycleGate(Arc<Mutex<()>>);

impl LifecycleGate {
    pub(crate) fn for_directory(directory: &Dir) -> Result<Self, RepoError> {
        let identity = directory_identity(directory)?;
        let gates = REPOSITORY_LIFECYCLES.get_or_init(|| StdMutex::new(HashMap::new()));
        let mut gates = gates
            .lock()
            .map_err(|_| RepoError::InvalidData("repository lifecycle registry poisoned"))?;
        gates.retain(|_, gate| gate.strong_count() > 0);
        let gate = gates
            .get(&identity)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let gate = Arc::new(Mutex::new(()));
                gates.insert(identity, Arc::downgrade(&gate));
                gate
            });
        Ok(Self(gate))
    }

    pub(crate) async fn acquire(&self) -> OwnedMutexGuard<()> {
        Arc::clone(&self.0).lock_owned().await
    }

    pub(crate) fn try_acquire(&self) -> Result<OwnedMutexGuard<()>, RepoError> {
        Arc::clone(&self.0)
            .try_lock_owned()
            .map_err(|_| RepoError::RepositoryBusy)
    }
}

#[cfg(unix)]
fn directory_identity(directory: &Dir) -> Result<DirectoryIdentity, RepoError> {
    use cap_std::fs::MetadataExt;

    let metadata = directory.dir_metadata()?;
    Ok(DirectoryIdentity {
        volume: metadata.dev(),
        file: metadata.ino(),
    })
}

#[cfg(windows)]
fn directory_identity(directory: &Dir) -> Result<DirectoryIdentity, RepoError> {
    use cap_std::fs::MetadataExt;

    let metadata = directory.dir_metadata()?;
    let volume = metadata
        .volume_serial_number()
        .ok_or(RepoError::InvalidData(
            "repository data directory volume identity is unavailable",
        ))?;
    let file = metadata.file_index().ok_or(RepoError::InvalidData(
        "repository data directory file identity is unavailable",
    ))?;
    Ok(DirectoryIdentity {
        volume: u64::from(volume),
        file,
    })
}

#[cfg(not(any(unix, windows)))]
fn directory_identity(_directory: &Dir) -> Result<DirectoryIdentity, RepoError> {
    Err(RepoError::InvalidData(
        "repository data directory identity is unavailable on this platform",
    ))
}
