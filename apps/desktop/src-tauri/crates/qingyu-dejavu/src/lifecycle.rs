use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};

use cap_std::fs::Dir;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::RepoError;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct DirectoryIdentity {
    volume: u64,
    file: u64,
}

static REPOSITORY_LIFECYCLES: OnceLock<StdMutex<HashMap<DirectoryIdentity, Weak<Mutex<()>>>>> =
    OnceLock::new();

#[derive(Clone)]
pub(crate) struct LifecycleGate {
    identity: DirectoryIdentity,
    mutex: Arc<Mutex<()>>,
}

pub(crate) struct LifecyclePermits {
    _guards: Vec<OwnedMutexGuard<()>>,
}

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
        Ok(Self {
            identity,
            mutex: gate,
        })
    }

    pub(crate) async fn acquire(&self) -> OwnedMutexGuard<()> {
        Arc::clone(&self.mutex).lock_owned().await
    }

    pub(crate) fn try_acquire(&self) -> Result<OwnedMutexGuard<()>, RepoError> {
        Arc::clone(&self.mutex)
            .try_lock_owned()
            .map_err(|_| RepoError::RepositoryBusy)
    }

    pub(crate) async fn acquire_pair(left: &Self, right: &Self) -> LifecyclePermits {
        let (first, second) = ordered_pair(left, right);
        let first_guard = first.acquire().await;
        let mut guards = vec![first_guard];
        if let Some(second) = second {
            guards.push(second.acquire().await);
        }
        LifecyclePermits { _guards: guards }
    }

    pub(crate) fn try_acquire_pair(
        left: &Self,
        right: &Self,
    ) -> Result<LifecyclePermits, RepoError> {
        let (first, second) = ordered_pair(left, right);
        let first_guard = first.try_acquire()?;
        let mut guards = vec![first_guard];
        if let Some(second) = second {
            guards.push(second.try_acquire()?);
        }
        Ok(LifecyclePermits { _guards: guards })
    }
}

fn ordered_pair<'gate>(
    left: &'gate LifecycleGate,
    right: &'gate LifecycleGate,
) -> (&'gate LifecycleGate, Option<&'gate LifecycleGate>) {
    match left.identity.cmp(&right.identity) {
        std::cmp::Ordering::Less => (left, Some(right)),
        std::cmp::Ordering::Equal => (left, None),
        std::cmp::Ordering::Greater => (right, Some(left)),
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
    use cap_fs_ext::MetadataExt;

    let metadata = directory.dir_metadata()?;
    Ok(DirectoryIdentity {
        volume: metadata.dev(),
        file: metadata.ino(),
    })
}

#[cfg(not(any(unix, windows)))]
fn directory_identity(_directory: &Dir) -> Result<DirectoryIdentity, RepoError> {
    Err(RepoError::InvalidData(
        "repository data directory identity is unavailable on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use cap_std::ambient_authority;
    use cap_std::fs::Dir;
    use tokio::sync::Barrier;

    use super::LifecycleGate;

    #[tokio::test]
    async fn crossed_scope_pairs_acquire_in_identity_order_without_deadlock() {
        let temp = tempfile::tempdir().unwrap();
        let first_path = temp.path().join("first");
        let second_path = temp.path().join("second");
        fs::create_dir(&first_path).unwrap();
        fs::create_dir(&second_path).unwrap();
        let first = LifecycleGate::for_directory(
            &Dir::open_ambient_dir(first_path, ambient_authority()).unwrap(),
        )
        .unwrap();
        let second = LifecycleGate::for_directory(
            &Dir::open_ambient_dir(second_path, ambient_authority()).unwrap(),
        )
        .unwrap();
        let start = Arc::new(Barrier::new(3));

        let left = tokio::spawn({
            let first = first.clone();
            let second = second.clone();
            let start = Arc::clone(&start);
            async move {
                start.wait().await;
                LifecycleGate::acquire_pair(&first, &second).await
            }
        });
        let right = tokio::spawn({
            let first = first.clone();
            let second = second.clone();
            let start = Arc::clone(&start);
            async move {
                start.wait().await;
                LifecycleGate::acquire_pair(&second, &first).await
            }
        });
        start.wait().await;

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::pin!(left);
            tokio::pin!(right);
            tokio::select! {
                first = &mut left => {
                    drop(first.unwrap());
                    drop(right.await.unwrap());
                }
                first = &mut right => {
                    drop(first.unwrap());
                    drop(left.await.unwrap());
                }
            }
        })
        .await
        .expect("crossed lifecycle scope acquisition must not deadlock");
    }

    #[tokio::test]
    async fn try_pair_deduplicates_identity_and_releases_first_when_second_is_busy() {
        let temp = tempfile::tempdir().unwrap();
        let first_path = temp.path().join("first");
        let second_path = temp.path().join("second");
        fs::create_dir(&first_path).unwrap();
        fs::create_dir(&second_path).unwrap();
        let left = LifecycleGate::for_directory(
            &Dir::open_ambient_dir(first_path, ambient_authority()).unwrap(),
        )
        .unwrap();
        let right = LifecycleGate::for_directory(
            &Dir::open_ambient_dir(second_path, ambient_authority()).unwrap(),
        )
        .unwrap();
        let (first, second) = super::ordered_pair(&left, &right);
        let second = second.unwrap();
        let held = second.acquire().await;

        assert!(matches!(
            LifecycleGate::try_acquire_pair(&left, &right),
            Err(crate::RepoError::RepositoryBusy)
        ));
        assert!(first.try_acquire().is_ok());
        drop(held);

        let deduplicated = LifecycleGate::try_acquire_pair(&left, &left).unwrap();
        drop(deduplicated);
        assert!(left.try_acquire().is_ok());
    }
}
