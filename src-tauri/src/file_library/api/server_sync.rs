//! 网页客户端的同步。和 App 一样由用户在设置里填写 S3 或选择轻羽云存储，配置保存在客户端配置目录；
//! 同步本身由 Node 子进程运行和 App 相同的 syncNotebook，笔记读写经 dispatch 回到这里的 DiskLibrary 事务。
use super::*;
use crate::{
    client_account::{storage::UploadObject, AccountError, ClientAccount},
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
        Ok(sync_profile::load(&sync_profile::profile_path(
            self.config()?,
        ))?)
    }

    pub(super) fn save_profile(&self, profile: &Profile) -> Result<()> {
        Ok(sync_profile::save(
            &sync_profile::profile_path(self.config()?),
            profile,
        )?)
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
            status.message = if account.is_some() {
                "正在连接轻羽云存储…"
            } else {
                "正在连接 S3…"
            }
            .into();
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

    fn worker_profile(
        &self,
        account: Option<&str>,
    ) -> std::result::Result<serde_json::Value, String> {
        let profile = self.profile().map_err(|e| e.to_string())?;
        match account {
            Some(_) => {
                // 还没有保存过配置时和 App 一样默认是轻羽云存储
                if !profile.as_ref().is_none_or(Profile::cloud) {
                    return Err("请先切换至轻羽云存储".into());
                }
                Ok(json!({ "mode": "cloud" }))
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

    async fn run(
        &self,
        state: &Arc<Service>,
        account: Option<&str>,
    ) -> std::result::Result<Option<String>, String> {
        let profile = self.worker_profile(account)?;
        let result = self.spawn(state, profile, "sync", account, None).await?;
        if let (Some(account), Some(config)) = (account, &self.config) {
            let _ = state
                .account
                .refresh_storage_usage(ClientAccount::session_path(config), account, None)
                .await;
        }
        Ok(result.and_then(|value| value.as_str().map(str::to_owned)))
    }

    /// 测试连接：用表单里的配置跑一次读写删检查，不保存配置。
    pub(super) async fn test(
        &self,
        state: &Arc<Service>,
        profile: Profile,
    ) -> std::result::Result<(), String> {
        if let Some(reason) = self.unavailable() {
            return Err(reason.into());
        }
        profile.validate()?;
        if profile.cloud() {
            return Err("请先切换至 S3 同步".into());
        }
        self.spawn(
            state,
            serde_json::to_value(profile).map_err(|e| e.to_string())?,
            "test",
            None,
            None,
        )
        .await
        .map(|_| ())
    }

    async fn spawn(
        &self,
        state: &Arc<Service>,
        profile: serde_json::Value,
        task: &'static str,
        account: Option<&str>,
        password_action: Option<PasswordAction>,
    ) -> std::result::Result<Option<serde_json::Value>, String> {
        // 子进程从配置目录里的临时文件读取连接配置，用完即删
        let dir = self.config().map_err(|e| e.to_string())?.to_owned();
        let cloud = account.map(|id| (ClientAccount::session_path(&dir), id.to_owned()));
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
        let result = tokio::task::spawn_blocking(move || {
            run_worker(
                &state,
                &worker,
                &path,
                task,
                cloud.as_ref(),
                password_action,
            )
        })
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
                    .is_none_or(|last| {
                        last.elapsed() >= Duration::from_secs(schedule.interval * 60)
                    });
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
    Credentials {
        rejected_key: Option<String>,
        #[serde(default)]
        begin: bool,
        generation: Option<u64>,
    },
    CheckStorage {
        generation: u64,
    },
    Upload {
        upload_id: u64,
        object: UploadObject,
        body: String,
    },
    DeleteObjects {
        config_version: u64,
        generation: Option<u64>,
        keys: Vec<String>,
    },
    CachedKey {
        target: String,
        key: Option<String>,
    },
    PasswordAction,
    PasswordResult {
        result: serde_json::Value,
    },
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
        generation: Option<u64>,
        revision: u64,
        next: Library,
        changes: assets::AssetSync,
        published: Option<serde_json::Value>,
        expected_sync: Option<serde_json::Value>,
    },
}

struct Stage {
    files: assets::StageFiles,
    _lock: FileLock,
}

fn dispatch(
    state: &Service,
    stage: &mut Option<Stage>,
    op: Operation,
) -> Result<serde_json::Value> {
    let mut store = state.store.lock().map_err(error)?;
    let value = match op {
        Operation::Credentials { .. }
        | Operation::CheckStorage { .. }
        | Operation::Upload { .. }
        | Operation::DeleteObjects { .. }
        | Operation::CachedKey { .. }
        | Operation::PasswordAction
        | Operation::PasswordResult { .. } => {
            return Err(error("operation requires a cloud sync session"))
        }
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
                let lock = try_lock(lock).map_err(error)?;
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
            generation: _,
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
                    if let Some(binding) =
                        next.sync.as_ref().and_then(|sync| sync.get("cloudBinding"))
                    {
                        sync["cloudBinding"] = binding.clone();
                    }
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

fn reply_to_worker(
    input: &Mutex<BufWriter<std::process::ChildStdin>>,
    response: &serde_json::Value,
) -> std::result::Result<(), String> {
    let mut input = input.lock().map_err(|_| "同步进程输入不可用")?;
    serde_json::to_writer(&mut *input, response).map_err(|e| e.to_string())?;
    input.write_all(b"\n").map_err(|e| e.to_string())?;
    input.flush().map_err(|e| e.to_string())
}

/// 成功时返回同步程序报告的完成提示。
fn run_worker(
    state: &Service,
    worker: &Path,
    profile: &Path,
    task: &str,
    cloud: Option<&(PathBuf, String)>,
    mut password_action: Option<PasswordAction>,
) -> std::result::Result<Option<serde_json::Value>, String> {
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
    let input = Arc::new(Mutex::new(BufWriter::new(
        child.0.stdin.take().ok_or("无同步输入")?,
    )));
    #[derive(Default)]
    struct Watch(Option<tokio::task::JoinHandle<()>>);
    impl Drop for Watch {
        fn drop(&mut self) {
            if let Some(task) = &self.0 {
                task.abort();
            }
        }
    }
    let mut watch = Watch::default();
    let mut upload_jobs = tokio::task::JoinSet::new();
    let mut stage = None;
    let (mut failure, mut finished) = (None, None);
    for line in output.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let op: Operation = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if let Operation::Upload {
            upload_id,
            object,
            body,
        } = op
        {
            let bytes = (body.len() <= (64_usize * 1024 * 1024).div_ceil(3) * 4)
                .then(|| STANDARD.decode(body.as_bytes()).ok())
                .flatten();
            let (Some((session, account)), Some(bytes)) = (cloud, bytes) else {
                reply_to_worker(
                    &input,
                    &json!({ "uploadId": upload_id, "credentialError": "invalid_request" }),
                )?;
                continue;
            };
            if bytes.len() > 64 * 1024 * 1024 {
                reply_to_worker(
                    &input,
                    &json!({ "uploadId": upload_id, "credentialError": "invalid_request" }),
                )?;
                continue;
            }
            let (client, session, account, input) = (
                state.account.clone(),
                session.clone(),
                account.clone(),
                input.clone(),
            );
            while upload_jobs.try_join_next().is_some() {}
            upload_jobs.spawn(async move {
                let response = match client
                    .storage_upload(session, &account, object, bytes)
                    .await
                {
                    Ok(status) => json!({ "uploadId": upload_id, "value": status }),
                    Err(error) => {
                        json!({ "uploadId": upload_id, "credentialError": account_message(error) })
                    }
                };
                let _ = reply_to_worker(&input, &response);
            });
            continue;
        }
        match &op {
            Operation::Failed { message } => failure = Some(message.clone()),
            Operation::Finished { message } => finished = Some(json!(message)),
            Operation::PasswordResult { result } if task == "password" => {
                finished = Some(result.clone())
            }
            _ => {}
        }
        let response = match cloud {
            Some((session, account)) => tokio::runtime::Handle::current().block_on(async {
                let result = match op {
                    Operation::Credentials {
                        rejected_key,
                        begin,
                        generation,
                    } => state
                        .account
                        .sync_config(
                            session.clone(),
                            account,
                            rejected_key.as_deref(),
                            begin,
                            generation,
                        )
                        .await
                        .and_then(|config| {
                            let mut value = json!(config);
                            if begin && value["protocolVersion"] == 2 {
                                let mut cancellation = state.account.watch_storage(
                                    value["storageGeneration"]
                                        .as_u64()
                                        .ok_or("invalid_response")?,
                                )?;
                                let input = input.clone();
                                watch.0 = Some(tokio::spawn(async move {
                                    if cancellation.changed().await.is_ok() {
                                        if let Ok(mut input) = input.lock() {
                                            let _ = input
                                                .write_all(b"{\"cancelStorage\":true}\n")
                                                .and_then(|_| input.flush());
                                        }
                                    }
                                }));
                            }
                            value["accountId"] = json!(account);
                            Ok(value)
                        }),
                    Operation::CheckStorage { generation } => state
                        .account
                        .with_sync_account(session.clone(), account, async {
                            state.account.watch_storage(generation).map(|_| json!(null))
                        })
                        .await
                        .and_then(|result| result),
                    Operation::DeleteObjects {
                        config_version,
                        generation,
                        keys,
                    } => state
                        .account
                        .storage_delete(session.clone(), account, config_version, generation, keys)
                        .await
                        .map(|()| json!(null)),
                    Operation::CachedKey { target, key } => state
                        .account
                        .with_sync_account(session.clone(), account, async {
                            state
                                .config
                                .as_ref()
                                .and_then(|dir| {
                                    sync_profile::cached_key(
                                        dir,
                                        &target,
                                        key,
                                        Some(account.clone()),
                                        false,
                                    )
                                    .ok()
                                })
                                .ok_or(AccountError::Code("secure_storage"))
                                .map(|value| json!(value))
                        })
                        .await
                        .and_then(|result| result),
                    Operation::PasswordAction if task == "password" => {
                        Ok(json!(password_action.take()))
                    }
                    Operation::PasswordResult { .. } if task == "password" => Ok(json!(null)),
                    op @ Operation::Commit { generation, .. } => {
                        return state
                            .account
                            .with_sync_account(session.clone(), account, async {
                                if let Some(generation) = generation {
                                    if let Err(error) = state.account.watch_storage(generation) {
                                        return json!({"credentialError": account_message(error)});
                                    }
                                }
                                dispatch(state, &mut stage, op)
                                    .unwrap_or_else(|_| json!({"error":"storage"}))
                            })
                            .await
                            .unwrap_or_else(
                                |error| json!({"credentialError": account_message(error)}),
                            )
                    }
                    op => {
                        return dispatch(state, &mut stage, op)
                            .unwrap_or_else(|_| json!({"error":"storage"}))
                    }
                };
                match result {
                    Ok(value) => json!({ "value": value }),
                    Err(error) => json!({ "credentialError": account_message(error) }),
                }
            }),
            None => dispatch(state, &mut stage, op).unwrap_or_else(|_| json!({"error":"storage"})),
        };
        reply_to_worker(&input, &response)?;
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
        .route("/web/sync/password", axum::routing::post(password))
}

fn failed(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.into() })),
    )
        .into_response()
}

fn sync_of(state: &Service) -> std::result::Result<&Arc<ServerSync>, Response> {
    state.sync.as_ref().ok_or_else(|| failed("同步不可用"))
}

async fn status(State(state): State<Arc<Service>>) -> Json<SyncStatus> {
    Json(
        state
            .sync
            .as_ref()
            .map(|sync| sync.status())
            .unwrap_or(SyncStatus {
                unavailable: Some("同步不可用"),
                ..Default::default()
            }),
    )
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
    let account = body
        .and_then(|Json(body)| body.account_id)
        .filter(|id| !id.is_empty());
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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PasswordAction {
    state: String,
    etag: Option<String>,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PasswordRequest {
    account_id: String,
    action: Option<PasswordAction>,
}

async fn password(
    State(state): State<Arc<Service>>,
    body: std::result::Result<Json<PasswordRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return failed("密钥请求无效");
    };
    let sync = match sync_of(&state) {
        Ok(sync) => sync,
        Err(response) => return response,
    };
    if let Some(reason) = sync.unavailable() {
        return failed(reason);
    }
    let profile = match sync.worker_profile(Some(&body.account_id)) {
        Ok(profile) => profile,
        Err(error) => return failed(error),
    };
    {
        let mut status = sync.status.lock().unwrap_or_else(|err| err.into_inner());
        if status.busy {
            return failed("同步正在进行，请稍候");
        }
        status.busy = true;
    }
    struct Busy(Arc<ServerSync>);
    impl Drop for Busy {
        fn drop(&mut self) {
            self.0
                .status
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .busy = false;
        }
    }
    let busy = Busy(sync.clone());
    let result = tokio::spawn(async move {
        let unlock = body
            .action
            .as_ref()
            .is_some_and(|action| matches!(action.state.as_str(), "new" | "locked"));
        let sync = busy.0.clone();
        let result = sync
            .spawn(
                &state,
                profile,
                "password",
                Some(&body.account_id),
                body.action,
            )
            .await;
        drop(busy);
        if unlock
            && matches!(&result, Ok(Some(info)) if info["state"] == "unlocked")
            && sync
                .schedule()
                .is_ok_and(|schedule| schedule.cloud_account == body.account_id)
        {
            let _ = sync.trigger(state, Some(body.account_id));
        }
        result
    })
    .await;
    match result {
        Ok(Ok(Some(result))) => Json(result).into_response(),
        Ok(Err(error)) => failed(error),
        _ => failed("密钥操作未完成"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn starts_cloud_sync_only_after_unlocking_the_scheduled_account() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("notes");
        let mut state = Service::open(
            root.clone(),
            Some("synthetic-token-for-sync-tests-12345"),
            None,
            None,
        )
        .unwrap();
        let mut sync = ServerSync::open(Some(dir.path().to_owned()), &root).unwrap();
        let worker = dir.path().join("worker.cjs");
        fs::write(&worker, r#"
            const password = process.argv[3] === 'password';
            const reply = password ? {operation:'passwordResult', result:{state:'unlocked', etag:null}}
                                   : {operation:'finished', message:'done'};
            process.stdout.write(JSON.stringify(reply) + '\n');
            process.stdin.once('data', () => process.exit(0));
        "#).unwrap();
        Arc::get_mut(&mut sync).unwrap().worker = worker;
        Arc::get_mut(&mut state).unwrap().sync = Some(sync.clone());
        sync.save_schedule(&Schedule {
            cloud_account: "one".into(),
            ..Schedule::default()
        })
        .unwrap();

        for (account, key_state) in [
            ("one", None),
            ("other", Some("new")),
            ("one", Some("new")),
            ("one", Some("locked")),
        ] {
            let previous = *sync.started.lock().unwrap();
            let action = key_state.map(|s| PasswordAction {
                state: s.into(),
                etag: None,
                password: "test".into(),
            });
            let response = password(
                State(state.clone()),
                Ok(Json(PasswordRequest {
                    account_id: account.into(),
                    action,
                })),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            if account == "one" && key_state.is_some() {
                assert_ne!(*sync.started.lock().unwrap(), previous);
                tokio::time::timeout(Duration::from_secs(5), async {
                    while sync.status().busy {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                assert_eq!(sync.status().last_synced_account.as_deref(), Some(account));
            } else {
                assert_eq!(*sync.started.lock().unwrap(), previous);
            }
        }
    }
}
