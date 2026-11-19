use reqwest::{redirect::Policy, Client, Method};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, DirBuilder, Metadata, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

const API_ORIGIN: &str = "https://api.qynotes.com";
const PORTAL: &str = "https://qynotes.com";
const API: &str = "/api/client/v1";
const STORAGE_ERROR: &str = "secure_storage";
const MAX_SESSION_BYTES: u64 = 64 * 1024;
const MAX_CACHE_BYTES: u64 = 1024 * 1024;
const ACCOUNT_CACHE_TTL: u64 = 24 * 60 * 60;
const SYNC_PREFIX: &str = "repo/qingyu/v1";
type Result<T> = std::result::Result<T, AccountError>;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum AccountError {
    Code(&'static str),
    Redemption { code: &'static str, message: String },
}

impl From<&'static str> for AccountError {
    fn from(code: &'static str) -> Self {
        Self::Code(code)
    }
}

#[derive(Default)]
pub struct ClientAccount {
    session: Mutex<AccountState>,
    client: OnceLock<Result<Client>>,
}

#[derive(Default)]
struct AccountState {
    access: Option<Access>,
    cache: Option<AccountCache>,
    storage: Option<(String, StorageConfig)>,
}

#[derive(Deserialize, Serialize)]
struct AccountCache {
    account_id: String,
    info: Option<Cached<AccountInfo>>,
    redemptions: Option<Cached<Vec<Redemption>>>,
}

#[derive(Deserialize, Serialize)]
struct Cached<T> {
    checked_at: u64,
    value: Option<T>,
}

impl<T> Cached<T> {
    fn fresh(&self) -> bool {
        now()
            .checked_sub(self.checked_at)
            .is_some_and(|age| age < ACCOUNT_CACHE_TTL)
    }
}

struct Access {
    token: String,
    expires_at: u64,
    session_expires_at: u64,
    account_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    session_expires_at: u64,
    token_type: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedSession {
    refresh_token: String,
    session_expires_at: u64,
    account_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    id: String,
    name: Option<String>,
    email: Option<String>,
    display_name: Option<String>,
    avatar: Option<String>,
    email_verified: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Subscription {
    quota_bytes: u64,
    active: bool,
    expires_at: Option<AccountTimestamp>,
    email_verified: bool,
    #[serde(default = "requires_email_by_default")]
    email_verification_required: bool,
}

fn requires_email_by_default() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    used_bytes: u64,
    quota_bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum AccountTimestamp {
    Unix(u64),
    Date(String),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Redemption {
    years: u64,
    redeemed_time: AccountTimestamp,
}

#[derive(Deserialize)]
struct Redeemed {
    years: u64,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct AccountInfo {
    profile: Profile,
    subscription: Option<Subscription>,
    usage: Option<Usage>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfig {
    endpoint: String,
    bucket: String,
    region: String,
    access_key_id: String,
    secret_access_key: String,
    force_path_style: bool,
    concurrent_reqs: usize,
    credentials_expire_at: u64,
    #[serde(skip_deserializing)]
    prefix: &'static str,
}

#[derive(Deserialize)]
struct Envelope {
    status: String,
    code: String,
    #[serde(default)]
    data: Value,
    #[serde(default)]
    msg: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn private(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.mode() & 0o077 == 0 && metadata.uid() == unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        // Windows inherits permissions from the user's local app data directory.
        !metadata.file_type().is_symlink()
    }
}

fn session_dir(path: &Path, create: bool) -> Result<Option<&Path>> {
    let dir = path.parent().ok_or(STORAGE_ERROR)?;
    if create {
        let mut builder = DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(dir).map_err(|_| STORAGE_ERROR)?;
    }
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() && private(&meta) => Ok(Some(dir)),
        Err(error) if error.kind() == ErrorKind::NotFound && !create => Ok(None),
        _ => Err(STORAGE_ERROR.into()),
    }
}

fn load(path: &Path) -> Result<Option<SavedSession>> {
    load_private(path, MAX_SESSION_BYTES)
}

fn load_private<T: DeserializeOwned>(path: &Path, max_bytes: u64) -> Result<Option<T>> {
    if session_dir(path, false)?.is_none() {
        return Ok(None);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(STORAGE_ERROR.into()),
    };
    let meta = file.metadata().map_err(|_| STORAGE_ERROR)?;
    if !meta.is_file() || !private(&meta) || meta.len() > max_bytes {
        return Err(STORAGE_ERROR.into());
    }
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| STORAGE_ERROR)?;
    if bytes.len() as u64 > max_bytes {
        return Err(STORAGE_ERROR.into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| STORAGE_ERROR.into())
}

fn clear(path: &Path) -> Result<()> {
    if session_dir(path, false)?.is_none() {
        return Ok(());
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err(STORAGE_ERROR.into()),
    }
}

fn save(path: &Path, session: &SavedSession) -> Result<()> {
    save_private(path, session, MAX_SESSION_BYTES)
}

fn save_private(path: &Path, value: &impl Serialize, max_bytes: u64) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| STORAGE_ERROR)?;
    if bytes.len() as u64 > max_bytes {
        return Err(STORAGE_ERROR.into());
    }
    let dir = session_dir(path, true)?.ok_or(STORAGE_ERROR)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|_| STORAGE_ERROR)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| STORAGE_ERROR)?;
    }
    temp.write_all(&bytes).map_err(|_| STORAGE_ERROR)?;
    temp.as_file().sync_all().map_err(|_| STORAGE_ERROR)?;
    temp.persist(path).map_err(|_| STORAGE_ERROR)?;
    Ok(())
}

fn validate_tokens(tokens: &Tokens) -> Result<()> {
    if tokens.token_type != "Bearer"
        || tokens.expires_at <= now()
        || tokens.session_expires_at < tokens.expires_at
        || [&tokens.access_token, &tokens.refresh_token]
            .iter()
            .any(|token| {
                token.is_empty()
                    || token.len() > 4096
                    || token.bytes().any(|byte| !byte.is_ascii_graphic())
            })
    {
        return Err("invalid_response".into());
    }
    Ok(())
}

fn persist(
    path: &Path,
    access: &mut Option<Access>,
    tokens: Tokens,
    account_id: String,
) -> Result<()> {
    let saved = SavedSession {
        refresh_token: tokens.refresh_token,
        session_expires_at: tokens.session_expires_at,
        account_id: account_id.clone(),
    };
    save(path, &saved)?;
    *access = Some(Access {
        token: tokens.access_token,
        expires_at: tokens.expires_at,
        session_expires_at: tokens.session_expires_at,
        account_id,
    });
    Ok(())
}

fn client() -> Result<Client> {
    Client::builder()
        .redirect(Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "network".into())
}

struct Service<'a> {
    origin: &'a str,
    client: &'a Client,
    session: PathBuf,
}

impl Service<'_> {
    fn cache_path(&self) -> PathBuf {
        self.session.with_file_name("cache.json")
    }

    fn clear_session(&self) -> Result<()> {
        clear(&self.session)?;
        let _ = clear(&self.cache_path());
        Ok(())
    }

    fn current_account(&self, access: &Option<Access>) -> Result<Option<String>> {
        if let Some(access) = access {
            return Ok((access.session_expires_at > now()).then(|| access.account_id.clone()));
        }
        Ok(load(&self.session)?
            .filter(|saved| saved.session_expires_at > now())
            .map(|saved| saved.account_id))
    }

    fn cache<'a>(&self, state: &'a mut AccountState, account_id: &str) -> &'a mut AccountCache {
        if state
            .cache
            .as_ref()
            .is_none_or(|cache| cache.account_id != account_id)
        {
            state.cache = Some(
                load_private::<AccountCache>(&self.cache_path(), MAX_CACHE_BYTES)
                    .ok()
                    .flatten()
                    .filter(|cache| {
                        cache.account_id == account_id
                            && cache
                                .info
                                .as_ref()
                                .and_then(|entry| entry.value.as_ref())
                                .is_none_or(|info| info.profile.id == account_id)
                    })
                    .unwrap_or_else(|| AccountCache {
                        account_id: account_id.to_owned(),
                        info: None,
                        redemptions: None,
                    }),
            );
        }
        state.cache.as_mut().unwrap()
    }

    fn save_cache(&self, state: &AccountState) {
        if let Some(cache) = &state.cache {
            let _ = save_private(&self.cache_path(), cache, MAX_CACHE_BYTES);
        }
    }

    async fn send<T: DeserializeOwned>(
        &self,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> Result<T> {
        let method = if body.is_some() {
            Method::POST
        } else {
            Method::GET
        };
        let mut request = self
            .client
            .request(method, format!("{}{API}/{path}", self.origin));
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        if let Some(body) = &body {
            request = request
                .header("Content-Type", "application/json")
                .body(body.to_string());
        }
        let mut response = request.send().await.map_err(|error| {
            if error.is_connect() {
                "network"
            } else {
                "response_lost"
            }
        })?;
        let status = response.status();
        if status.as_u16() == 429 {
            return Err("rate_limited".into());
        }
        if status.as_u16() == 503 {
            return Err("temporarily_unavailable".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "response_lost")? {
            if bytes.len() + chunk.len() > 256 * 1024 {
                return Err("invalid_response".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let reply: Envelope = serde_json::from_slice(&bytes).map_err(|_| "invalid_response")?;
        match (status.as_u16(), reply.code.as_str()) {
            (200..=299, "ok") if reply.status == "ok" => {
                serde_json::from_value(reply.data).map_err(|_| "invalid_response".into())
            }
            (400, "redemption_failed") if path == "redeem" => {
                let mut message = reply.msg;
                let code = body
                    .as_ref()
                    .and_then(|body| body["activationCode"].as_str());
                for secret in [code, bearer].into_iter().flatten() {
                    if !secret.is_empty() {
                        message = message.replace(secret, "[已隐藏]");
                    }
                }
                Err(AccountError::Redemption {
                    code: "redemption_failed",
                    message: message.trim().chars().take(300).collect(),
                })
            }
            (401, "login_failed") => Err("login_failed".into()),
            (401, "invalid_token") => Err("invalid_token".into()),
            (401, "invalid_refresh_token") => Err("invalid_refresh_token".into()),
            (403, "email_unverified") => Err("email_unverified".into()),
            (403, "subscription_required") => Err("subscription_required".into()),
            _ => Err("invalid_response".into()),
        }
    }

    async fn refresh(&self, access: &mut Option<Access>) -> Result<()> {
        *access = None;
        let Some(saved) = load(&self.session)? else {
            return Err("signed_out".into());
        };
        if saved.session_expires_at <= now() {
            self.clear_session()?;
            return Err("signed_out".into());
        }
        let result = self
            .send::<Tokens>(
                "refresh",
                None,
                Some(json!({ "refreshToken": saved.refresh_token })),
            )
            .await;
        let tokens = match result {
            Ok(tokens)
                if validate_tokens(&tokens).is_ok()
                    && tokens.session_expires_at <= saved.session_expires_at =>
            {
                tokens
            }
            Err(
                error @ AccountError::Code("network" | "temporarily_unavailable" | "rate_limited"),
            ) => return Err(error),
            // The server might have consumed the old credential before the connection failed.
            _ => {
                self.clear_session()?;
                return Err("signed_out".into());
            }
        };
        if let Err(error) = persist(&self.session, access, tokens, saved.account_id) {
            self.clear_session()?;
            return Err(error);
        }
        Ok(())
    }

    async fn authorized<T: DeserializeOwned>(
        &self,
        access: &mut Option<Access>,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        if access
            .as_ref()
            .is_none_or(|entry| entry.expires_at <= now().saturating_add(30))
        {
            self.refresh(access).await?;
        }
        let result = self
            .send(
                path,
                access.as_ref().map(|entry| entry.token.as_str()),
                body.clone(),
            )
            .await;
        if !matches!(result, Err(AccountError::Code("invalid_token"))) {
            return result;
        }
        self.refresh(access).await?;
        let result = self
            .send(
                path,
                access.as_ref().map(|entry| entry.token.as_str()),
                body,
            )
            .await;
        if matches!(result, Err(AccountError::Code("invalid_token"))) {
            *access = None;
            self.clear_session()?;
            return Err("signed_out".into());
        }
        result
    }

    fn require_account(&self, access: &Option<Access>, account_id: &str) -> Result<()> {
        let matches = match access {
            Some(access) => access.account_id == account_id,
            None => load(&self.session)?.ok_or("signed_out")?.account_id == account_id,
        };
        if !matches {
            return Err("account_changed".into());
        }
        Ok(())
    }

    async fn profile(&self, access: &mut Option<Access>) -> Result<AccountInfo> {
        let profile: Profile = self.authorized(access, "me", None).await?;
        if !access
            .as_ref()
            .is_some_and(|entry| entry.account_id == profile.id)
        {
            self.clear_session()?;
            *access = None;
            return Err("signed_out".into());
        }
        self.account_info(access, profile).await
    }

    async fn status(&self, state: &mut AccountState, force: bool) -> Result<Option<AccountInfo>> {
        let Some(account_id) = self.current_account(&state.access)? else {
            *state = AccountState::default();
            self.clear_session()?;
            return Ok(None);
        };
        let entry = self.cache(state, &account_id).info.as_ref();
        let previous = entry.and_then(|entry| entry.value.clone());
        if !force && entry.is_some_and(Cached::fresh) {
            return previous
                .map(Some)
                .ok_or_else(|| "temporarily_unavailable".into());
        }
        match self.profile(&mut state.access).await {
            Ok(info) => {
                self.cache(state, &info.profile.id).info = Some(Cached {
                    checked_at: now(),
                    value: Some(info.clone()),
                });
                self.save_cache(state);
                Ok(Some(info))
            }
            Err(AccountError::Code("signed_out")) => {
                *state = AccountState::default();
                Ok(None)
            }
            Err(error) => {
                if self.current_account(&state.access)?.as_deref() != Some(account_id.as_str()) {
                    *state = AccountState::default();
                    return Err(error);
                }
                self.cache(state, &account_id).info = Some(Cached {
                    checked_at: now(),
                    value: previous.clone(),
                });
                self.save_cache(state);
                if !force && previous.is_some() {
                    Ok(previous)
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn redemptions(
        &self,
        state: &mut AccountState,
        account_id: &str,
        force: bool,
    ) -> Result<Vec<Redemption>> {
        let current = self.current_account(&state.access)?.ok_or("signed_out")?;
        if current != account_id {
            return Err("account_changed".into());
        }
        let entry = self.cache(state, account_id).redemptions.as_ref();
        let previous = entry.and_then(|entry| entry.value.clone());
        if !force && entry.is_some_and(Cached::fresh) {
            return previous.ok_or_else(|| "temporarily_unavailable".into());
        }
        let result: Result<Vec<Redemption>> = self
            .authorized(&mut state.access, "redemptions", None)
            .await;
        if matches!(&result, Err(AccountError::Code("signed_out"))) {
            *state = AccountState::default();
            return result;
        }
        let current = self.current_account(&state.access)?.ok_or("signed_out")?;
        if current != account_id {
            return Err("account_changed".into());
        }
        self.cache(state, account_id).redemptions = Some(Cached {
            checked_at: now(),
            value: result.as_ref().ok().cloned().or_else(|| previous.clone()),
        });
        self.save_cache(state);
        if force {
            result
        } else {
            result.or_else(|error| previous.ok_or(error))
        }
    }

    async fn storage_config(
        &self,
        state: &mut AccountState,
        account_id: &str,
    ) -> Result<StorageConfig> {
        self.require_account(&state.access, account_id)?;
        let minimum_expiry = now().saturating_add(60);
        if let Some((id, config)) = &state.storage {
            if id == account_id
                && config.credentials_expire_at > minimum_expiry
                && state
                    .access
                    .as_ref()
                    .is_some_and(|access| access.expires_at > minimum_expiry)
            {
                return Ok(config.clone());
            }
        }
        let mut config: StorageConfig = self
            .authorized(&mut state.access, "storage-config", Some(json!({})))
            .await?;
        self.require_account(&state.access, account_id)?;
        if config.credentials_expire_at <= minimum_expiry {
            return Err("storage_credentials_expiring".into());
        }
        if !reqwest::Url::parse(&config.endpoint).is_ok_and(|endpoint| endpoint.scheme() == "https")
            || config.region.trim().is_empty()
            || config.access_key_id.trim().is_empty()
            || config.secret_access_key.is_empty()
        {
            return Err("invalid_response".into());
        }
        config.prefix = SYNC_PREFIX;
        state.storage = Some((account_id.to_owned(), config.clone()));
        Ok(config)
    }

    async fn login(
        &self,
        access: &mut Option<Access>,
        username: &str,
        password: &str,
    ) -> Result<AccountInfo> {
        if access.is_some() || load(&self.session)?.is_some() {
            return Err("already_signed_in".into());
        }
        let tokens: Tokens = self
            .send(
                "login",
                None,
                Some(json!({ "username": username, "password": password })),
            )
            .await?;
        validate_tokens(&tokens)?;
        let profile: Profile = self.send("me", Some(&tokens.access_token), None).await?;
        if profile.id.trim().is_empty() {
            return Err("invalid_response".into());
        }
        persist(&self.session, access, tokens, profile.id.clone())?;
        self.account_info(access, profile).await
    }

    async fn account_info(
        &self,
        access: &mut Option<Access>,
        profile: Profile,
    ) -> Result<AccountInfo> {
        let subscription: Option<Subscription> =
            match self.authorized(access, "subscription", None).await {
                Ok(subscription) => Some(subscription),
                Err(error @ AccountError::Code("signed_out")) => return Err(error),
                Err(_) => None,
            };
        let usage = if subscription.as_ref().is_some_and(|plan| {
            plan.active && (!plan.email_verification_required || plan.email_verified)
        }) {
            match self.authorized(access, "usage", None).await {
                Ok(usage) => Some(usage),
                Err(error @ AccountError::Code("signed_out")) => return Err(error),
                Err(_) => None,
            }
        } else {
            None
        };
        Ok(AccountInfo {
            profile,
            subscription,
            usage,
        })
    }

    async fn redeem(&self, access: &mut Option<Access>, code: &str) -> Result<u64> {
        let redeemed: Redeemed = self
            .authorized(access, "redeem", Some(json!({ "activationCode": code })))
            .await?;
        Ok(redeemed.years)
    }

    async fn logout(&self, access: &mut Option<Access>, everywhere: bool) -> Result<bool> {
        let endpoint = if everywhere { "logout-all" } else { "logout" };
        let revoked = self
            .authorized::<()>(access, endpoint, Some(json!({})))
            .await;
        if everywhere {
            revoked.as_ref().map_err(Clone::clone)?;
        }
        *access = None;
        self.clear_session()?;
        Ok(revoked.is_ok())
    }
}

// 下面是各端共用的账号操作。会话文件的位置由调用方决定：
// App 放在自己的私有数据目录，网页客户端放在部署时指定的配置目录。
impl ClientAccount {
    pub fn session_path(dir: &Path) -> PathBuf {
        dir.join(".account").join("session.json")
    }

    fn service(&self, session: PathBuf) -> Result<Service<'_>> {
        let client = self
            .client
            .get_or_init(client)
            .as_ref()
            .map_err(Clone::clone)?;
        Ok(Service {
            origin: API_ORIGIN,
            client,
            session,
        })
    }

    pub async fn status(&self, session: PathBuf, force: bool) -> Result<Option<AccountInfo>> {
        let mut state = self.session.lock().await;
        self.service(session)?.status(&mut state, force).await
    }

    pub async fn login(&self, session: PathBuf, username: &str, password: &str) -> Result<AccountInfo> {
        let username = username.trim();
        if username.len() > 320
            || !username.contains('@')
            || username.chars().any(char::is_whitespace)
            || password.is_empty()
            || password.len() > 4096
        {
            return Err("invalid_input".into());
        }
        let mut state = self.session.lock().await;
        let service = self.service(session)?;
        let info = service.login(&mut state.access, username, password).await?;
        state.cache = Some(AccountCache {
            account_id: info.profile.id.clone(),
            info: Some(Cached {
                checked_at: now(),
                value: Some(info.clone()),
            }),
            redemptions: None,
        });
        service.save_cache(&state);
        state.storage = None;
        Ok(info)
    }

    pub async fn logout(&self, session: PathBuf, account_id: &str, everywhere: bool) -> Result<bool> {
        let mut state = self.session.lock().await;
        let service = self.service(session)?;
        service.require_account(&state.access, account_id)?;
        let revoked = service.logout(&mut state.access, everywhere).await?;
        *state = AccountState::default();
        Ok(revoked)
    }

    pub async fn redeem(&self, session: PathBuf, account_id: &str, activation_code: &str) -> Result<u64> {
        let code = activation_code.trim();
        if code.is_empty() || code.len() > 4096 || code.chars().any(char::is_control) {
            return Err("invalid_input".into());
        }
        let mut state = self.session.lock().await;
        let service = self.service(session)?;
        service.require_account(&state.access, account_id)?;
        let years = service.redeem(&mut state.access, code).await?;
        let cache = service.cache(&mut state, account_id);
        cache.info = None;
        cache.redemptions = None;
        service.save_cache(&state);
        state.storage = None;
        Ok(years)
    }

    pub async fn redemptions(&self, session: PathBuf, account_id: &str, force: bool) -> Result<Vec<Redemption>> {
        let mut state = self.session.lock().await;
        self.service(session)?
            .redemptions(&mut state, account_id, force)
            .await
    }

    pub async fn sync_config(&self, session: PathBuf, account_id: &str) -> Result<StorageConfig> {
        let mut state = self.session.lock().await;
        self.service(session)?
            .storage_config(&mut state, account_id)
            .await
    }

    // App 在提交同步结果前用它确认账号没变；网页客户端在同步开始时取凭据就已校验账号
    #[cfg_attr(not(feature = "app"), allow(dead_code))]
    pub async fn check_sync(&self, session: PathBuf, account_id: &str) -> Result<()> {
        let state = self.session.lock().await;
        self.service(session)?
            .require_account(&state.access, account_id)
    }
}

pub fn portal_url(page: &str) -> Result<String> {
    let path = match page {
        "register" => "/signup/app-customers",
        "reset" => "/forget/app-customers",
        "purchase" => "/purchase",
        "verify" | "account" => "/",
        _ => return Err("invalid_input".into()),
    };
    Ok(format!("{PORTAL}{path}"))
}

// App 的 Tauri 命令：只检查调用窗口、取 App 的私有目录，然后交给上面的共用方法。
#[cfg(feature = "app")]
fn app_session(window: &tauri::WebviewWindow) -> Result<PathBuf> {
    use tauri::Manager;
    if !matches!(window.label(), "main" | "settings") {
        return Err("not_allowed".into());
    }
    let dir = window
        .path()
        .app_local_data_dir()
        .map_err(|_| STORAGE_ERROR)?;
    Ok(ClientAccount::session_path(&dir))
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn account_status(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ClientAccount>,
    force: Option<bool>,
) -> Result<Option<AccountInfo>> {
    state.status(app_session(&window)?, force.unwrap_or(false)).await
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn account_login(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ClientAccount>,
    username: String,
    password: String,
) -> Result<AccountInfo> {
    state.login(app_session(&window)?, &username, &password).await
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn account_logout(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ClientAccount>,
    account_id: String,
    everywhere: bool,
) -> Result<bool> {
    state.logout(app_session(&window)?, &account_id, everywhere).await
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn account_redeem(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ClientAccount>,
    account_id: String,
    activation_code: String,
) -> Result<u64> {
    state.redeem(app_session(&window)?, &account_id, &activation_code).await
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn account_redemptions(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ClientAccount>,
    account_id: String,
    force: Option<bool>,
) -> Result<Vec<Redemption>> {
    state
        .redemptions(app_session(&window)?, &account_id, force.unwrap_or(false))
        .await
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn account_sync_config(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ClientAccount>,
    account_id: String,
) -> Result<StorageConfig> {
    if window.label() != "main" {
        return Err("not_allowed".into());
    }
    state.sync_config(app_session(&window)?, &account_id).await
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn account_check_sync(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ClientAccount>,
    account_id: String,
) -> Result<()> {
    if window.label() != "main" {
        return Err("not_allowed".into());
    }
    state.check_sync(app_session(&window)?, &account_id).await
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn account_open_portal(window: tauri::WebviewWindow, page: &str) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    app_session(&window)?;
    window
        .opener()
        .open_url(portal_url(page)?, None::<&str>)
        .map_err(|_| "open_failed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        thread,
    };

    fn mock_api(
        routes: Vec<(
            &'static str,
            Option<&'static str>,
            Option<Value>,
            u16,
            Value,
        )>,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = thread::spawn(move || {
            for (endpoint, bearer, payload, status, response) in routes {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(4)))
                    .unwrap();
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let method = if payload.is_some() { "POST" } else { "GET" };
                assert_eq!(line.trim(), format!("{method} {API}/{endpoint} HTTP/1.1"));
                let mut length = 0;
                let mut authorization = None;
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    let (name, value) = line.trim().split_once(':').unwrap();
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                    if name.eq_ignore_ascii_case("authorization") {
                        authorization = Some(value.trim().to_owned());
                    }
                }
                assert_eq!(authorization, bearer.map(|value| format!("Bearer {value}")));
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                if let Some(payload) = payload {
                    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), payload);
                }
                let body = response.to_string();
                write!(stream, "HTTP/1.1 {status} Test\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (origin, task)
    }

    fn ok(data: Value) -> Value {
        json!({ "status": "ok", "code": "ok", "data": data })
    }
    fn error(code: &str) -> Value {
        json!({ "status": "error", "code": code, "data": null })
    }

    #[tokio::test]
    async fn caches_account_snapshots_across_launch_and_refreshes_when_requested_or_stale() {
        let mut routes = Vec::new();
        for used in [1024, 2048, 4096] {
            routes.extend([
                ("me", Some("access"), None, 200, ok(json!({"id": "one", "emailVerified": false}))),
                ("subscription", Some("access"), None, 200, ok(json!({"active": true, "quotaBytes": 10000, "emailVerified": false, "emailVerificationRequired": false}))),
                ("usage", Some("access"), None, 200, ok(json!({"usedBytes": used, "quotaBytes": 10000}))),
            ]);
        }
        let (origin, finished) = mock_api(routes);
        let directory = tempfile::tempdir().unwrap();
        let http = client().unwrap();
        let service = Service {
            origin: &origin,
            client: &http,
            session: directory.path().join(".account/session.json"),
        };
        let mut state = AccountState {
            access: Some(Access {
                token: "access".into(),
                expires_at: now() + 3600,
                session_expires_at: now() + 86_400 * 2,
                account_id: "one".into(),
            }),
            ..Default::default()
        };
        save(
            &service.session,
            &SavedSession {
                refresh_token: "synthetic-refresh".into(),
                session_expires_at: now() + 86_400 * 2,
                account_id: "one".into(),
            },
        )
        .unwrap();
        let bytes = |info: Option<AccountInfo>| info.unwrap().usage.unwrap().used_bytes;
        assert_eq!(
            bytes(service.status(&mut state, false).await.unwrap()),
            1024
        );
        let mut reopened = AccountState::default();
        assert_eq!(
            bytes(service.status(&mut reopened, false).await.unwrap()),
            1024
        );
        assert!(reopened.access.is_none());
        state
            .cache
            .as_mut()
            .unwrap()
            .info
            .as_mut()
            .unwrap()
            .checked_at -= ACCOUNT_CACHE_TTL - 10;
        assert_eq!(
            bytes(service.status(&mut state, false).await.unwrap()),
            1024
        );
        assert_eq!(bytes(service.status(&mut state, true).await.unwrap()), 2048);
        state
            .cache
            .as_mut()
            .unwrap()
            .info
            .as_mut()
            .unwrap()
            .checked_at -= ACCOUNT_CACHE_TTL;
        assert_eq!(
            bytes(service.status(&mut state, false).await.unwrap()),
            4096
        );
        let persisted = fs::read_to_string(service.cache_path()).unwrap();
        assert!(!persisted.contains("synthetic-refresh") && !persisted.contains("token"));
        service.clear_session().unwrap();
        assert!(!service.cache_path().exists());
        assert!(service
            .status(&mut AccountState::default(), false)
            .await
            .unwrap()
            .is_none());
        finished.join().unwrap();
    }

    #[tokio::test]
    async fn keeps_redemption_history_for_the_authenticated_account_and_throttles_failed_refreshes()
    {
        let records = |years| ok(json!([{"years": years, "redeemedTime": 1800000000}]));
        let (origin, finished) = mock_api(vec![
            ("redemptions", Some("access"), None, 200, records(1)),
            (
                "redemptions",
                Some("access"),
                None,
                503,
                error("temporarily_unavailable"),
            ),
            ("redemptions", Some("access"), None, 200, records(2)),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let http = client().unwrap();
        let service = Service {
            origin: &origin,
            client: &http,
            session: directory.path().join(".account/session.json"),
        };
        let mut state = AccountState {
            access: Some(Access {
                token: "access".into(),
                expires_at: now() + 3600,
                session_expires_at: now() + 7200,
                account_id: "one".into(),
            }),
            ..Default::default()
        };
        save(
            &service.session,
            &SavedSession {
                refresh_token: "synthetic-refresh".into(),
                session_expires_at: now() + 7200,
                account_id: "one".into(),
            },
        )
        .unwrap();
        assert_eq!(
            service.redemptions(&mut state, "one", false).await.unwrap()[0].years,
            1
        );
        let mut reopened = AccountState::default();
        assert_eq!(
            service
                .redemptions(&mut reopened, "one", false)
                .await
                .unwrap()[0]
                .years,
            1
        );
        assert!(matches!(
            service.redemptions(&mut reopened, "two", false).await,
            Err(AccountError::Code("account_changed"))
        ));
        state
            .cache
            .as_mut()
            .unwrap()
            .redemptions
            .as_mut()
            .unwrap()
            .checked_at -= ACCOUNT_CACHE_TTL;
        assert_eq!(
            service.redemptions(&mut state, "one", false).await.unwrap()[0].years,
            1
        );
        assert_eq!(
            service.redemptions(&mut state, "one", false).await.unwrap()[0].years,
            1
        );
        assert_eq!(
            service.redemptions(&mut state, "one", true).await.unwrap()[0].years,
            2
        );
        finished.join().unwrap();
    }

    #[tokio::test]
    async fn scopes_cloud_credentials_to_account_and_reuses_them_until_near_expiration() {
        let grant = |key: &str| {
            ok(
                json!({"endpoint": "https://s3.example.test", "bucket": "account-bucket", "region": "us-east-1",
                "accessKeyId": key, "secretAccessKey": "synthetic-secret", "forcePathStyle": true, "concurrentReqs": 3,
                "credentialsExpireAt": now() + 600, "prefix": "repo/foreign"}),
            )
        };
        let (origin, finished) = mock_api(vec![
            (
                "storage-config",
                Some("access"),
                Some(json!({})),
                200,
                grant("first"),
            ),
            (
                "storage-config",
                Some("access"),
                Some(json!({})),
                200,
                grant("second"),
            ),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let http = client().unwrap();
        let service = Service {
            origin: &origin,
            client: &http,
            session: directory.path().join("session.json"),
        };
        let mut state = AccountState {
            access: Some(Access {
                token: "access".into(),
                expires_at: now() + 3600,
                session_expires_at: now() + 7200,
                account_id: "one".into(),
            }),
            ..Default::default()
        };
        let first = service.storage_config(&mut state, "one").await.unwrap();
        assert_eq!(first.prefix, "repo/qingyu/v1");
        assert_eq!(
            service
                .storage_config(&mut state, "one")
                .await
                .unwrap()
                .access_key_id,
            "first"
        );
        assert!(matches!(
            service.storage_config(&mut state, "two").await,
            Err(AccountError::Code("account_changed"))
        ));
        state.storage.as_mut().unwrap().1.credentials_expire_at = now() + 30;
        assert_eq!(
            service
                .storage_config(&mut state, "one")
                .await
                .unwrap()
                .access_key_id,
            "second"
        );
        assert!(!service.session.exists());
        finished.join().unwrap();
    }

    #[tokio::test]
    async fn authenticates_restores_and_rotates_once_then_revokes_the_current_session() {
        let deadline = now() + 3600 * 24;
        let tokens = |a, r| {
            ok(
                json!({"accessToken": a, "refreshToken": r, "tokenType": "Bearer", "expiresAt": deadline - 60, "sessionExpiresAt": deadline}),
            )
        };
        let profile = ok(
            json!({"id": "immutable-id", "name": "Fixture", "email": "synthetic@example.invalid", "emailVerified": true}),
        );
        let subscription = ok(
            json!({"quotaBytes": 10_000_000_000_u64, "maxObjectBytes": 1024, "active": true, "expiresAt": deadline, "emailVerified": true}),
        );
        let usage = ok(json!({"usedBytes": 2048, "quotaBytes": 10_000_000_000_u64}));
        let user = "synthetic@example.invalid";
        let (origin, done) = mock_api(vec![
            (
                "login",
                None,
                Some(json!({"username": user, "password": "fixture-password"})),
                200,
                tokens("first", "refresh-first"),
            ),
            ("me", Some("first"), None, 200, profile.clone()),
            (
                "subscription",
                Some("first"),
                None,
                200,
                subscription.clone(),
            ),
            ("usage", Some("first"), None, 200, usage.clone()),
            (
                "refresh",
                None,
                Some(json!({"refreshToken": "refresh-first"})),
                200,
                tokens("second", "refresh-second"),
            ),
            ("me", Some("second"), None, 401, error("invalid_token")),
            (
                "refresh",
                None,
                Some(json!({"refreshToken": "refresh-second"})),
                200,
                tokens("third", "refresh-third"),
            ),
            ("me", Some("third"), None, 200, profile.clone()),
            (
                "subscription",
                Some("third"),
                None,
                200,
                subscription.clone(),
            ),
            ("usage", Some("third"), None, 200, usage.clone()),
            ("me", Some("third"), None, 200, profile),
            ("subscription", Some("third"), None, 200, subscription),
            ("usage", Some("third"), None, 200, usage),
            (
                "logout",
                Some("third"),
                Some(json!({})),
                200,
                ok(Value::Null),
            ),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let session = directory.path().join(".account/session.json");
        assert!(load(&session).unwrap().is_none());
        let http = client().unwrap();
        let service = Service {
            origin: &origin,
            client: &http,
            session: session.clone(),
        };
        let mut initial = None;
        let info = service
            .login(&mut initial, user, "fixture-password")
            .await
            .unwrap();
        assert_eq!(info.profile.id, "immutable-id");
        let plan = info.subscription.unwrap();
        assert!(plan.active && plan.email_verification_required);
        assert_eq!(info.usage.unwrap().used_bytes, 2048);
        let saved = fs::read_to_string(&session).unwrap();
        assert!(saved.contains("refresh-first"));
        assert!(!saved.contains("accessToken") && !saved.contains("fixture-password"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&session).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(session.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }

        let restarted = Mutex::new(None);
        let read = || async {
            service
                .profile(&mut *restarted.lock().await)
                .await
                .map(|info| info.profile.id)
        };
        let (a, b) = tokio::join!(read(), read());
        assert_eq!(a.unwrap(), "immutable-id");
        assert_eq!(b.unwrap(), "immutable-id");
        assert_eq!(
            load(&session).unwrap().unwrap().refresh_token,
            "refresh-third"
        );
        assert!(service
            .logout(&mut *restarted.lock().await, false)
            .await
            .unwrap());
        assert!(!session.exists());
        done.join().unwrap();
    }

    #[tokio::test]
    async fn redeems_for_the_bound_account_and_requires_confirmed_global_revocation() {
        let profile = ok(json!({"id": "fixture", "emailVerified": false}));
        let plan = |active: bool| {
            ok(json!({
                "quotaBytes": 10_000_000_000_u64,
                "expiresAt": active.then_some("2027-11-17T00:00:00Z"),
                "active": active, "emailVerified": false, "emailVerificationRequired": false
            }))
        };
        let body = || Some(json!({"activationCode": "fixture-code"}));
        let (origin, done) = mock_api(vec![
            ("me", Some("access"), None, 200, profile.clone()),
            ("subscription", Some("access"), None, 200, plan(false)),
            (
                "redeem",
                Some("access"),
                body(),
                400,
                json!({
                    "status": "error", "code": "redemption_failed", "msg": "已使用 fixture-code"
                }),
            ),
            (
                "redeem",
                Some("access"),
                body(),
                200,
                ok(json!({"years": 1})),
            ),
            ("me", Some("access"), None, 200, profile),
            ("subscription", Some("access"), None, 200, plan(true)),
            (
                "usage",
                Some("access"),
                None,
                200,
                ok(json!({"usedBytes": 512, "quotaBytes": 10_000_000_000_u64})),
            ),
            (
                "redemptions",
                Some("access"),
                None,
                200,
                ok(json!([
                    {"id": "record", "years": 1, "redeemedTime": "2026-11-17T00:00:00Z"}
                ])),
            ),
            (
                "logout-all",
                Some("access"),
                Some(json!({})),
                503,
                error("temporarily_unavailable"),
            ),
            (
                "logout-all",
                Some("access"),
                Some(json!({})),
                200,
                ok(Value::Null),
            ),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let session = directory.path().join(".account/session.json");
        let http = client().unwrap();
        let service = Service {
            origin: &origin,
            client: &http,
            session: session.clone(),
        };
        let mut access = None;
        persist(
            &session,
            &mut access,
            Tokens {
                access_token: "access".into(),
                refresh_token: "refresh".into(),
                expires_at: now() + 3600,
                session_expires_at: now() + 7200,
                token_type: "Bearer".into(),
            },
            "fixture".into(),
        )
        .unwrap();

        assert_eq!(
            service.require_account(&access, "other"),
            Err("account_changed".into())
        );
        service.require_account(&access, "fixture").unwrap();
        let info = service.profile(&mut access).await.unwrap();
        assert!(info.usage.is_none());
        assert!(!info.subscription.unwrap().email_verification_required);
        assert_eq!(
            service.redeem(&mut access, "fixture-code").await,
            Err(AccountError::Redemption {
                code: "redemption_failed",
                message: "已使用 [已隐藏]".into(),
            })
        );
        assert_eq!(
            service.redeem(&mut access, "fixture-code").await.unwrap(),
            1
        );
        let info = service.profile(&mut access).await.unwrap();
        let subscription = serde_json::to_value(info.subscription.unwrap()).unwrap();
        assert_eq!(subscription["active"], true);
        assert_eq!(subscription["expiresAt"], "2027-11-17T00:00:00Z");
        assert_eq!(info.usage.unwrap().used_bytes, 512);
        let history: Vec<Redemption> = service
            .authorized(&mut access, "redemptions", None)
            .await
            .unwrap();
        assert_eq!(history[0].years, 1);
        assert_eq!(
            service.logout(&mut access, true).await,
            Err("temporarily_unavailable".into())
        );
        assert!(access.is_some() && load(&session).unwrap().is_some());
        assert!(service.logout(&mut access, true).await.unwrap());
        assert!(access.is_none() && !session.exists());
        done.join().unwrap();
    }

    #[tokio::test]
    async fn rejected_refresh_discards_the_credential_and_requires_sign_in() {
        let (origin, done) = mock_api(vec![(
            "refresh",
            None,
            Some(json!({"refreshToken": "revoked"})),
            401,
            error("invalid_refresh_token"),
        )]);
        let directory = tempfile::tempdir().unwrap();
        let session = directory.path().join(".account/session.json");
        save(
            &session,
            &SavedSession {
                refresh_token: "revoked".into(),
                account_id: "fixture".into(),
                session_expires_at: now() + 3600,
            },
        )
        .unwrap();
        let http = client().unwrap();
        let service = Service {
            origin: &origin,
            client: &http,
            session: session.clone(),
        };
        assert!(matches!(
            service.profile(&mut None).await,
            Err(AccountError::Code("signed_out"))
        ));
        assert!(!session.exists());
        done.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_exposed_sessions_and_never_follows_session_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let directory = tempfile::tempdir().unwrap();
        let real = directory.path().join("real/session.json");
        let session = SavedSession {
            refresh_token: "fixture-token".into(),
            account_id: "fixture".into(),
            session_expires_at: now() + 3600,
        };
        save(&real, &session).unwrap();
        let linked_dir = directory.path().join("linked");
        symlink(real.parent().unwrap(), &linked_dir).unwrap();
        let via_directory = linked_dir.join("session.json");
        assert!(load(&via_directory).is_err());
        assert!(save(&via_directory, &session).is_err());
        assert!(clear(&via_directory).is_err());
        let via_file = real.with_file_name("linked.json");
        symlink(&real, &via_file).unwrap();
        assert!(load(&via_file).is_err());
        assert!(load(&real).unwrap().is_some());
        fs::set_permissions(&real, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(&real).is_err());
    }
}
