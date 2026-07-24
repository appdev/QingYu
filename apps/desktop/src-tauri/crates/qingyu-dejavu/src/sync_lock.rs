use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{interval_at, Instant};

use crate::{Cloud, CloudError};

const LOCK_SYNC_KEY: &str = "lock-sync";
const ACQUIRE_ATTEMPTS: usize = 3;
const ACQUIRE_RETRY_DELAY: Duration = Duration::from_secs(5);
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const STALE_AFTER_MILLIS: i64 = 65_000;
const RELEASE_ATTEMPTS: usize = 3;

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
struct LockData {
    #[serde(rename = "deviceID")]
    device_id: String,
    time: i64,
}

#[must_use = "dropping the guard stops refresh but leaves the remote lock until it becomes stale"]
pub struct RemoteLockGuard {
    cloud: Arc<dyn Cloud>,
    stop_refresh: Option<oneshot::Sender<()>>,
    refresh_task: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for RemoteLockGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteLockGuard")
            .field("refresh_active", &self.refresh_task.is_some())
            .finish_non_exhaustive()
    }
}

impl RemoteLockGuard {
    pub async fn release(mut self) -> Result<(), CloudError> {
        self.stop_refresh_task().await;
        for _attempt in 0..RELEASE_ATTEMPTS {
            if self.cloud.remove(LOCK_SYNC_KEY).await.is_ok() {
                return Ok(());
            }
        }
        Err(CloudError::UnlockFailed)
    }

    async fn stop_refresh_task(&mut self) {
        if let Some(stop_refresh) = self.stop_refresh.take() {
            let _send_result = stop_refresh.send(());
        }
        if let Some(refresh_task) = self.refresh_task.take() {
            refresh_task.abort();
            let _join_result = refresh_task.await;
        }
    }
}

impl Drop for RemoteLockGuard {
    fn drop(&mut self) {
        if let Some(stop_refresh) = self.stop_refresh.take() {
            let _send_result = stop_refresh.send(());
        }
        if let Some(refresh_task) = self.refresh_task.take() {
            refresh_task.abort();
        }
    }
}

pub(crate) async fn acquire_remote_lock(
    cloud: Arc<dyn Cloud>,
    device_id: String,
) -> Result<RemoteLockGuard, CloudError> {
    for _attempt in 0..ACQUIRE_ATTEMPTS {
        match try_acquire(&cloud, &device_id).await {
            Ok(()) => return Ok(start_refresh(cloud, device_id)),
            Err(CloudError::Locked) => tokio::time::sleep(ACQUIRE_RETRY_DELAY).await,
            Err(error) => return Err(error),
        }
    }
    Err(CloudError::Locked)
}

fn start_refresh(cloud: Arc<dyn Cloud>, device_id: String) -> RemoteLockGuard {
    let (stop_refresh, mut stopped) = oneshot::channel();
    let refresh_cloud = Arc::clone(&cloud);
    let refresh_task = tokio::spawn(async move {
        let first = Instant::now() + REFRESH_INTERVAL;
        let mut interval = interval_at(first, REFRESH_INTERVAL);
        loop {
            tokio::select! {
                biased;
                _ = &mut stopped => break,
                _ = interval.tick() => {
                    let _refresh_result = write_and_verify_lock(&refresh_cloud, &device_id).await;
                }
            }
        }
    });
    RemoteLockGuard {
        cloud,
        stop_refresh: Some(stop_refresh),
        refresh_task: Some(refresh_task),
    }
}

async fn try_acquire(cloud: &Arc<dyn Cloud>, device_id: &str) -> Result<(), CloudError> {
    let existing = match cloud.get(LOCK_SYNC_KEY).await {
        Ok(bytes) => match serde_json::from_slice::<LockData>(&bytes) {
            Ok(existing) => Some(existing),
            Err(_) => {
                cloud
                    .remove(LOCK_SYNC_KEY)
                    .await
                    .map_err(|_| CloudError::LockFailed)?;
                None
            }
        },
        Err(CloudError::NotFound) => None,
        Err(error) => return Err(error),
    };

    if let Some(existing) = existing {
        let now = now_millis()?;
        let stale = existing
            .time
            .checked_add(STALE_AFTER_MILLIS)
            .is_some_and(|expires| now > expires);
        if !stale && existing.device_id != device_id {
            return Err(CloudError::Locked);
        }
    }

    write_and_verify_lock(cloud, device_id).await
}

async fn write_and_verify_lock(cloud: &Arc<dyn Cloud>, device_id: &str) -> Result<(), CloudError> {
    let lock = LockData {
        device_id: device_id.to_owned(),
        time: now_millis()?,
    };
    let bytes = serde_json::to_vec(&lock).map_err(|_| CloudError::LockFailed)?;
    let written = cloud
        .put(LOCK_SYNC_KEY, &bytes, true)
        .await
        .map_err(|_| CloudError::LockFailed)?;
    if written != bytes.len() as u64 {
        return Err(CloudError::LockFailed);
    }

    let verified = match cloud.get(LOCK_SYNC_KEY).await {
        Ok(bytes) => serde_json::from_slice::<LockData>(&bytes).ok(),
        Err(CloudError::NotFound) => return Err(CloudError::Locked),
        Err(_) => return Err(CloudError::LockFailed),
    };
    if verified.as_ref() == Some(&lock) {
        Ok(())
    } else {
        Err(CloudError::Locked)
    }
}

fn now_millis() -> Result<i64, CloudError> {
    i64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| CloudError::LockFailed)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tempfile::TempDir;

    use crate::{
        Cloud, CloudError, CloudObject, CloudOperation, Device, LocalCloud, Repo, RepoOptions,
        RepoPaths,
    };

    struct RecordingCloud {
        object: Mutex<Option<Vec<u8>>>,
        post_put_replacement: Mutex<Option<Vec<u8>>>,
        get_count: AtomicUsize,
        put_count: AtomicUsize,
        remove_count: AtomicUsize,
        remove_failures: AtomicUsize,
    }

    impl RecordingCloud {
        fn empty() -> Self {
            Self {
                object: Mutex::new(None),
                post_put_replacement: Mutex::new(None),
                get_count: AtomicUsize::new(0),
                put_count: AtomicUsize::new(0),
                remove_count: AtomicUsize::new(0),
                remove_failures: AtomicUsize::new(0),
            }
        }

        fn with_lock(device_id: &str, time: i64) -> Self {
            let cloud = Self::empty();
            *cloud.object.lock().unwrap() = Some(lock_json(device_id, time));
            cloud
        }

        fn counts(&self) -> (usize, usize, usize) {
            (
                self.get_count.load(Ordering::SeqCst),
                self.put_count.load(Ordering::SeqCst),
                self.remove_count.load(Ordering::SeqCst),
            )
        }
    }

    #[async_trait::async_trait]
    impl Cloud for RecordingCloud {
        async fn get(&self, key: &str) -> Result<Vec<u8>, CloudError> {
            assert_eq!(key, "lock-sync");
            self.get_count.fetch_add(1, Ordering::SeqCst);
            self.object
                .lock()
                .unwrap()
                .clone()
                .ok_or(CloudError::NotFound)
        }

        async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError> {
            assert_eq!(key, "lock-sync");
            assert!(overwrite);
            self.put_count.fetch_add(1, Ordering::SeqCst);
            *self.object.lock().unwrap() = Some(bytes.to_vec());
            if let Some(replacement) = self.post_put_replacement.lock().unwrap().take() {
                *self.object.lock().unwrap() = Some(replacement);
            }
            Ok(bytes.len() as u64)
        }

        async fn remove(&self, key: &str) -> Result<(), CloudError> {
            assert_eq!(key, "lock-sync");
            self.remove_count.fetch_add(1, Ordering::SeqCst);
            let failed = self
                .remove_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok();
            if failed {
                return Err(CloudError::Injected(CloudOperation::Remove));
            }
            *self.object.lock().unwrap() = None;
            Ok(())
        }

        async fn list(&self, _prefix: &str) -> Result<Vec<CloudObject>, CloudError> {
            Ok(Vec::new())
        }

        async fn available_size(&self) -> Result<u64, CloudError> {
            Ok(u64::MAX)
        }
    }

    fn repo_fixture() -> (TempDir, Repo) {
        let temp = TempDir::new().unwrap();
        let paths = RepoPaths {
            data: temp.path().join("data"),
            repo: temp.path().join("repo"),
            history: temp.path().join("history"),
            temp: temp.path().join("temp"),
        };
        fs::create_dir_all(&paths.data).unwrap();
        fs::write(paths.data.join("document.md"), b"indexable").unwrap();
        let repo = Repo::open(
            paths,
            Device {
                id: "device-a".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [7; 32],
            RepoOptions::default(),
        )
        .unwrap();
        (temp, repo)
    }

    fn now_millis() -> i64 {
        i64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).unwrap()
    }

    fn lock_json(device_id: &str, time: i64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "deviceID": device_id,
            "time": time,
        }))
        .unwrap()
    }

    async fn advance_retry() {
        tokio::time::advance(Duration::from_secs(5)).await;
        tokio::task::yield_now().await;
    }

    #[tokio::test]
    async fn lock_json_uses_exact_pinned_field_names_and_millisecond_time() {
        let (_temp, repo) = repo_fixture();
        let cloud = Arc::new(LocalCloud::new(_temp.path()).unwrap());

        let guard = repo.lock_cloud(cloud.clone()).await.unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&cloud.get("lock-sync").await.unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        assert_eq!(value["deviceID"], "device-a");
        let written = value["time"].as_i64().unwrap();
        assert!((written - now_millis()).abs() < 1_000);

        guard.release().await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn fresh_other_device_lock_gets_three_attempts_and_three_five_second_waits() {
        let (_temp, repo) = repo_fixture();
        let cloud = Arc::new(RecordingCloud::with_lock("device-b", now_millis()));
        let task = {
            let cloud = cloud.clone();
            tokio::spawn(async move { repo.lock_cloud(cloud).await })
        };
        tokio::task::yield_now().await;
        assert_eq!(cloud.counts(), (1, 0, 0));

        advance_retry().await;
        assert_eq!(cloud.counts(), (2, 0, 0));
        advance_retry().await;
        assert_eq!(cloud.counts(), (3, 0, 0));
        assert!(!task.is_finished());
        advance_retry().await;

        assert!(matches!(task.await.unwrap(), Err(CloudError::Locked)));
        assert_eq!(cloud.counts(), (3, 0, 0));
    }

    #[tokio::test]
    async fn stale_other_device_and_fresh_same_device_locks_can_be_reacquired() {
        for cloud in [
            RecordingCloud::with_lock("device-b", now_millis() - 65_001),
            RecordingCloud::with_lock("device-a", now_millis()),
        ] {
            let (_temp, repo) = repo_fixture();
            let cloud = Arc::new(cloud);
            let guard = repo.lock_cloud(cloud.clone()).await.unwrap();
            assert_eq!(cloud.counts(), (2, 1, 0));
            let stored: serde_json::Value =
                serde_json::from_slice(cloud.object.lock().unwrap().as_ref().unwrap()).unwrap();
            assert_eq!(stored["deviceID"], "device-a");
            guard.release().await.unwrap();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn write_then_reread_detects_a_competing_lock_and_fails_acquisition() {
        let (_temp, repo) = repo_fixture();
        let cloud = Arc::new(RecordingCloud::empty());
        *cloud.post_put_replacement.lock().unwrap() = Some(lock_json("device-b", now_millis()));
        let task = {
            let cloud = cloud.clone();
            tokio::spawn(async move { repo.lock_cloud(cloud).await })
        };
        tokio::task::yield_now().await;
        assert_eq!(cloud.counts(), (2, 1, 0));
        advance_retry().await;
        advance_retry().await;
        advance_retry().await;

        assert!(matches!(task.await.unwrap(), Err(CloudError::Locked)));
        assert_eq!(cloud.counts(), (4, 1, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_runs_every_thirty_seconds_and_explicit_release_stops_it() {
        let (_temp, repo) = repo_fixture();
        let cloud = Arc::new(RecordingCloud::empty());
        let guard = repo.lock_cloud(cloud.clone()).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(cloud.counts(), (2, 1, 0));

        tokio::time::advance(Duration::from_secs(29)).await;
        tokio::task::yield_now().await;
        assert_eq!(cloud.counts(), (2, 1, 0));
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(cloud.counts(), (3, 2, 0));

        guard.release().await.unwrap();
        assert_eq!(cloud.counts(), (3, 2, 1));
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;
        assert_eq!(cloud.counts(), (3, 2, 1));
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_guard_stops_refresh_without_removing_the_remote_lock() {
        let (_temp, repo) = repo_fixture();
        let cloud = Arc::new(RecordingCloud::empty());
        let guard = repo.lock_cloud(cloud.clone()).await.unwrap();
        drop(guard);
        tokio::task::yield_now().await;

        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;
        assert_eq!(cloud.counts(), (2, 1, 0));
        assert!(cloud.object.lock().unwrap().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn release_retries_three_times_returns_typed_error_and_keeps_refresh_stopped() {
        let (_temp, repo) = repo_fixture();
        let cloud = Arc::new(RecordingCloud::empty());
        cloud.remove_failures.store(3, Ordering::SeqCst);
        let guard = repo.lock_cloud(cloud.clone()).await.unwrap();

        assert!(matches!(
            guard.release().await,
            Err(CloudError::UnlockFailed)
        ));
        assert_eq!(cloud.counts(), (2, 1, 3));
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;
        assert_eq!(cloud.counts(), (2, 1, 3));
    }

    #[tokio::test]
    async fn release_succeeds_on_the_third_remove_attempt() {
        let (_temp, repo) = repo_fixture();
        let cloud = Arc::new(RecordingCloud::empty());
        cloud.remove_failures.store(2, Ordering::SeqCst);
        let guard = repo.lock_cloud(cloud.clone()).await.unwrap();

        guard.release().await.unwrap();
        assert_eq!(cloud.counts(), (2, 1, 3));
        assert!(cloud.object.lock().unwrap().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn remote_retry_wait_does_not_hold_the_local_repository_operation_mutex() {
        let (_temp, repo) = repo_fixture();
        let repo = Arc::new(repo);
        let cloud = Arc::new(RecordingCloud::with_lock("device-b", now_millis()));
        let task = {
            let repo = repo.clone();
            let cloud = cloud.clone();
            tokio::spawn(async move { repo.lock_cloud(cloud).await })
        };
        tokio::task::yield_now().await;

        let index = repo.index("while remote lock retries").unwrap();
        assert_eq!(index.count, 1);

        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}
