use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::DateTime;
use md5::{Digest, Md5};
use reqwest::{Url, header::HeaderMap};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Managed {
    protocol_version: u32,
    provider: String,
    repository: String,
    config_version: u64,
    session_token: String,
}

impl StorageConfig {
    pub(super) fn same_storage(&self, other: &Self) -> bool {
        self.endpoint == other.endpoint
            && self.bucket == other.bucket
            && self.prefix == other.prefix
            && self.region == other.region
            && self.force_path_style == other.force_path_style
            && match (&self.managed, &other.managed) {
                (Some(a), Some(b)) => {
                    a.config_version == b.config_version
                        && a.repository == b.repository
                        && a.provider == b.provider
                }
                (None, None) => true,
                _ => false,
            }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConfigReply {
    endpoint: String,
    bucket: String,
    region: String,
    access_key_id: String,
    secret_access_key: String,
    force_path_style: bool,
    concurrent_reqs: Option<usize>,
    credentials_expire_at: AccountTimestamp,
    protocol_version: Option<u32>,
    credential_type: Option<String>,
    provider: Option<String>,
    prefix: Option<String>,
    repository: Option<String>,
    config_version: Option<u64>,
    session_token: Option<String>,
    read_only: Option<bool>,
    upload_mode: Option<String>,
    delete_mode: Option<String>,
}

fn rfc3339(value: &str) -> Result<u64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|date| date.timestamp().try_into().ok())
        .ok_or_else(|| "invalid_response".into())
}

fn https(value: &str) -> Result<Url> {
    Url::parse(value)
        .ok()
        .filter(|url| {
            url.scheme() == "https"
                && url.host().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
        })
        .ok_or_else(|| "invalid_response".into())
}

impl ConfigReply {
    pub(super) fn validate(self) -> Result<StorageConfig> {
        let invalid = || AccountError::Code("invalid_response");
        if https(&self.endpoint)?.query().is_some()
            || self.region.trim().is_empty()
            || self.access_key_id.trim().is_empty()
            || self.secret_access_key.is_empty()
        {
            return Err(invalid());
        }
        let (prefix, credentials_expire_at, managed) =
            match (self.protocol_version, self.credentials_expire_at) {
                (None, AccountTimestamp::Unix(expiry)) => (SYNC_PREFIX.into(), expiry, None),
                (Some(2), AccountTimestamp::Date(expiry)) => {
                    let prefix = self.prefix.ok_or_else(invalid)?;
                    let storage_id = prefix
                        .strip_prefix("users/")
                        .and_then(|id| id.strip_suffix('/'))
                        .ok_or_else(invalid)?;
                    if !uuid::Uuid::parse_str(storage_id)
                        .is_ok_and(|id| id.hyphenated().to_string() == storage_id)
                        || self.read_only != Some(true)
                        || self
                            .credential_type
                            .as_deref()
                            .is_some_and(|value| value != "sts")
                        || self.upload_mode.as_deref() != Some("presigned")
                        || self.delete_mode.as_deref() != Some("server")
                    {
                        return Err(invalid());
                    }
                    let managed = Managed {
                        protocol_version: 2,
                        provider: self
                            .provider
                            .filter(|value| matches!(value.as_str(), "tos" | "s3"))
                            .ok_or_else(invalid)?,
                        repository: self
                            .repository
                            .filter(|value| {
                                !value.trim().is_empty() && !value.chars().any(char::is_control)
                            })
                            .ok_or_else(invalid)?,
                        config_version: self
                            .config_version
                            .filter(|version| (1..=(1_u64 << 53) - 1).contains(version))
                            .ok_or_else(invalid)?,
                        session_token: self
                            .session_token
                            .filter(|value| {
                                !value.is_empty()
                                    && value.bytes().all(|byte| byte.is_ascii_graphic())
                            })
                            .ok_or_else(invalid)?,
                    };
                    (prefix, rfc3339(&expiry)?, Some(managed))
                }
                _ => return Err(invalid()),
            };
        Ok(StorageConfig {
            endpoint: self.endpoint,
            bucket: self.bucket,
            region: self.region,
            access_key_id: self.access_key_id,
            secret_access_key: self.secret_access_key,
            force_path_style: self.force_path_style,
            concurrent_reqs: self.concurrent_reqs.unwrap_or(2),
            credentials_expire_at,
            prefix,
            can_upload: None,
            storage_generation: 0,
            managed,
        })
    }
}

const MAX_OBJECT: usize = 64 * 1024 * 1024;

#[cfg(feature = "app")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Upload {
    #[serde(flatten)]
    pub object: UploadObject,
    pub account_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadObject {
    config_version: u64,
    storage_generation: Option<u64>,
    key: String,
    if_match: Option<String>,
    if_none_match: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Authorizations {
    config_version: u64,
    grants: Vec<Grant>,
}

const MAX_BATCH: usize = 100;
const AUTHORIZE: &str = "upload-authorize";
const COMPLETE: &str = "upload-complete";

#[derive(Clone, PartialEq, Eq)]
struct UploadScope {
    session: PathBuf,
    account_id: String,
    config_version: u64,
    generation: Option<u64>,
}

pub(super) struct QueuedUpload {
    scope: UploadScope,
    path: &'static str,
    key: String,
    item: Value,
    reply: tokio::sync::oneshot::Sender<Result<BatchReply>>,
}

enum BatchReply {
    Grant(Grant),
    Confirmed(bool),
    UnknownGrant,
}

fn take_upload_batch(queue: &mut Vec<QueuedUpload>) -> Vec<QueuedUpload> {
    let first = queue.remove(0);
    if first.separate_authorization() {
        return vec![first];
    }
    let mut seen = std::collections::HashSet::from([first.item_id().to_owned()]);
    let mut batch = vec![first];
    let mut index = 0;
    while index < queue.len() && batch.len() < MAX_BATCH {
        let entry = &queue[index];
        if entry.scope == batch[0].scope
            && entry.path == batch[0].path
            && !entry.separate_authorization()
            && seen.insert(entry.item_id().to_owned())
        {
            batch.push(queue.remove(index));
        } else {
            index += 1;
        }
    }
    batch
}

impl QueuedUpload {
    fn separate_authorization(&self) -> bool {
        self.path == AUTHORIZE
            && (self.key.ends_with("/vault-v2/manifest.json.enc")
                || self.key.ends_with("/miaoyan-key.json"))
    }

    fn item_id(&self) -> &str {
        if self.path == AUTHORIZE {
            &self.key
        } else {
            self.item.as_str().unwrap()
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Grant {
    id: String,
    key: String,
    url: String,
    expires_at: String,
    headers: HashMap<String, String>,
}

impl UploadObject {
    fn metadata(&self, bytes: &[u8]) -> Result<(Value, BTreeMap<&'static str, String>)> {
        if bytes.is_empty()
            || bytes.len() > MAX_OBJECT
            || self
                .if_none_match
                .as_deref()
                .is_some_and(|value| value != "*")
            || self.if_match.as_ref().is_some_and(|etag| {
                etag.is_empty()
                    || etag.len() > 256
                    || !etag.bytes().all(|byte| byte.is_ascii_graphic())
            })
            || self.if_match.is_some() && self.if_none_match.is_some()
        {
            return Err("invalid_request".into());
        }
        let digest = STANDARD.encode(Md5::digest(bytes));
        let mut object = json!({"key": self.key, "size": bytes.len(), "md5": digest});
        let mut headers = BTreeMap::from([
            ("content-length", bytes.len().to_string()),
            ("content-md5", digest),
        ]);
        if let Some(etag) = &self.if_match {
            object["ifMatch"] = json!(etag);
            headers.insert("if-match", etag.clone());
        }
        if let Some(condition) = &self.if_none_match {
            object["ifNoneMatch"] = json!(condition);
            headers.insert("if-none-match", condition.clone());
        }
        Ok((object, headers))
    }
}

impl Grant {
    fn validate(
        &self,
        key: &str,
        expected: &BTreeMap<&str, String>,
    ) -> Result<(Url, HeaderMap, u64)> {
        let invalid = || AccountError::Code("invalid_response");
        let url = https(&self.url)?;
        if self.key != key || self.id.is_empty() || url.query().is_none() {
            return Err(invalid());
        }
        let mut headers = HeaderMap::new();
        for (name, value) in &self.headers {
            let name =
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?;
            if name != "content-type" && expected.get(name.as_str()) != Some(value) {
                return Err(invalid());
            }
            if headers
                .insert(name, value.parse().map_err(|_| invalid())?)
                .is_some()
            {
                return Err(invalid());
            }
        }
        if headers.len() != expected.len() + 1 || !headers.contains_key("content-type") {
            return Err(invalid());
        }
        Ok((url, headers, rfc3339(&self.expires_at)?))
    }
}

fn batch_responses(
    path: &str,
    config_version: u64,
    batch: &[QueuedUpload],
    value: Value,
) -> Result<HashMap<String, BatchReply>> {
    let mut responses = HashMap::new();
    if path == AUTHORIZE {
        let reply: Authorizations =
            serde_json::from_value(value).map_err(|_| "invalid_response")?;
        if reply.config_version != config_version || reply.grants.len() != batch.len() {
            return Err("invalid_response".into());
        }
        let mut ids = std::collections::HashSet::new();
        for grant in reply.grants {
            if !ids.insert(grant.id.clone())
                || responses
                    .insert(grant.key.clone(), BatchReply::Grant(grant))
                    .is_some()
            {
                return Err("invalid_response".into());
            }
        }
    } else {
        let rows = value["results"].as_array().ok_or("invalid_response")?;
        for row in rows {
            let id = row["id"].as_str().ok_or("invalid_response")?;
            let reply = match row["state"].as_str() {
                Some("completed") => BatchReply::Confirmed(true),
                Some("issued" | "settled") => BatchReply::Confirmed(false),
                Some("unknown") => BatchReply::UnknownGrant,
                _ => return Err("invalid_response".into()),
            };
            if responses.insert(id.to_owned(), reply).is_some() {
                return Err("invalid_response".into());
            }
        }
    }
    if responses.len() != batch.len()
        || batch
            .iter()
            .any(|item| !responses.contains_key(item.item_id()))
    {
        return Err("invalid_response".into());
    }
    Ok(responses)
}

impl ClientAccount {
    async fn storage_operation<T: DeserializeOwned>(
        &self,
        session: PathBuf,
        account_id: &str,
        config_version: u64,
        generation: Option<u64>,
        keys: &[&str],
        path: &str,
        body: Value,
    ) -> Result<T> {
        let service = self.service(session)?;
        let mut rejected = None;
        for attempt in 0..2 {
            let token = {
                let mut state = self.session.lock().await;
                if let Some(generation) = generation {
                    self.watch_storage(generation)?;
                }
                service.require_account(&state.access, account_id)?;
                let (_, config) = state
                    .storage
                    .as_ref()
                    .filter(|(id, config)| {
                        id == account_id
                            && config
                                .managed
                                .as_ref()
                                .is_some_and(|managed| managed.config_version == config_version)
                    })
                    .ok_or("storage_config_changed")?;
                if keys.iter().any(|key| !key.starts_with(&config.prefix)) {
                    return Err("invalid_request".into());
                }
                if path == AUTHORIZE && config.can_upload == Some(false) {
                    return Err("storage_expired".into());
                }
                if state.access.as_ref().is_none_or(|entry| {
                    entry.expires_at <= now().saturating_add(30)
                        || rejected.as_ref() == Some(&entry.token)
                }) {
                    if let Err(error) = service.refresh(&mut state.access).await {
                        if matches!(error, AccountError::Code("signed_out")) {
                            state.storage = None;
                            self.invalidate_storage();
                        }
                        return Err(error);
                    }
                }
                state.access.as_ref().ok_or("signed_out")?.token.clone()
            };
            // Keep session rotation serialized, while grant requests and HEAD confirmation can overlap.
            let result = service.send(path, Some(&token), Some(body.clone())).await;
            let mut state = self.session.lock().await;
            if let Some(generation) = generation {
                self.watch_storage(generation)?;
            }
            service.require_account(&state.access, account_id)?;
            if matches!(result, Err(AccountError::Code("invalid_token"))) {
                if attempt == 0 {
                    rejected = Some(token);
                    continue;
                }
                if state
                    .access
                    .as_ref()
                    .is_some_and(|current| current.token == token)
                {
                    service.clear_session()?;
                    *state = AccountState::default();
                    self.invalidate_storage();
                    return Err("signed_out".into());
                }
            }
            if matches!(
                result,
                Err(AccountError::Code(
                    "storage_config_changed" | "storage_busy" | "session_expiring"
                ))
            ) {
                state.storage = None;
                self.invalidate_storage();
            }
            return result;
        }
        unreachable!("storage API has at most two attempts")
    }

    async fn queue_upload(
        &self,
        scope: UploadScope,
        path: &'static str,
        key: String,
        item: Value,
    ) -> tokio::sync::oneshot::Receiver<Result<BatchReply>> {
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let mut queue = self.upload_batches.lock().await;
        let schedule = queue.is_empty();
        queue.push(QueuedUpload {
            scope,
            path,
            key,
            item,
            reply,
        });
        if schedule {
            let account = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let mut entries = std::mem::take(&mut *account.upload_batches.lock().await);
                while !entries.is_empty() {
                    let batch = take_upload_batch(&mut entries);
                    let account = account.clone();
                    tokio::spawn(async move {
                        account.flush_uploads(batch).await;
                    });
                }
            });
        }
        receiver
    }

    async fn flush_uploads(&self, batch: Vec<QueuedUpload>) {
        let scope = &batch[0].scope;
        let path = batch[0].path;
        let field = if path == AUTHORIZE {
            "objects"
        } else {
            "grantIds"
        };
        let keys = batch
            .iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>();
        let body = json!({"configVersion":scope.config_version, field: batch.iter().map(|item| &item.item).collect::<Vec<_>>()});
        let operation = || {
            self.storage_operation::<Value>(
                scope.session.clone(),
                &scope.account_id,
                scope.config_version,
                scope.generation,
                &keys,
                path,
                body.clone(),
            )
        };
        let _completion = if path == COMPLETE {
            Some(self.upload_confirmation.lock().await)
        } else {
            None
        };
        let mut result = operation().await;
        if path == AUTHORIZE && matches!(result, Err(AccountError::Code("rate_limited"))) {
            let until = self
                .storage_retry
                .lock()
                .await
                .get(AUTHORIZE)
                .map(|(until, _)| *until);
            if let Some(until) = until.filter(|until| {
                until.saturating_duration_since(Instant::now()) <= Duration::from_secs(60)
            }) {
                let wait = tokio::time::sleep_until(until.into());
                result = match scope
                    .generation
                    .map(|generation| self.watch_storage(generation))
                    .transpose()
                {
                    Ok(Some(mut cancellation)) => tokio::select! {
                        biased;
                        _ = cancellation.changed() => Err("storage_config_changed".into()),
                        _ = wait => operation().await,
                    },
                    Ok(None) => {
                        wait.await;
                        operation().await
                    }
                    Err(error) => Err(error),
                };
            }
        }
        let mut responses =
            result.and_then(|value| batch_responses(path, scope.config_version, &batch, value));
        for item in batch {
            let result = match &mut responses {
                Ok(values) => values
                    .remove(item.item_id())
                    .ok_or_else(|| "invalid_response".into()),
                Err(error) => Err(error.clone()),
            };
            let _ = item.reply.send(result);
        }
    }

    async fn confirm_upload(&self, scope: &UploadScope, key: &str, id: &str) -> Result<bool> {
        for delay in [0, 400, 1200] {
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            match self
                .queue_upload(scope.clone(), COMPLETE, key.to_owned(), json!(id))
                .await
                .await
                .map_err(|_| AccountError::Code("temporarily_unavailable"))?
            {
                Ok(BatchReply::Confirmed(true)) => return Ok(true),
                Ok(BatchReply::UnknownGrant) => return Ok(false),
                Ok(BatchReply::Confirmed(false)) => {}
                Err(AccountError::Code(
                    "network" | "response_lost" | "temporarily_unavailable",
                )) => {}
                Err(error) => return Err(error),
                _ => return Err("invalid_response".into()),
            }
        }
        Ok(false)
    }

    pub(crate) async fn storage_upload(
        &self,
        session: PathBuf,
        account_id: &str,
        upload: UploadObject,
        bytes: Vec<u8>,
    ) -> Result<u16> {
        let (object, expected) = upload.metadata(&bytes)?;
        let client = storage_http()?;
        let bytes = bytes::Bytes::from(bytes);
        let scope = UploadScope {
            session,
            account_id: account_id.to_owned(),
            config_version: upload.config_version,
            generation: upload.storage_generation,
        };
        for attempt in 0..2 {
            let BatchReply::Grant(grant) = self
                .queue_upload(scope.clone(), AUTHORIZE, upload.key.clone(), object.clone())
                .await
                .await
                .map_err(|_| AccountError::Code("temporarily_unavailable"))??
            else {
                return Err("invalid_response".into());
            };
            let (url, headers, expires) = grant.validate(&upload.key, &expected)?;
            if expires <= now() {
                continue;
            }
            let send = client.put(url).headers(headers).body(bytes.clone()).send();
            let response = if let Some(generation) = upload.storage_generation {
                let mut cancellation = self.watch_storage(generation)?;
                tokio::select! {
                    biased;
                    _ = cancellation.changed() => return Err("storage_config_changed".into()),
                    response = send => response,
                }
            } else {
                send.await
            };
            let response = match response {
                Ok(response) => response,
                Err(_) => {
                    if self.confirm_upload(&scope, &upload.key, &grant.id).await? {
                        return Ok(200);
                    }
                    return Err("storage_upload_failed".into());
                }
            };
            let status = response.status();
            if status.as_u16() == 403 && expires <= now() && attempt == 0 {
                continue;
            }
            if status.is_success() {
                // Keep a successful PUT if HEAD accounting is temporarily unavailable.
                drop(
                    self.queue_upload(scope.clone(), COMPLETE, upload.key.clone(), json!(grant.id))
                        .await,
                );
                if let Some(generation) = upload.storage_generation {
                    self.watch_storage(generation)?;
                }
                return Ok(status.as_u16());
            }
            return match status.as_u16() {
                409 | 412 => Ok(status.as_u16()),
                429 => Err("rate_limited".into()),
                500..=599 => Err("temporarily_unavailable".into()),
                _ => Err("storage_upload_failed".into()),
            };
        }
        Err("storage_upload_expired".into())
    }

    pub(crate) async fn storage_delete(
        &self,
        session: PathBuf,
        account_id: &str,
        config_version: u64,
        generation: Option<u64>,
        keys: Vec<String>,
    ) -> Result<()> {
        if keys.is_empty() || keys.len() > 1000 {
            return Err("invalid_request".into());
        }
        let _: Value = self
            .storage_operation(
                session.clone(),
                account_id,
                config_version,
                generation,
                &keys.iter().map(String::as_str).collect::<Vec<_>>(),
                "objects-delete",
                json!({"configVersion": config_version, "keys": keys}),
            )
            .await?;
        let mut state = self.session.lock().await;
        let service = self.service(session)?;
        service.require_account(&state.access, account_id)?;
        if let Some(generation) = generation {
            self.watch_storage(generation)?;
        }
        if let Ok(usage) = service
            .authorized::<Usage>(&mut state.access, "usage", None)
            .await
        {
            if let Some(info) = service
                .cache(&mut state, account_id)
                .info
                .as_mut()
                .and_then(|entry| entry.value.as_mut())
            {
                info.usage = Some(usage);
                service.save_cache(&state);
            }
        }
        Ok(())
    }
}

fn storage_http() -> Result<&'static Client> {
    static HTTP: OnceLock<Result<Client>> = OnceLock::new();
    HTTP.get_or_init(|| {
        Client::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|_| "network".into())
    })
    .as_ref()
    .map_err(Clone::clone)
}

#[cfg(feature = "app")]
#[tauri::command]
pub(crate) async fn account_storage_delete(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ClientAccount>,
    account_id: String,
    config_version: u64,
    generation: Option<u64>,
    keys: Vec<String>,
) -> Result<()> {
    if window.label() != "main" {
        return Err("not_allowed".into());
    }
    state
        .storage_delete(
            app_session(&window)?,
            &account_id,
            config_version,
            generation,
            keys,
        )
        .await
}

#[cfg(feature = "app")]
#[tauri::command]
pub(crate) async fn account_storage_upload(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ClientAccount>,
    request: tauri::ipc::Request<'_>,
) -> Result<u16> {
    if window.label() != "main" {
        return Err("not_allowed".into());
    }
    let meta = request
        .headers()
        .get("x-qingyu-upload")
        .and_then(|header| STANDARD.decode(header.as_bytes()).ok())
        .and_then(|value| serde_json::from_slice::<Upload>(&value).ok())
        .ok_or("invalid_request")?;
    let bytes = match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) if bytes.len() <= MAX_OBJECT => bytes.clone(),
        tauri::ipc::InvokeBody::Json(value) => {
            serde_json::from_value(value.clone()).map_err(|_| "invalid_request")?
        }
        _ => return Err("invalid_request".into()),
    };
    state
        .storage_upload(app_session(&window)?, &meta.account_id, meta.object, bytes)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued(key: &str, generation: u64) -> QueuedUpload {
        QueuedUpload {
            scope: UploadScope {
                session: PathBuf::from("session"),
                account_id: "account".into(),
                config_version: 5,
                generation: Some(generation),
            },
            path: AUTHORIZE,
            key: key.into(),
            item: json!({"key": key}),
            reply: tokio::sync::oneshot::channel().0,
        }
    }

    #[test]
    fn batches_up_to_one_hundred_items_without_mixing_scopes_or_duplicate_keys() {
        let mut queue = vec![queued("0", 1), queued("0", 1), queued("other", 2)];
        queue.extend((1..=MAX_BATCH).map(|index| queued(&index.to_string(), 1)));
        let batch = take_upload_batch(&mut queue);
        assert_eq!(batch.len(), MAX_BATCH);
        assert_eq!(batch.last().unwrap().key, (MAX_BATCH - 1).to_string());
        let batch = take_upload_batch(&mut queue);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].key, "0");
        assert_eq!(batch[1].key, MAX_BATCH.to_string());
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].scope.generation, Some(2));
    }

    #[test]
    fn authorizes_the_manifest_and_key_in_separate_batches() {
        let manifest = "users/example/vault-v2/manifest.json.enc";
        let key_file = "users/example/miaoyan-key.json";
        let mut queue = vec![
            queued("first-chunk", 1),
            queued(manifest, 1),
            queued(key_file, 1),
            queued("second-chunk", 1),
        ];
        let batch = take_upload_batch(&mut queue);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].key, "first-chunk");
        assert_eq!(batch[1].key, "second-chunk");
        for path in [manifest, key_file] {
            let batch = take_upload_batch(&mut queue);
            assert_eq!(batch.len(), 1);
            assert_eq!(batch[0].key, path);
        }
        assert!(queue.is_empty());
    }

    #[test]
    fn keeps_valid_confirmations_in_a_batch_with_an_unknown_grant() {
        let mut batch = [queued("first", 1), queued("second", 1)];
        for (entry, id) in batch.iter_mut().zip(["uploaded", "expired"]) {
            entry.path = COMPLETE;
            entry.item = json!(id);
        }
        let responses = batch_responses(
            COMPLETE,
            5,
            &batch,
            json!({"results": [
                {"id": "expired", "state": "unknown"}, {"id": "uploaded", "state": "completed"},
            ]}),
        )
        .unwrap();
        assert!(matches!(responses["uploaded"], BatchReply::Confirmed(true)));
        assert!(matches!(responses["expired"], BatchReply::UnknownGrant));
    }

    #[test]
    fn requires_camel_case_config_version_and_matches_grants_by_key() {
        let batch = [queued("first", 1), queued("second", 1)];
        let grant = |key, id| {
            json!({
                "key": key, "id": id, "url": "https://writes.example.test/object?Signature=example",
                "expiresAt": "2026-11-24T12:00:00+08:00", "headers": {},
            })
        };
        let grants = [grant("second", "2"), grant("first", "1")];
        let responses = batch_responses(
            AUTHORIZE,
            5,
            &batch,
            json!({
                "configVersion": 5, "grants": grants,
            }),
        )
        .unwrap();
        assert!(matches!(&responses["first"], BatchReply::Grant(grant) if grant.id == "1"));
        for value in [
            json!({"configVersion": 6, "grants": grants}),
            json!({"config_version": 5, "grants": grants}),
            json!({"configVersion": 5, "grants": [grants[0], grants[0]]}),
        ] {
            assert!(batch_responses(AUTHORIZE, 5, &batch, value).is_err());
        }
    }

    #[test]
    fn authorizes_the_content_digest_and_validates_the_signed_headers() {
        let key = "users/aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee/vault-v2/manifest.json.enc";
        let upload: UploadObject = serde_json::from_value(json!({
            "configVersion": 5, "key": key, "ifMatch": "\"previous-etag\"",
        }))
        .unwrap();
        let (object, expected) = upload.metadata(b"hello").unwrap();
        assert_eq!(
            object,
            json!({
                "key": key, "size": 5, "md5": "XUFAKrxLKna5cZ2REBfFkg==", "ifMatch": "\"previous-etag\"",
            })
        );
        let mut grant: Grant = serde_json::from_value(json!({
            "key": key, "id": "test-grant", "url": "https://writes.example.test/object?Signature=example",
            "expiresAt": "2026-11-24T12:00:00+08:00",
            "headers": {
                "Content-Length": "5", "Content-MD5": "XUFAKrxLKna5cZ2REBfFkg==",
                "Content-Type": "application/octet-stream", "If-Match": "\"previous-etag\"",
            },
        })).unwrap();
        let (url, headers, _) = grant.validate(key, &expected).unwrap();
        assert_eq!(url.host_str(), Some("writes.example.test"));
        assert_eq!(headers["if-match"], "\"previous-etag\"");
        grant.headers.insert("Content-Length".into(), "6".into());
        assert!(grant.validate(key, &expected).is_err());
        grant.headers.insert("Content-Length".into(), "5".into());
        grant.headers.remove("If-Match");
        assert!(grant.validate(key, &expected).is_err());
    }
}
