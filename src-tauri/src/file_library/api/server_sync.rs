//! 网页客户端的同步。和 App 一样由用户在设置里填写 S3 或选择轻羽云存储，配置保存在客户端配置目录；
//! 同步本身由 Node 子进程运行和 App 相同的 syncNotebook，笔记读写经 dispatch 回到这里的 DiskLibrary 事务。
use super::*;
use crate::{
    client_account::{AccountError, ClientAccount},
    sync_profile::{self, Profile},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    io::{BufRead, BufReader, BufWriter, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const INTERVALS: [u64; 7] = [1, 3, 5, 10, 15, 30, 60];

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncStatus {
    busy: bool,
    error: bool,
    message: String,
    last_synced_at: Option<u64>,
    last_synced_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<&'static str>,
}

/// 自动同步设置，含义和 App 的偏好设置相同：S3 自动同步开关、开启了云存储自动同步的账号、同步间隔（分钟）。
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Schedule {
    s3: bool,
    cloud_account: String,
    interval: u64,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            s3: true,
            cloud_account: String::new(),
            interval: 3,
        }
    }
}

pub(super) struct ServerSync {
    config: Option<PathBuf>,
    worker: PathBuf,
    status: Mutex<SyncStatus>,
    started: Mutex<Option<Instant>>,
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or("配置目录无效")?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(error)?;
    }
    temp.write_all(bytes).map_err(error)?;
    temp.as_file().sync_all().map_err(error)?;
    temp.persist(path).map_err(error)?;
    Ok(())
}

fn account_message(error: AccountError) -> String {
    match error {
        AccountError::Code(code) => code.into(),
        AccountError::Redemption { message, .. } => message,
    }
}

impl ServerSync {
    pub(super) fn open(config: Option<PathBuf>, root: &Path) -> Result<Arc<Self>> {
        let worker = environment("SYNC_WORKER")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dist-sync/server-sync-worker.mjs"));
        let sync = Arc::new(Self {
            config,
            worker,
            status: Default::default(),
            started: Default::default(),
        });
        sync.import_legacy(root)?;
        Ok(sync)
    }

    /// 旧部署用 QINGYU_SYNC_PROFILE 指定 S3 配置。配置目录里还没有配置时导入一次，之后以设置页为准。
    fn import_legacy(&self, root: &Path) -> Result<()> {
        let (Some(legacy), Some(config)) = (environment("SYNC_PROFILE"), &self.config) else {
            return Ok(());
        };
        let target = sync_profile::profile_path(config);
        if target.exists() {
            return Ok(());
        }
        let legacy = fs::canonicalize(legacy).map_err(|_| "无法读取 QINGYU_SYNC_PROFILE")?;
        if legacy.starts_with(fs::canonicalize(root).map_err(error)?) {
            return Err("QINGYU_SYNC_PROFILE 必须在笔记目录以外".into());
        }
        let profile = sync_profile::load(&legacy)?.ok_or("无法读取 QINGYU_SYNC_PROFILE")?;
        sync_profile::save(&target, &profile)?;
        self.save_schedule(&Schedule {
            interval: 1,
            ..Default::default()
        })
    }

    fn unavailable(&self) -> Option<&'static str> {
        if self.config.is_none() {
            Some("部署时没有设置配置目录（QINGYU_CONFIG_DIR），不能保存同步设置")
        } else if !self.worker.is_file() {
            Some("部署时没有同步程序（QINGYU_SYNC_WORKER），暂时不能同步")
        } else {
            None
        }
    }

    fn config(&self) -> Result<&Path> {
        self.config
            .as_deref()
            .ok_or_else(|| self.unavailable().unwrap_or("配置目录不可用").into())
    }

    pub(super) fn status(&self) -> SyncStatus {
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        status.unavailable = self.unavailable();
        status
    }

    pub(super) fn profile(&self) -> Result<Option<Profile>> {
        Ok(sync_profile::load(&sync_profile::profile_path(self.config()?))?)
    }

    pub(super) fn save_profile(&self, profile: &Profile) -> Result<()> {
        Ok(sync_profile::save(&sync_profile::profile_path(self.config()?), profile)?)
    }

    pub(super) fn schedule(&self) -> Result<Schedule> {
        let path = self.config()?.join("sync-schedule.json");
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "自动同步设置损坏".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Schedule::default()),
            Err(error) => Err(error.to_string().into()),
        }
    }

    pub(super) fn save_schedule(&self, schedule: &Schedule) -> Result<()> {
        if !INTERVALS.contains(&schedule.interval)
            || schedule.cloud_account.len() > 512
            || schedule.cloud_account.chars().any(char::is_control)
        {
            return Err("自动同步设置无效".into());
        }
        let path = self.config()?.join("sync-schedule.json");
        write_private(&path, &serde_json::to_vec(schedule).map_err(error)?)
    }

    /// 开始一次同步。带账号时用轻羽云存储，否则用保存的 S3 配置。同步在后台进行，进度从 status 读取。
    pub(super) fn trigger(
        self: &Arc<Self>,
        state: Arc<Service>,
        account: Option<String>,
    ) -> std::result::Result<(), String> {
        if let Some(reason) = self.unavailable() {
            return Err(reason.into());
        }
        {
            let mut status = self.status.lock().unwrap_or_else(|err| err.into_inner());
            if status.busy {
                return Err("同步正在进行，请稍候".into());
            }
            status.busy = true;
            status.error = false;
            status.message = if account.is_some() { "正在连接轻羽云存储…" } else { "正在连接 S3…" }.into();
        }
        *self.started.lock().unwrap_or_else(|err| err.into_inner()) = Some(Instant::now());
        let sync = self.clone();
        tokio::spawn(async move {
            let result = sync.run(&state, account.as_deref()).await;
            let mut status = sync.status.lock().unwrap_or_else(|err| err.into_inner());
            status.busy = false;
            match result {
                Ok(message) => {
                    status.error = false;
                    status.message = message.unwrap_or_else(|| "同步完成".into());
                    status.last_synced_at = Some(now() as u64);
                    status.last_synced_account = account;
                }
                Err(message) => {
                    status.error = true;
                    status.message = message;
                }
            }
        });
        Ok(())
    }

    async fn worker_profile(&self, state: &Service, account: Option<&str>) -> std::result::Result<serde_json::Value, String> {
        let profile = self.profile().map_err(|e| e.to_string())?;
        match account {
            Some(account) => {
                // 还没有保存过配置时和 App 一样默认是轻羽云存储
                if !profile.as_ref().is_none_or(Profile::cloud) {
                    return Err("请先切换至轻羽云存储".into());
                }
                let session = ClientAccount::session_path(self.config().map_err(|e| e.to_string())?);
                let grant = state
                    .account
                    .sync_config(session, account)
                    .await
                    .map_err(account_message)?;
                let mut value = serde_json::to_value(grant).map_err(|e| e.to_string())?;
                let concurrency = value["concurrentReqs"].as_u64().unwrap_or(2).clamp(1, 8);
                value["concurrency"] = concurrency.into();
                Ok(value)
            }
            None => {
                let profile = profile.ok_or("请先填写 S3 连接配置")?;
                if profile.cloud() {
                    return Err("请先切换至 S3 同步".into());
                }
                serde_json::to_value(profile).map_err(|e| e.to_string())
            }
        }
    }

    async fn run(&self, state: &Arc<Service>, account: Option<&str>) -> std::result::Result<Option<String>, String> {
        let profile = self.worker_profile(state, account).await?;
        self.spawn(state, profile, "sync").await
    }

    /// 测试连接：用表单里的配置跑一次读写删检查，不保存配置。
    pub(super) async fn test(&self, state: &Arc<Service>, profile: Profile) -> std::result::Result<(), String> {
        if let Some(reason) = self.unavailable() {
            return Err(reason.into());
        }
        profile.validate()?;
        if profile.cloud() {
            return Err("请先切换至 S3 同步".into());
        }
        self.spawn(state, serde_json::to_value(profile).map_err(|e| e.to_string())?, "test")
            .await
            .map(|_| ())
    }

    async fn spawn(&self, state: &Arc<Service>, profile: serde_json::Value, task: &'static str) -> std::result::Result<Option<String>, String> {
        // 子进程从配置目录里的临时文件读取连接配置，用完即删
        let dir = self.config().map_err(|e| e.to_string())?.to_owned();
        let file = tokio::task::spawn_blocking(move || -> Result<tempfile::TempPath> {
            let file = tempfile::NamedTempFile::new_in(&dir).map_err(error)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.as_file()
                    .set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(error)?;
            }
            serde_json::to_writer(file.as_file(), &profile).map_err(error)?;
            Ok(file.into_temp_path())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        let (state, worker, path) = (state.clone(), self.worker.clone(), file.to_path_buf());
        let result = tokio::task::spawn_blocking(move || run_worker(&state, &worker, &path, task))
            .await
            .map_err(|e| e.to_string())?;
        drop(file);
        result
    }

    /// 按自动同步设置定时同步：S3 开关或云存储账号开启时，按间隔触发。
    pub(super) fn start(self: &Arc<Self>, state: Arc<Service>) {
        let sync = self.clone();
        tokio::spawn(async move {
            let mut poll = tokio::time::interval(Duration::from_secs(30));
            poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                poll.tick().await;
                let (Ok(schedule), Ok(profile)) = (sync.schedule(), sync.profile()) else {
                    continue;
                };
                let account = if profile.as_ref().is_none_or(Profile::cloud) {
                    if schedule.cloud_account.is_empty() {
                        continue;
                    }
                    Some(schedule.cloud_account.clone())
                } else if schedule.s3 && profile.as_ref().is_some_and(Profile::complete) {
                    None
                } else {
                    continue;
                };
                let due = sync
                    .started
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(schedule.interval * 60));
                if due {
                    let _ = sync.trigger(state.clone(), account);
                }
            }
        });
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Operation {
    Snapshot,
    ReadAsset {
        asset: assets::AssetStat,
        revision: u64,
    },
    Stage {
        data: String,
        revision: u64,
    },
    ReadStage {
        asset: assets::StagedAsset,
    },
    Release,
    /// 子进程失败前报告的原因，例如 S3 返回的错误
    Failed {
        message: String,
    },
    /// 同步完成后的统计，例如笔记、文件夹和附件数量
    Finished {
        message: String,
    },
    Commit {
        revision: u64,
        next: Library,
        changes: assets::AssetSync,
        published: Option<serde_json::Value>,
        expected_sync: Option<serde_json::Value>,
    },
}

struct Stage {
    files: assets::StageFiles,
    _lock: fs::File,
}

fn dispatch(
    state: &Service,
    stage: &mut Option<Stage>,
    op: Operation,
) -> Result<serde_json::Value> {
    let mut store = state.store.lock().map_err(error)?;
    let value = match op {
        Operation::Snapshot => {
            store.reconcile(None)?;
            json!({ "value": { "library": store.data, "revision": store.revision,
                "assets": assets::scan_assets(&store.root)?, "placeholders": assets::placeholders::load(&store)? } })
        }
        Operation::ReadAsset { asset, revision } => {
            if revision != store.revision {
                return Ok(json!({"error":"stale"}));
            }
            json!({"value": STANDARD.encode(assets::read_asset(&store.root, &asset)?)})
        }
        Operation::Stage { data, revision } => {
            if revision != store.revision {
                return Ok(json!({"error":"stale"}));
            }
            if data.len() > assets::MAX_FILE.div_ceil(3) * 4 {
                return Err("附件超过 64 MiB".into());
            }
            let bytes = STANDARD.decode(data).map_err(error)?;
            if bytes.len() > assets::MAX_FILE {
                return Err("附件超过 64 MiB".into());
            }
            if stage.is_none() {
                let lock = assets::stage_lock(&store.root)?;
                try_lock(&lock).map_err(error)?;
                *stage = Some(Stage {
                    files: assets::StageFiles::new(&store.root),
                    _lock: lock,
                });
            }
            json!({"value":stage.as_mut().unwrap().files.write(&bytes)?})
        }
        Operation::ReadStage { asset } => {
            if stage.is_none() {
                return Err("没有附件暂存会话".into());
            }
            json!({"value":STANDARD.encode(asset.read(&store.root)?)})
        }
        Operation::Release => {
            stage.take();
            json!({})
        }
        Operation::Failed { .. } | Operation::Finished { .. } => json!({}),
        Operation::Commit {
            revision,
            mut next,
            changes,
            published,
            expected_sync,
        } => {
            store.reconcile(None)?;
            if revision != store.revision {
                if let Some(published) = published.filter(|_| store.data.sync == expected_sync) {
                    // Acknowledge only the local versions already published by MDSync.
                    let target = next
                        .sync
                        .as_ref()
                        .and_then(|value| value.get("target"))
                        .ok_or("缺少同步目标")?;
                    let mut current = store.data.clone();
                    let mut sync = current
                        .sync
                        .take()
                        .filter(|sync| sync.get("target") == Some(target))
                        .unwrap_or_else(|| json!({"version":1,"target":target}));
                    sync["state"] = published;
                    current.sync = Some(sync);
                    store.commit(current, "sync-checkpoint")?;
                }
                return Ok(json!({"error":"stale"}));
            }
            if next
                .notes
                .iter()
                .any(|note| note.id == store.data.active_id && !note.trashed)
            {
                next.active_id.clone_from(&store.data.active_id);
            }
            store.commit_bytes(next, "sync", &HashMap::new(), Some(&changes))?;
            json!({})
        }
    };
    Ok(value)
}

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 成功时返回同步程序报告的完成提示。
fn run_worker(state: &Service, worker: &Path, profile: &Path, task: &str) -> std::result::Result<Option<String>, String> {
    let mut child = Worker(
        Command::new("node")
            .arg(worker)
            .arg(profile)
            .arg(task)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "无法启动同步程序，请确认部署环境安装了 Node.js 22 或更高版本")?,
    );
    let output = BufReader::new(child.0.stdout.take().ok_or("无同步输出")?);
    let mut input = BufWriter::new(child.0.stdin.take().ok_or("无同步输入")?);
    let mut stage = None;
    let (mut failure, mut finished) = (None, None);
    for line in output.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let op: Operation = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        match &op {
            Operation::Failed { message } => failure = Some(message.clone()),
            Operation::Finished { message } => finished = Some(message.clone()),
            _ => {}
        }
        let response =
            dispatch(state, &mut stage, op).unwrap_or_else(|_| json!({"error":"storage"}));
        serde_json::to_writer(&mut input, &response).map_err(|e| e.to_string())?;
        input.write_all(b"\n").map_err(|e| e.to_string())?;
        input.flush().map_err(|e| e.to_string())?;
    }
    if child.0.wait().map_err(|e| e.to_string())?.success() {
        Ok(finished)
    } else {
        Err(failure.unwrap_or_else(|| "同步未完成".into()))
    }
}

/// 设置页使用的同步接口，挂在 browser_library 下，需要先用访问密码解锁。
pub(super) fn routes() -> Router<Arc<Service>> {
    Router::new()
        .route("/web/sync", get(status).post(start))
        .route("/web/sync/profile", get(load_profile).put(store_profile))
        .route("/web/sync/schedule", get(load_schedule).put(store_schedule))
        .route("/web/sync/test", axum::routing::post(test))
}

fn failed(message: impl Into<String>) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message.into() }))).into_response()
}

fn sync_of(state: &Service) -> std::result::Result<&Arc<ServerSync>, Response> {
    state.sync.as_ref().ok_or_else(|| failed("同步不可用"))
}

async fn status(State(state): State<Arc<Service>>) -> Json<SyncStatus> {
    Json(state.sync.as_ref().map(|sync| sync.status()).unwrap_or(SyncStatus {
        unavailable: Some("同步不可用"),
        ..Default::default()
    }))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Start {
    account_id: Option<String>,
}

async fn start(State(state): State<Arc<Service>>, body: Option<Json<Start>>) -> Response {
    let sync = match sync_of(&state) {
        Ok(sync) => sync.clone(),
        Err(response) => return response,
    };
    let account = body.and_then(|Json(body)| body.account_id).filter(|id| !id.is_empty());
    match sync.trigger(state.clone(), account) {
        Ok(()) => Json(sync.status()).into_response(),
        Err(message) => failed(message),
    }
}

async fn blocking<T: Send + 'static>(
    state: &Arc<Service>,
    action: impl FnOnce(&ServerSync) -> Result<T> + Send + 'static,
) -> std::result::Result<T, Response> {
    let sync = sync_of(state)?.clone();
    tokio::task::spawn_blocking(move || action(&sync))
        .await
        .map_err(|_| failed("操作未完成"))?
        .map_err(|e| failed(e.to_string()))
}

async fn load_profile(State(state): State<Arc<Service>>) -> Response {
    match blocking(&state, |sync| sync.profile()).await {
        Ok(profile) => Json(profile).into_response(),
        Err(response) => response,
    }
}

async fn store_profile(
    State(state): State<Arc<Service>>,
    body: std::result::Result<Json<Profile>, JsonRejection>,
) -> Response {
    let Ok(Json(profile)) = body else {
        return failed("连接配置无效");
    };
    match blocking(&state, move |sync| sync.save_profile(&profile)).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(response) => response,
    }
}

async fn load_schedule(State(state): State<Arc<Service>>) -> Response {
    match blocking(&state, |sync| sync.schedule()).await {
        Ok(schedule) => Json(schedule).into_response(),
        Err(response) => response,
    }
}

async fn store_schedule(
    State(state): State<Arc<Service>>,
    body: std::result::Result<Json<Schedule>, JsonRejection>,
) -> Response {
    let Ok(Json(schedule)) = body else {
        return failed("自动同步设置无效");
    };
    match blocking(&state, move |sync| sync.save_schedule(&schedule)).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(response) => response,
    }
}

async fn test(
    State(state): State<Arc<Service>>,
    body: std::result::Result<Json<Profile>, JsonRejection>,
) -> Response {
    let Ok(Json(profile)) = body else {
        return failed("连接配置无效");
    };
    let sync = match sync_of(&state) {
        Ok(sync) => sync.clone(),
        Err(response) => return response,
    };
    match sync.test(&state, profile).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(message) => failed(message),
    }
}
