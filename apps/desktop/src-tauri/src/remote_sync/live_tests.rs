use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use super::backend::{RemoteSyncBackend, RemoteSyncError, RemoteSyncFile, ValidRemoteRoot};
use super::catalog::list_remote_notebooks;
use super::engine::{
    execute_remote_sync as execute_scoped_remote_sync,
    execute_remote_sync_with_hooks as execute_scoped_remote_sync_with_hooks,
    RemoteSyncExecutionHooks, RemoteSyncSummary, MANIFEST_VERSION,
};
use super::s3_backend::{object_key_for_test, S3Backend, S3SyncSettings};
use super::scope::RemoteSyncScope;
use super::{
    create_webdav_backend, create_webdav_backend_at_validated_prefix, webdav_child_url,
    webdav_sync_root_url, WebDavSyncSettings,
};
use crate::notebook_scope::notes_remote_prefix;
use crate::sync_config::model::{S3Config, SyncConfig, SyncProvider, SyncSnapshot, SyncTarget};
use time::{Duration as TimeDuration, OffsetDateTime};

static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[test]
fn offline_namespace_helper_uses_one_random_root_with_disjoint_application_namespaces() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let raw_root = format!("qingyu-tests/{nanos}-{sequence}");
    let root = ValidRemoteRoot::parse(&raw_root).unwrap();
    let notes_prefix = notes_remote_prefix(&root, "Team Notes").unwrap();
    let app_prefix = root.app_prefix();

    assert_eq!(
        object_key_for_test(&notes_prefix, "topic/note.md").unwrap(),
        format!("{raw_root}/notes/Team Notes/topic/note.md")
    );
    assert_eq!(
        object_key_for_test(&app_prefix, "settings.json").unwrap(),
        format!("{raw_root}/app/settings.json")
    );

    let webdav_notes = webdav_sync_root_url("https://dav.example.test/base", &notes_prefix)
        .and_then(|root| webdav_child_url(&root, "topic/note.md", false))
        .unwrap();
    let webdav_app = webdav_sync_root_url("https://dav.example.test/base", &app_prefix)
        .and_then(|root| webdav_child_url(&root, "settings.json", false))
        .unwrap();
    assert_eq!(
        webdav_notes.path(),
        format!("/base/{raw_root}/notes/Team%20Notes/topic/note.md")
    );
    assert_eq!(
        webdav_app.path(),
        format!("/base/{raw_root}/app/settings.json")
    );
}

#[test]
fn webdav_backends_send_real_requests_to_disjoint_application_namespaces() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let raw_root = format!("qingyu-tests/{nanos}-{sequence}");
    let root = ValidRemoteRoot::parse(&raw_root).unwrap();
    let (server_url, requests, handle) = spawn_recording_webdav_fixture(14);

    tauri::async_runtime::block_on(async {
        let notes_backend = create_webdav_backend_at_validated_prefix(WebDavSyncSettings {
            password: String::new(),
            remote_path: notes_remote_prefix(&root, "Team Notes").unwrap(),
            server_url: server_url.clone(),
            username: String::new(),
        })
        .await
        .expect("notes WebDAV backend");
        let app_backend = create_webdav_backend(WebDavSyncSettings {
            password: String::new(),
            remote_path: root.app_prefix(),
            server_url,
            username: String::new(),
        })
        .await
        .expect("app WebDAV backend");

        assert!(notes_backend.list_files().await.unwrap().is_empty());
        notes_backend
            .upload("topic/note.md", b"note", None)
            .await
            .expect("notes upload");
        assert!(app_backend.list_files().await.unwrap().is_empty());
        app_backend
            .upload("settings.json", br#"{"language":"en"}"#, None)
            .await
            .expect("settings upload");
    });
    handle.join().expect("WebDAV fixture should finish");

    let requests = requests.lock().expect("WebDAV fixture request log");
    let notes_root = format!("/dav/{raw_root}/notes/Team%20Notes/");
    let app_root = format!("/dav/{raw_root}/app/");
    assert!(requests
        .iter()
        .any(|request| request.starts_with(&format!("MKCOL {notes_root} HTTP/1.1\r\n"))));
    assert!(requests.iter().any(|request| {
        request.starts_with(&format!("PROPFIND {notes_root} HTTP/1.1\r\n"))
            && request.to_ascii_lowercase().contains("depth: 1\r\n")
    }));
    assert!(requests.iter().any(|request| {
        request.starts_with(&format!("PUT {notes_root}topic/note.md HTTP/1.1\r\n"))
    }));
    assert!(requests
        .iter()
        .any(|request| request.starts_with(&format!("MKCOL {app_root} HTTP/1.1\r\n"))));
    assert!(requests.iter().any(|request| {
        request.starts_with(&format!("PROPFIND {app_root} HTTP/1.1\r\n"))
            && request.to_ascii_lowercase().contains("depth: 1\r\n")
    }));
    assert!(requests.iter().any(|request| {
        request.starts_with(&format!("PUT {app_root}settings.json HTTP/1.1\r\n"))
    }));
    for request in requests
        .iter()
        .filter(|request| request.starts_with("PROPFIND ") || request.starts_with("PUT "))
    {
        assert!(
            request.starts_with(&format!("PROPFIND {notes_root}"))
                || request.starts_with(&format!("PUT {notes_root}"))
                || request.starts_with(&format!("PROPFIND {app_root}"))
                || request.starts_with(&format!("PUT {app_root}")),
            "provider request escaped application namespaces: {}",
            request.lines().next().unwrap_or_default()
        );
    }
}

fn spawn_recording_webdav_fixture(
    expected_requests: usize,
) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("WebDAV fixture should bind");
    let address = listener.local_addr().expect("WebDAV fixture address");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    let handle = thread::spawn(move || {
        for _ in 0..expected_requests {
            let (mut stream, _) = listener.accept().expect("WebDAV fixture request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("WebDAV fixture read");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|index| index + 4)
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + content_length {
                    break;
                }
            }

            let request_text = String::from_utf8_lossy(&request).into_owned();
            let lower_request = request_text.to_ascii_lowercase();
            let response = if request_text.starts_with("MKCOL ") {
                "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else if request_text.starts_with("PROPFIND ")
                && lower_request.contains("depth: 0\r\n")
            {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            } else if request_text.starts_with("PROPFIND ")
                && lower_request.contains("depth: 1\r\n")
            {
                let body =
                    r#"<?xml version="1.0" encoding="utf-8"?><d:multistatus xmlns:d="DAV:"/>"#;
                format!(
                    "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            } else if request_text.starts_with("PUT ") {
                "HTTP/1.1 201 Created\r\nETag: \"recording-etag\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            } else {
                panic!(
                    "unexpected WebDAV fixture request: {}",
                    request_text.lines().next().unwrap_or_default()
                );
            };

            recorded
                .lock()
                .expect("WebDAV fixture request log")
                .push(request_text);
            stream
                .write_all(response.as_bytes())
                .expect("WebDAV fixture response");
        }
    });

    (format!("http://{address}/dav"), requests, handle)
}

fn live_state_root(root: &Path) -> PathBuf {
    let file_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("remote-sync-live");
    root.with_file_name(format!("{file_name}-sync-state"))
}

fn live_scope(root: &Path) -> Result<RemoteSyncScope, String> {
    live_scope_at(root, &live_state_root(root))
}

fn live_scope_at(root: &Path, state_root: &Path) -> Result<RemoteSyncScope, String> {
    RemoteSyncScope::notes(root, state_root, "s3-manifest.json", None, None)
}

async fn execute_remote_sync<B: RemoteSyncBackend>(
    root: &Path,
    backend: &B,
) -> Result<RemoteSyncSummary, RemoteSyncError> {
    let scope = live_scope(root)?;
    execute_scoped_remote_sync(&scope, backend).await
}

async fn execute_remote_sync_with_hooks<B: RemoteSyncBackend>(
    root: &Path,
    backend: &B,
    hooks: RemoteSyncExecutionHooks,
) -> Result<RemoteSyncSummary, String> {
    let scope = live_scope(root)?;
    execute_scoped_remote_sync_with_hooks(&scope, backend, hooks)
        .await
        .map_err(String::from)
}

struct FailAfterUploadBackend {
    inner: S3Backend,
    successful_uploads_remaining: Mutex<usize>,
}

impl FailAfterUploadBackend {
    fn new(inner: S3Backend, successful_uploads: usize) -> Self {
        Self {
            inner,
            successful_uploads_remaining: Mutex::new(successful_uploads),
        }
    }
}

impl RemoteSyncBackend for FailAfterUploadBackend {
    fn target_fingerprint_source(&self) -> String {
        self.inner.target_fingerprint_source()
    }

    async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
        self.inner.list_files().await
    }

    async fn download(
        &self,
        path: &str,
        expected_identity: &str,
    ) -> Result<Vec<u8>, RemoteSyncError> {
        self.inner.download(path, expected_identity).await
    }

    async fn upload(
        &self,
        path: &str,
        bytes: &[u8],
        expected_identity: Option<&str>,
    ) -> Result<String, RemoteSyncError> {
        let should_fail = {
            let mut remaining = self.successful_uploads_remaining.lock().unwrap();
            if *remaining == 0 {
                true
            } else {
                *remaining -= 1;
                false
            }
        };
        if should_fail {
            return Err(format!("Injected live S3 upload failure: {path}").into());
        }
        self.inner.upload(path, bytes, expected_identity).await
    }

    async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError> {
        self.inner.delete(path, expected_identity).await
    }
}

struct MutateBeforeUploadBackend {
    inner: S3Backend,
    mutated: Mutex<bool>,
}

struct RecordingS3Backend {
    inner: S3Backend,
    operations: Mutex<Vec<String>>,
}

impl RecordingS3Backend {
    fn new(inner: S3Backend) -> Self {
        Self {
            inner,
            operations: Mutex::new(Vec::new()),
        }
    }

    fn operations(&self) -> Vec<String> {
        self.operations.lock().unwrap().clone()
    }

    fn record(&self, operation: &str, path: &str) {
        self.operations
            .lock()
            .unwrap()
            .push(format!("{operation}:{path}"));
    }
}

impl RemoteSyncBackend for RecordingS3Backend {
    fn target_fingerprint_source(&self) -> String {
        self.inner.target_fingerprint_source()
    }

    async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
        self.inner.list_files().await
    }

    async fn download(
        &self,
        path: &str,
        expected_identity: &str,
    ) -> Result<Vec<u8>, RemoteSyncError> {
        self.record("download", path);
        self.inner.download(path, expected_identity).await
    }

    async fn upload(
        &self,
        path: &str,
        bytes: &[u8],
        expected_identity: Option<&str>,
    ) -> Result<String, RemoteSyncError> {
        self.record("upload", path);
        self.inner.upload(path, bytes, expected_identity).await
    }

    async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError> {
        self.record("delete", path);
        self.inner.delete(path, expected_identity).await
    }
}

impl MutateBeforeUploadBackend {
    fn new(inner: S3Backend) -> Self {
        Self {
            inner,
            mutated: Mutex::new(false),
        }
    }
}

impl RemoteSyncBackend for MutateBeforeUploadBackend {
    fn target_fingerprint_source(&self) -> String {
        self.inner.target_fingerprint_source()
    }

    async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
        self.inner.list_files().await
    }

    async fn download(
        &self,
        path: &str,
        expected_identity: &str,
    ) -> Result<Vec<u8>, RemoteSyncError> {
        self.inner.download(path, expected_identity).await
    }

    async fn upload(
        &self,
        path: &str,
        bytes: &[u8],
        expected_identity: Option<&str>,
    ) -> Result<String, RemoteSyncError> {
        let should_mutate = {
            let mut mutated = self.mutated.lock().unwrap();
            if *mutated || expected_identity.is_none() {
                false
            } else {
                *mutated = true;
                true
            }
        };
        if should_mutate {
            self.inner
                .upload(path, b"concurrent remote version", expected_identity)
                .await?;
        }
        self.inner.upload(path, bytes, expected_identity).await
    }

    async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError> {
        self.inner.delete(path, expected_identity).await
    }
}

struct LiveS3Config {
    access_key_id: String,
    bucket: String,
    endpoint_url: String,
    prefix_root: String,
    region: String,
    run_id: String,
    secret_access_key: String,
}

impl LiveS3Config {
    fn from_env() -> Result<Self, String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("System clock is before the Unix epoch: {error}"))?
            .as_nanos();
        let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Ok(Self {
            access_key_id: required_env("MARKRA_TEST_S3_ACCESS_KEY_ID")?,
            bucket: required_env("MARKRA_TEST_S3_BUCKET")?,
            endpoint_url: required_env("MARKRA_TEST_S3_ENDPOINT")?,
            prefix_root: optional_env("MARKRA_TEST_S3_PREFIX_ROOT")
                .unwrap_or_else(|| "markra-sync-tests".to_string()),
            region: optional_env("MARKRA_TEST_S3_REGION")
                .unwrap_or_else(|| "us-east-1".to_string()),
            run_id: format!("{nanos}-{}-{sequence}", std::process::id()),
            secret_access_key: required_env("MARKRA_TEST_S3_SECRET_ACCESS_KEY")?,
        })
    }

    fn settings_for(&self, scenario: &str) -> S3SyncSettings {
        S3SyncSettings {
            access_key_id: self.access_key_id.clone(),
            bucket: self.bucket.clone(),
            endpoint_url: self.endpoint_url.clone(),
            region: self.region.clone(),
            remote_path: format!("{}/{}/{}", self.prefix_root, self.run_id, scenario),
            secret_access_key: self.secret_access_key.clone(),
        }
    }

    fn backend_for(&self, scenario: &str) -> Result<S3Backend, String> {
        S3Backend::new(self.settings_for(scenario))
    }

    fn backend_at(&self, remote_path: String) -> Result<S3Backend, String> {
        S3Backend::new(S3SyncSettings {
            access_key_id: self.access_key_id.clone(),
            bucket: self.bucket.clone(),
            endpoint_url: self.endpoint_url.clone(),
            region: self.region.clone(),
            remote_path,
            secret_access_key: self.secret_access_key.clone(),
        })
    }

    fn snapshot_at(&self, remote_root: &ValidRemoteRoot, state_root: PathBuf) -> SyncSnapshot {
        let s3 = S3Config {
            access_key_id: self.access_key_id.clone(),
            bucket: self.bucket.clone(),
            endpoint_url: self.endpoint_url.clone(),
            region: self.region.clone(),
            secret_access_key: self.secret_access_key.clone(),
            ..S3Config::default()
        };
        let config = SyncConfig {
            enabled: true,
            provider: SyncProvider::S3,
            remote_root: remote_root.as_str().to_string(),
            s3: s3.clone(),
            ..SyncConfig::default()
        };
        SyncSnapshot {
            config,
            revision: format!("live-{}", self.run_id),
            state_root,
            target: SyncTarget::S3 {
                access_key_id: s3.access_key_id,
                bucket: s3.bucket,
                endpoint_url: s3.endpoint_url,
                region: s3.region,
                remote_root: remote_root.as_str().to_string(),
                secret_access_key: s3.secret_access_key,
                addressing_style: s3.addressing_style,
                request_timeout_seconds: s3.request_timeout_seconds,
                tls_verification: s3.tls_verification,
            },
        }
    }
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn required_env(name: &str) -> Result<String, String> {
    optional_env(name).ok_or_else(|| format!("Missing required live S3 test variable: {name}"))
}

async fn cleanup_backend_prefix(backend: &S3Backend) -> Result<(), String> {
    let files = backend.list_files().await?;
    for (path, file) in files {
        backend.delete(&path, &file.identity).await?;
    }
    let remaining = backend.list_files().await?;
    if remaining.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Live S3 cleanup left {} object(s) below the isolated scenario prefix",
        remaining.len()
    ))
}

async fn run_harness_smoke() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("harness")?;
    let scenario_result = async {
        let expected = b"markra live MinIO smoke";
        backend.upload("marker.md", expected, None).await?;
        let files = backend.list_files().await?;
        let marker = files
            .get("marker.md")
            .ok_or_else(|| "Live S3 marker was not listed after upload".to_string())?;
        let downloaded = backend.download("marker.md", &marker.identity).await?;
        if downloaded != expected {
            return Err("Live S3 marker bytes did not match the uploaded bytes".to_string());
        }
        Ok(())
    }
    .await;
    let cleanup_result = cleanup_backend_prefix(&backend).await;
    match (scenario_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(format!(
            "Live S3 scenario {} failed: {error}",
            config.run_id
        )),
        (Ok(()), Err(error)) => Err(format!("Live S3 cleanup {} failed: {error}", config.run_id)),
        (Err(scenario), Err(cleanup)) => Err(format!(
            "Live S3 scenario {} failed: {scenario}; cleanup also failed: {cleanup}",
            config.run_id
        )),
    }
}

async fn run_named_notebooks_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let prefix_root = config
        .prefix_root
        .trim_matches(|character| character == '/' || character == '\\');
    let raw_root = format!("{prefix_root}/{}/named-notebooks", config.run_id);
    let root = ValidRemoteRoot::parse(&raw_root)?;
    let exact_root_backend = config.backend_at(root.as_str().to_string())?;
    let notebook_a_backend = config.backend_at(notes_remote_prefix(&root, "A")?)?;
    let notebook_b_backend = config.backend_at(notes_remote_prefix(&root, "B")?)?;
    let app_backend = config.backend_at(root.app_prefix())?;
    let local_suite = temp_root(&config, "named-notebooks")?;
    let notebook_a_root = local_suite.join("A");
    let notebook_b_root = local_suite.join("B");
    fs::create_dir_all(&notebook_a_root)
        .map_err(|error| format!("Failed to create live notebook A: {error}"))?;
    fs::create_dir_all(&notebook_b_root)
        .map_err(|error| format!("Failed to create live notebook B: {error}"))?;
    let notebook_a_state = local_suite.join("sync-state/A");
    let notebook_b_state = local_suite.join("sync-state/B");

    let scenario_result = async {
        if !exact_root_backend.list_files().await?.is_empty() {
            return Err("Live named-notebook root was not isolated at start".to_string());
        }

        notebook_a_backend
            .upload("remote-a.md", b"remote notebook A", None)
            .await?;
        notebook_b_backend
            .upload("remote-b.md", b"remote notebook B", None)
            .await?;
        app_backend
            .upload(
                "settings.json",
                br#"{"language":"en","themeMode":"system"}"#,
                None,
            )
            .await?;
        let settings_identity = listed_remote_identity(&app_backend, "settings.json").await?;

        let catalog = list_remote_notebooks(
            config.snapshot_at(&root, local_suite.join("catalog-state")),
        )
        .await?;
        let catalog_names = catalog
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        if catalog_names != ["A", "B"] || catalog.iter().any(|entry| !entry.available) {
            return Err(format!(
                "Live S3 shallow catalog did not return exactly available A and B: {catalog_names:?}"
            ));
        }

        write_local_file(&notebook_a_root, "local-a.md", b"local notebook A")?;
        let notebook_a_backend = RecordingS3Backend::new(notebook_a_backend);
        let notebook_a_scope = live_scope_at(&notebook_a_root, &notebook_a_state)?;
        let first_a = execute_scoped_remote_sync(&notebook_a_scope, &notebook_a_backend).await?;
        assert_summary(&first_a, 1, 1, 0)?;
        if notebook_a_backend.operations() != ["download:remote-a.md", "upload:local-a.md"] {
            return Err(format!(
                "Existing notebook A was not hydrated before local publication: {:?}",
                notebook_a_backend.operations()
            ));
        }
        if read_local_file(&notebook_a_root, "remote-a.md")? != b"remote notebook A"
            || notebook_a_root.join("remote-b.md").exists()
        {
            return Err("Selecting notebook A downloaded notebook B or missed A".to_string());
        }
        assert_manifest_paths_at_state(
            &notebook_a_state,
            &["local-a.md", "remote-a.md"],
        )?;
        assert_remote_identity(&app_backend, "settings.json", &settings_identity).await?;

        write_local_file(&notebook_b_root, "local-b.md", b"local notebook B")?;
        let notebook_b_scope = live_scope_at(&notebook_b_root, &notebook_b_state)?;
        let first_b = execute_scoped_remote_sync(&notebook_b_scope, &notebook_b_backend).await?;
        assert_summary(&first_b, 1, 1, 0)?;
        if read_local_file(&notebook_b_root, "remote-b.md")? != b"remote notebook B"
            || notebook_b_root.join("remote-a.md").exists()
            || notebook_b_root.join("local-a.md").exists()
        {
            return Err("Switching to notebook B crossed notebook namespaces".to_string());
        }
        assert_manifest_paths_at_state(
            &notebook_b_state,
            &["local-b.md", "remote-b.md"],
        )?;
        assert_remote_identity(&app_backend, "settings.json", &settings_identity).await?;

        write_local_file(&notebook_a_root, "return-a.md", b"returned to notebook A")?;
        let return_a = execute_scoped_remote_sync(&notebook_a_scope, &notebook_a_backend).await?;
        assert_summary(&return_a, 1, 0, 0)?;
        assert_manifest_paths_at_state(
            &notebook_a_state,
            &["local-a.md", "remote-a.md", "return-a.md"],
        )?;
        assert_remote_identity(&app_backend, "settings.json", &settings_identity).await?;

        let remote_paths = exact_root_backend
            .list_files()
            .await?
            .into_keys()
            .collect::<Vec<_>>();
        let expected_paths = [
            "app/settings.json",
            "notes/A/local-a.md",
            "notes/A/remote-a.md",
            "notes/A/return-a.md",
            "notes/B/local-b.md",
            "notes/B/remote-b.md",
        ];
        if remote_paths != expected_paths {
            return Err(format!(
                "Live named-notebook remote layout was unexpected: {remote_paths:?}"
            ));
        }

        Ok(())
    }
    .await;
    finish_scenario(
        &config,
        &exact_root_backend,
        &[local_suite],
        scenario_result,
    )
    .await
}

async fn run_read_only_connection_test_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("connection-test")?;
    let scenario_result = async {
        backend
            .upload("existing.md", b"connection test fixture", None)
            .await?;
        let before = backend.list_files().await?;
        let checked_target = backend.test_connection().await?;
        let after = backend.list_files().await?;
        if before != after {
            return Err("Read-only S3 connection test changed the remote snapshot".to_string());
        }
        if checked_target.is_empty() {
            return Err("Read-only S3 connection test returned an empty target".to_string());
        }
        Ok(())
    }
    .await;
    let cleanup_result = cleanup_backend_prefix(&backend).await;
    match (scenario_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(format!(
            "Live S3 connection-test scenario {} failed: {error}",
            config.run_id
        )),
        (Ok(()), Err(error)) => Err(format!("Live S3 cleanup {} failed: {error}", config.run_id)),
        (Err(scenario), Err(cleanup)) => Err(format!(
            "Live S3 connection-test scenario {} failed: {scenario}; cleanup also failed: {cleanup}",
            config.run_id
        )),
    }
}

fn temp_root(config: &LiveS3Config, name: &str) -> Result<PathBuf, String> {
    let root = env::temp_dir().join(format!("markra-s3-live-{}-{name}", config.run_id));
    fs::create_dir_all(&root)
        .map_err(|error| format!("Failed to create live S3 local root {name}: {error}"))?;
    root.canonicalize()
        .map_err(|error| format!("Failed to resolve live S3 local root {name}: {error}"))
}

fn write_local_file(root: &Path, relative_path: &str, bytes: &[u8]) -> Result<(), String> {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create local test folder: {error}"))?;
    }
    fs::write(path, bytes).map_err(|error| format!("Failed to write local test file: {error}"))
}

fn read_local_file(root: &Path, relative_path: &str) -> Result<Vec<u8>, String> {
    fs::read(root.join(relative_path))
        .map_err(|error| format!("Failed to read local test file {relative_path}: {error}"))
}

fn snapshot_local_files(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(directory)
            .map_err(|error| format!("Failed to list local test folder: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("Failed to read local test entry: {error}"))?;
            let path = entry.path();
            if path.is_dir() {
                if matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some(".qingyu" | ".markra-sync")
                ) {
                    continue;
                }
                visit(root, &path, files)?;
            } else if path.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| format!("Failed to normalize local test path: {error}"))?
                    .to_path_buf();
                files.insert(
                    relative,
                    fs::read(&path)
                        .map_err(|error| format!("Failed to snapshot local test file: {error}"))?,
                );
            }
        }
        Ok(())
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn assert_manifest_paths(root: &Path, expected_paths: &[&str]) -> Result<(), String> {
    assert_manifest_paths_at_state(&live_state_root(root), expected_paths)
}

fn assert_manifest_paths_at_state(
    state_root: &Path,
    expected_paths: &[&str],
) -> Result<(), String> {
    let path = state_root.join("s3-manifest.json");
    let bytes =
        fs::read(&path).map_err(|error| format!("Failed to read live S3 manifest: {error}"))?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Failed to parse live S3 manifest: {error}"))?;
    if manifest.get("version").and_then(serde_json::Value::as_u64)
        != Some(u64::from(MANIFEST_VERSION))
    {
        return Err(format!(
            "Live S3 manifest version was not {MANIFEST_VERSION}"
        ));
    }
    if manifest
        .get("target_fingerprint")
        .and_then(serde_json::Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("Live S3 manifest target fingerprint was empty".to_string());
    }
    let entries = manifest
        .get("entries")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "Live S3 manifest entries were missing".to_string())?;
    if entries.len() != expected_paths.len()
        || expected_paths
            .iter()
            .any(|path| !entries.contains_key(*path))
    {
        return Err(format!(
            "Live S3 manifest paths did not match expected paths: {expected_paths:?}"
        ));
    }
    Ok(())
}

fn manifest_fingerprint(root: &Path) -> Result<String, String> {
    let bytes = fs::read(live_state_root(root).join("s3-manifest.json"))
        .map_err(|error| format!("Failed to read live S3 manifest fingerprint: {error}"))?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Failed to parse live S3 manifest fingerprint: {error}"))?;
    manifest
        .get("target_fingerprint")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "Live S3 manifest fingerprint was missing".to_string())
}

fn conflict_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = fs::read_dir(root)
        .map_err(|error| format!("Failed to list conflict test root: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(".remote-conflict-"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn conflict_timestamp(now: OffsetDateTime) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

async fn read_remote_file(backend: &S3Backend, relative_path: &str) -> Result<Vec<u8>, String> {
    let files = backend.list_files().await?;
    let file = files
        .get(relative_path)
        .ok_or_else(|| format!("Live S3 object was not listed: {relative_path}"))?;
    backend
        .download(relative_path, &file.identity)
        .await
        .map_err(String::from)
}

async fn listed_remote_identity(
    backend: &S3Backend,
    relative_path: &str,
) -> Result<String, String> {
    backend
        .list_files()
        .await?
        .get(relative_path)
        .map(|file| file.identity.clone())
        .ok_or_else(|| format!("Live S3 object was not listed: {relative_path}"))
}

async fn assert_remote_identity(
    backend: &S3Backend,
    relative_path: &str,
    expected_identity: &str,
) -> Result<(), String> {
    if listed_remote_identity(backend, relative_path).await? == expected_identity {
        Ok(())
    } else {
        Err(format!(
            "Live S3 object identity changed unexpectedly: {relative_path}"
        ))
    }
}

async fn delete_remote_file(backend: &S3Backend, relative_path: &str) -> Result<(), String> {
    let files = backend.list_files().await?;
    let file = files
        .get(relative_path)
        .ok_or_else(|| format!("Live S3 object was not listed for deletion: {relative_path}"))?;
    backend
        .delete(relative_path, &file.identity)
        .await
        .map_err(String::from)
}

fn assert_summary(
    summary: &RemoteSyncSummary,
    uploaded: u64,
    downloaded: u64,
    conflicts: u64,
) -> Result<(), String> {
    if summary.uploaded_files == uploaded
        && summary.downloaded_files == downloaded
        && summary.conflict_files == conflicts
    {
        return Ok(());
    }
    Err(format!(
        "Unexpected live S3 summary: uploaded={}, downloaded={}, conflicts={}",
        summary.uploaded_files, summary.downloaded_files, summary.conflict_files
    ))
}

async fn assert_noop_sync(root: &Path, backend: &S3Backend) -> Result<(), String> {
    let remote_before = backend.list_files().await?;
    let local_before = snapshot_local_files(root)?;
    let manifest_before = fs::read(live_state_root(root).join("s3-manifest.json"))
        .map_err(|error| format!("Failed to snapshot live S3 manifest: {error}"))?;
    let summary = execute_remote_sync(root, backend).await?;
    assert_summary(&summary, 0, 0, 0)?;
    let remote_after = backend.list_files().await?;
    let local_after = snapshot_local_files(root)?;
    let manifest_after = fs::read(live_state_root(root).join("s3-manifest.json"))
        .map_err(|error| format!("Failed to re-read live S3 manifest: {error}"))?;
    if remote_before == remote_after
        && local_before == local_after
        && manifest_before == manifest_after
    {
        return Ok(());
    }
    Err("No-op live S3 sync changed local, remote, or manifest state".to_string())
}

async fn finish_scenario(
    config: &LiveS3Config,
    backend: &S3Backend,
    local_roots: &[PathBuf],
    scenario_result: Result<(), String>,
) -> Result<(), String> {
    let remote_cleanup = cleanup_backend_prefix(backend).await;
    let mut local_cleanup_errors = Vec::new();
    for root in local_roots {
        let state = live_state_root(root);
        for path in [root.as_path(), state.as_path()] {
            if let Err(error) = fs::remove_dir_all(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    local_cleanup_errors.push(error.to_string());
                }
            }
        }
    }
    let cleanup_result = match (remote_cleanup, local_cleanup_errors.is_empty()) {
        (Ok(()), true) => Ok(()),
        (Err(error), true) => Err(error),
        (Ok(()), false) => Err(format!(
            "Local cleanup failed: {}",
            local_cleanup_errors.join("; ")
        )),
        (Err(remote), false) => Err(format!(
            "{remote}; local cleanup also failed: {}",
            local_cleanup_errors.join("; ")
        )),
    };
    match (scenario_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(format!(
            "Live S3 scenario {} failed: {error}",
            config.run_id
        )),
        (Ok(()), Err(error)) => Err(format!("Live S3 cleanup {} failed: {error}", config.run_id)),
        (Err(scenario), Err(cleanup)) => Err(format!(
            "Live S3 scenario {} failed: {scenario}; cleanup also failed: {cleanup}",
            config.run_id
        )),
    }
}

async fn run_create_read_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("create-read")?;
    let device_a = temp_root(&config, "create-read-a")?;
    let device_b = temp_root(&config, "create-read-b")?;
    let scenario_result = async {
        let local_bytes = b"created on device A";
        write_local_file(&device_a, "local.md", local_bytes)?;
        let upload = execute_remote_sync(&device_a, &backend).await?;
        assert_summary(&upload, 1, 0, 0)?;
        if read_remote_file(&backend, "local.md").await? != local_bytes {
            return Err("Local create did not reach MinIO with exact bytes".to_string());
        }

        let remote_bytes = b"created directly in MinIO";
        backend.upload("remote.md", remote_bytes, None).await?;
        let download = execute_remote_sync(&device_b, &backend).await?;
        assert_summary(&download, 0, 2, 0)?;
        if read_local_file(&device_b, "local.md")? != local_bytes
            || read_local_file(&device_b, "remote.md")? != remote_bytes
        {
            return Err("Remote create did not download to device B with exact bytes".to_string());
        }
        assert_manifest_paths(&device_b, &["local.md", "remote.md"])?;
        assert_noop_sync(&device_b, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[device_a, device_b], scenario_result).await
}

async fn run_update_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("updates")?;
    let root = temp_root(&config, "updates")?;
    let scenario_result = async {
        write_local_file(&root, "same-size.md", b"AAAA")?;
        let baseline = execute_remote_sync(&root, &backend).await?;
        assert_summary(&baseline, 1, 0, 0)?;

        write_local_file(&root, "same-size.md", b"BBBB")?;
        let local_update = execute_remote_sync(&root, &backend).await?;
        assert_summary(&local_update, 1, 0, 0)?;
        if read_remote_file(&backend, "same-size.md").await? != b"BBBB" {
            return Err("Local same-size update did not replace MinIO bytes".to_string());
        }

        let listed = backend.list_files().await?;
        let current = listed
            .get("same-size.md")
            .ok_or_else(|| "Updated live S3 object was not listed".to_string())?;
        backend
            .upload("same-size.md", b"CCCC", Some(&current.identity))
            .await?;
        let remote_update = execute_remote_sync(&root, &backend).await?;
        assert_summary(&remote_update, 0, 1, 0)?;
        if read_local_file(&root, "same-size.md")? != b"CCCC" {
            return Err("Remote same-size update did not replace local bytes".to_string());
        }
        assert_manifest_paths(&root, &["same-size.md"])?;
        assert_noop_sync(&root, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_topology_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("topology")?;
    let source = temp_root(&config, "topology-source")?;
    let target = temp_root(&config, "topology-target")?;
    let files = vec![
        ("nested/space name.md", b"space path".to_vec()),
        ("unicode/安全.md", "你好，MinIO".as_bytes().to_vec()),
        (
            "encoded/hash#question?percent%.md",
            b"reserved key characters".to_vec(),
        ),
        ("empty.md", Vec::new()),
        ("assets/blob.bin", vec![0, 1, 2, 127, 128, 254, 255]),
    ];
    let scenario_result = async {
        for (path, bytes) in &files {
            write_local_file(&source, path, bytes)?;
        }
        let upload = execute_remote_sync(&source, &backend).await?;
        assert_summary(&upload, files.len() as u64, 0, 0)?;
        for (path, bytes) in &files {
            if read_remote_file(&backend, path).await? != *bytes {
                return Err(format!("Topology object bytes did not match: {path}"));
            }
        }

        let download = execute_remote_sync(&target, &backend).await?;
        assert_summary(&download, 0, files.len() as u64, 0)?;
        for (path, bytes) in &files {
            if read_local_file(&target, path)? != *bytes {
                return Err(format!("Topology local bytes did not match: {path}"));
            }
        }
        let expected_paths = files.iter().map(|(path, _)| *path).collect::<Vec<_>>();
        assert_manifest_paths(&target, &expected_paths)?;
        assert_noop_sync(&target, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[source, target], scenario_result).await
}

async fn run_pagination_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("pagination")?;
    let scenario_result = async {
        for index in 0..1_001 {
            backend
                .upload(&format!("pages/{index:04}.bin"), b"x", None)
                .await?;
        }
        let files = backend.list_files().await?;
        if files.len() != 1_001 {
            return Err(format!(
                "Paginated live S3 listing returned {} objects instead of 1001",
                files.len()
            ));
        }
        if !files.contains_key("pages/0000.bin") || !files.contains_key("pages/1000.bin") {
            return Err("Paginated live S3 listing missed a boundary object".to_string());
        }
        Ok(())
    }
    .await;
    finish_scenario(&config, &backend, &[], scenario_result).await
}

async fn run_local_delete_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("local-delete")?;
    let root = temp_root(&config, "local-delete")?;
    let scenario_result = async {
        write_local_file(&root, "deleted.md", b"baseline")?;
        let baseline = execute_remote_sync(&root, &backend).await?;
        assert_summary(&baseline, 1, 0, 0)?;
        fs::remove_file(root.join("deleted.md"))
            .map_err(|error| format!("Failed to remove local baseline file: {error}"))?;

        let deletion = execute_remote_sync(&root, &backend).await?;
        assert_summary(&deletion, 0, 0, 0)?;
        if backend.list_files().await?.contains_key("deleted.md") {
            return Err("Local deletion did not remove the MinIO object".to_string());
        }
        assert_manifest_paths(&root, &[])?;
        assert_noop_sync(&root, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_remote_delete_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("remote-delete")?;
    let root = temp_root(&config, "remote-delete")?;
    let scenario_result = async {
        write_local_file(&root, "deleted.md", b"baseline")?;
        let baseline = execute_remote_sync(&root, &backend).await?;
        assert_summary(&baseline, 1, 0, 0)?;
        delete_remote_file(&backend, "deleted.md").await?;

        let deletion = execute_remote_sync(&root, &backend).await?;
        assert_summary(&deletion, 0, 0, 0)?;
        if root.join("deleted.md").exists() {
            return Err("Remote deletion did not remove the local file".to_string());
        }
        assert_manifest_paths(&root, &[])?;
        assert_noop_sync(&root, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_remote_survivor_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("remote-survivor")?;
    let root = temp_root(&config, "remote-survivor")?;
    let scenario_result = async {
        write_local_file(&root, "survivor.md", b"baseline")?;
        let baseline = execute_remote_sync(&root, &backend).await?;
        assert_summary(&baseline, 1, 0, 0)?;
        fs::remove_file(root.join("survivor.md"))
            .map_err(|error| format!("Failed to remove local survivor baseline: {error}"))?;

        let files = backend.list_files().await?;
        let current = files
            .get("survivor.md")
            .ok_or_else(|| "Remote survivor baseline was not listed".to_string())?;
        backend
            .upload("survivor.md", b"remote changed", Some(&current.identity))
            .await?;
        let survivor = execute_remote_sync(&root, &backend).await?;
        assert_summary(&survivor, 0, 1, 0)?;
        if read_local_file(&root, "survivor.md")? != b"remote changed" {
            return Err("Changed remote survivor was not restored locally".to_string());
        }
        assert_manifest_paths(&root, &["survivor.md"])?;
        assert_noop_sync(&root, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_local_survivor_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("local-survivor")?;
    let root = temp_root(&config, "local-survivor")?;
    let scenario_result = async {
        write_local_file(&root, "survivor.md", b"baseline")?;
        let baseline = execute_remote_sync(&root, &backend).await?;
        assert_summary(&baseline, 1, 0, 0)?;
        delete_remote_file(&backend, "survivor.md").await?;
        write_local_file(&root, "survivor.md", b"local changed")?;

        let survivor = execute_remote_sync(&root, &backend).await?;
        assert_summary(&survivor, 1, 0, 0)?;
        if read_remote_file(&backend, "survivor.md").await? != b"local changed" {
            return Err("Changed local survivor was not restored to MinIO".to_string());
        }
        assert_manifest_paths(&root, &["survivor.md"])?;
        assert_noop_sync(&root, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_first_sync_conflict_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("first-conflict")?;
    let root = temp_root(&config, "first-conflict")?;
    let scenario_result = async {
        write_local_file(&root, "note.md", b"local first version")?;
        backend
            .upload("note.md", b"remote first version", None)
            .await?;

        let conflict = execute_remote_sync(&root, &backend).await?;
        assert_summary(&conflict, 0, 0, 1)?;
        if read_local_file(&root, "note.md")? != b"local first version" {
            return Err("First-sync conflict replaced the local original".to_string());
        }
        let conflicts = conflict_files(&root)?;
        if conflicts.len() != 1
            || conflicts[0].extension().and_then(|value| value.to_str()) != Some("md")
            || fs::read(&conflicts[0])
                .map_err(|error| format!("Failed to read conflict copy: {error}"))?
                != b"remote first version"
        {
            return Err(
                "First-sync conflict copy did not preserve the remote Markdown version".to_string(),
            );
        }

        let stabilization = execute_remote_sync(&root, &backend).await?;
        assert_summary(&stabilization, 1, 0, 0)?;
        if conflict_files(&root)?.len() != 1 {
            return Err("Stabilization created a repeated conflict copy".to_string());
        }
        let conflict_name = conflicts[0]
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Conflict copy name was invalid".to_string())?;
        assert_manifest_paths(&root, &["note.md", conflict_name])?;
        assert_noop_sync(&root, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_changed_conflict_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("changed-conflict")?;
    let root = temp_root(&config, "changed-conflict")?;
    let scenario_result = async {
        write_local_file(&root, "note.md", b"baseline")?;
        let baseline = execute_remote_sync(&root, &backend).await?;
        assert_summary(&baseline, 1, 0, 0)?;

        write_local_file(&root, "note.md", b"local changed version")?;
        let files = backend.list_files().await?;
        let current = files
            .get("note.md")
            .ok_or_else(|| "Changed-conflict baseline was not listed".to_string())?;
        backend
            .upload(
                "note.md",
                b"remote changed version",
                Some(&current.identity),
            )
            .await?;
        let conflict = execute_remote_sync(&root, &backend).await?;
        assert_summary(&conflict, 0, 0, 1)?;
        if read_local_file(&root, "note.md")? != b"local changed version" {
            return Err("Changed-both conflict replaced the local original".to_string());
        }
        let conflicts = conflict_files(&root)?;
        if conflicts.len() != 1
            || fs::read(&conflicts[0])
                .map_err(|error| format!("Failed to read changed conflict copy: {error}"))?
                != b"remote changed version"
        {
            return Err("Changed-both conflict copy did not preserve remote bytes".to_string());
        }

        let stabilization = execute_remote_sync(&root, &backend).await?;
        assert_summary(&stabilization, 1, 0, 0)?;
        if conflict_files(&root)?.len() != 1 {
            return Err("Changed-both stabilization repeated the conflict".to_string());
        }
        assert_noop_sync(&root, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_conflict_collision_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("conflict-collision")?;
    let root = temp_root(&config, "conflict-collision")?;
    let scenario_result = async {
        write_local_file(&root, "note.md", b"baseline")?;
        let baseline = execute_remote_sync(&root, &backend).await?;
        assert_summary(&baseline, 1, 0, 0)?;
        write_local_file(&root, "note.md", b"local collision version")?;
        let files = backend.list_files().await?;
        let current = files
            .get("note.md")
            .ok_or_else(|| "Conflict collision baseline was not listed".to_string())?;
        backend
            .upload(
                "note.md",
                b"remote collision version",
                Some(&current.identity),
            )
            .await?;

        let now = OffsetDateTime::now_utc();
        let mut reserved_paths = Vec::new();
        for seconds in -2..=4 {
            let timestamp = conflict_timestamp(now + TimeDuration::seconds(seconds));
            let relative = format!("note.remote-conflict-{timestamp}.md");
            write_local_file(&root, &relative, b"reserved conflict name")?;
            reserved_paths.push(root.join(relative));
        }

        let conflict = execute_remote_sync(&root, &backend).await?;
        assert_summary(&conflict, reserved_paths.len() as u64, 0, 1)?;
        for reserved in &reserved_paths {
            if fs::read(reserved)
                .map_err(|error| format!("Failed to read reserved conflict file: {error}"))?
                != b"reserved conflict name"
            {
                return Err("Conflict handling overwrote a reserved conflict name".to_string());
            }
        }
        let generated = conflict_files(&root)?
            .into_iter()
            .filter(|path| {
                fs::read(path)
                    .map(|bytes| bytes == b"remote collision version")
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if generated.len() != 1
            || !generated[0]
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".md-2"))
        {
            return Err("Conflict collision did not select one unique conflict name".to_string());
        }
        if read_local_file(&root, "note.md")? != b"local collision version" {
            return Err("Conflict collision replaced the local original".to_string());
        }

        let stabilization = execute_remote_sync(&root, &backend).await?;
        assert_summary(&stabilization, 1, 0, 0)?;
        let generated_count = conflict_files(&root)?
            .into_iter()
            .filter(|path| {
                fs::read(path)
                    .map(|bytes| bytes == b"remote collision version")
                    .unwrap_or(false)
            })
            .count();
        if generated_count != 1 {
            return Err("Conflict collision stabilization repeated the conflict".to_string());
        }
        assert_noop_sync(&root, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_target_reset_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let first = config.backend_for("target-a")?;
    let second = config.backend_for("target-b")?;
    let root = temp_root(&config, "target-reset")?;
    let scenario_result = async {
        write_local_file(&root, "target.md", b"target binding")?;
        let first_sync = execute_remote_sync(&root, &first).await?;
        assert_summary(&first_sync, 1, 0, 0)?;
        let first_fingerprint = manifest_fingerprint(&root)?;

        let second_sync = execute_remote_sync(&root, &second).await?;
        assert_summary(&second_sync, 1, 0, 0)?;
        let second_fingerprint = manifest_fingerprint(&root)?;
        if first_fingerprint == second_fingerprint {
            return Err("Changing the S3 prefix did not reset the manifest target".to_string());
        }
        if read_remote_file(&first, "target.md").await? != b"target binding"
            || read_remote_file(&second, "target.md").await? != b"target binding"
        {
            return Err("Target reset did not preserve isolated prefix objects".to_string());
        }
        assert_manifest_paths(&root, &["target.md"])?;
        assert_noop_sync(&root, &second).await
    }
    .await;

    let second_finish = finish_scenario(&config, &second, &[root], scenario_result).await;
    let first_cleanup = cleanup_backend_prefix(&first).await;
    match (second_finish, first_cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(format!(
            "Live S3 cleanup {} failed for the first target: {error}",
            config.run_id
        )),
        (Err(scenario), Err(cleanup)) => Err(format!(
            "{scenario}; first target cleanup also failed: {cleanup}"
        )),
    }
}

async fn run_malformed_manifest_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("malformed-manifest")?;
    let root = temp_root(&config, "malformed-manifest")?;
    let scenario_result = async {
        write_local_file(&root, "local.md", b"local unchanged")?;
        backend
            .upload("remote.md", b"remote unchanged", None)
            .await?;
        let metadata = live_state_root(&root);
        fs::create_dir_all(&metadata)
            .map_err(|error| format!("Failed to create malformed manifest folder: {error}"))?;
        let malformed = b"{not valid json";
        fs::write(metadata.join("s3-manifest.json"), malformed)
            .map_err(|error| format!("Failed to write malformed manifest: {error}"))?;
        let local_before = snapshot_local_files(&root)?;
        let remote_before = backend.list_files().await?;

        if execute_remote_sync(&root, &backend).await.is_ok() {
            return Err("Malformed S3 manifest unexpectedly synchronized".to_string());
        }
        let local_after = snapshot_local_files(&root)?;
        let remote_after = backend.list_files().await?;
        let manifest_after = fs::read(metadata.join("s3-manifest.json"))
            .map_err(|error| format!("Failed to re-read malformed manifest: {error}"))?;
        if local_before != local_after
            || remote_before != remote_after
            || manifest_after != malformed
        {
            return Err(
                "Malformed S3 manifest changed local, remote, or manifest state".to_string(),
            );
        }
        Ok(())
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_ignored_directories_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("ignored-directories")?;
    let root = temp_root(&config, "ignored-directories")?;
    let scenario_result = async {
        write_local_file(&root, "included.md", b"included")?;
        for directory in [
            ".git",
            ".qingyu",
            ".markra-sync",
            "build",
            "dist",
            "node_modules",
            "target",
        ] {
            write_local_file(
                &root,
                &format!("{directory}/ignored.md"),
                directory.as_bytes(),
            )?;
        }

        let summary = execute_remote_sync(&root, &backend).await?;
        assert_summary(&summary, 1, 0, 0)?;
        let remote = backend.list_files().await?;
        if remote.keys().cloned().collect::<Vec<_>>() != vec!["included.md".to_string()] {
            return Err(format!(
                "Ignored directories leaked into MinIO: {:?}",
                remote.keys().collect::<Vec<_>>()
            ));
        }
        assert_manifest_paths(&root, &["included.md"])?;
        assert_noop_sync(&root, &backend).await
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_checkpoint_recovery_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = FailAfterUploadBackend::new(config.backend_for("checkpoint-recovery")?, 1);
    let root = temp_root(&config, "checkpoint-recovery")?;
    let scenario_result = async {
        write_local_file(&root, "a.md", b"first")?;
        write_local_file(&root, "b.md", b"second")?;
        if execute_remote_sync(&root, &backend).await.is_ok() {
            return Err("Injected S3 upload failure unexpectedly succeeded".to_string());
        }
        let after_failure = backend.inner.list_files().await?;
        if after_failure.len() != 1 || !after_failure.contains_key("a.md") {
            return Err("Checkpoint failure did not leave exactly the first upload".to_string());
        }
        let first_identity = after_failure["a.md"].identity.clone();
        assert_manifest_paths(&root, &["a.md"])?;

        let retry = execute_remote_sync(&root, &backend.inner).await?;
        assert_summary(&retry, 1, 0, 0)?;
        let after_retry = backend.inner.list_files().await?;
        if after_retry.len() != 2
            || after_retry.get("a.md").map(|file| file.identity.as_str())
                != Some(first_identity.as_str())
            || !after_retry.contains_key("b.md")
        {
            return Err(
                "Checkpoint retry replayed the first upload or missed the second".to_string(),
            );
        }
        assert_manifest_paths(&root, &["a.md", "b.md"])?;
        assert_noop_sync(&root, &backend.inner).await
    }
    .await;
    finish_scenario(&config, &backend.inner, &[root], scenario_result).await
}

async fn run_concurrent_remote_change_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = MutateBeforeUploadBackend::new(config.backend_for("concurrent-change")?);
    let root = temp_root(&config, "concurrent-change")?;
    let scenario_result = async {
        write_local_file(&root, "note.md", b"baseline")?;
        execute_remote_sync(&root, &backend.inner).await?;
        let manifest_before = fs::read(live_state_root(&root).join("s3-manifest.json"))
            .map_err(|error| format!("Failed to snapshot concurrent manifest: {error}"))?;
        write_local_file(&root, "note.md", b"local planned version")?;
        let error = execute_remote_sync(&root, &backend)
            .await
            .expect_err("concurrent remote mutation must reject the stale upload plan");
        if error.safe_code() != "s3-object-changed" {
            return Err(format!(
                "Concurrent remote mutation returned an unexpected error: {error}"
            ));
        }
        if read_remote_file(&backend.inner, "note.md").await? != b"concurrent remote version" {
            return Err("Stale upload overwrote the concurrent remote version".to_string());
        }
        if read_local_file(&root, "note.md")? != b"local planned version" {
            return Err("Rejected stale upload changed the local planned version".to_string());
        }
        let manifest_after = fs::read(live_state_root(&root).join("s3-manifest.json"))
            .map_err(|error| format!("Failed to re-read concurrent manifest: {error}"))?;
        if manifest_before != manifest_after {
            return Err("Rejected stale upload changed the manifest checkpoint".to_string());
        }
        Ok(())
    }
    .await;
    finish_scenario(&config, &backend.inner, &[root], scenario_result).await
}

async fn run_atomic_replace_failure_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("atomic-replace")?;
    let root = temp_root(&config, "atomic-replace")?;
    let scenario_result = async {
        backend
            .upload("blocked.md", b"remote download bytes", None)
            .await?;
        let hooks = RemoteSyncExecutionHooks::with_final_replace(Box::new(|target_path| {
            fs::create_dir(target_path)
                .map_err(|error| format!("Failed to inject atomic replacement race: {error}"))
        }));
        let result = execute_remote_sync_with_hooks(&root, &backend, hooks).await;
        let error = result.expect_err("atomic replacement onto a directory must fail");
        if !error.contains("atomic publish") {
            return Err(format!(
                "Atomic replacement returned an unexpected error: {error}"
            ));
        }
        if root.join(".blocked.md.markra-sync-tmp").exists() {
            return Err("Failed atomic replacement left a temporary file".to_string());
        }
        if !root.join("blocked.md").is_dir() {
            return Err("Failed atomic replacement changed the local target directory".to_string());
        }
        let sync_directory = live_state_root(&root).join("staging");
        for entry in fs::read_dir(&sync_directory)
            .map_err(|error| format!("Failed to inspect protected sync staging: {error}"))?
        {
            let entry = entry
                .map_err(|error| format!("Failed to inspect protected sync staging: {error}"))?;
            if entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with(".markra-sync-stage-")
            {
                return Err("Failed atomic replacement left protected staging".to_string());
            }
        }
        if sync_directory.join("s3-manifest.json").exists() {
            return Err("Failed atomic replacement created a local manifest".to_string());
        }
        if read_remote_file(&backend, "blocked.md").await? != b"remote download bytes" {
            return Err("Failed atomic replacement changed the MinIO object".to_string());
        }
        Ok(())
    }
    .await;
    finish_scenario(&config, &backend, &[root], scenario_result).await
}

async fn run_invalid_credentials_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let valid = config.backend_for("invalid-credentials")?;
    let mut invalid_settings = config.settings_for("invalid-credentials");
    invalid_settings.secret_access_key = "invalid-live-test-secret".to_string();
    let invalid = S3Backend::new(invalid_settings)?;
    let root = temp_root(&config, "invalid-credentials")?;
    let scenario_result = async {
        write_local_file(&root, "local.md", b"must stay local")?;
        let local_before = snapshot_local_files(&root)?;
        let error = execute_remote_sync(&root, &invalid)
            .await
            .expect_err("invalid MinIO credentials must fail");
        if error.contains(&config.access_key_id)
            || error.contains(&config.secret_access_key)
            || error.contains("invalid-live-test-secret")
            || error.to_ascii_lowercase().contains("authorization")
        {
            return Err("Invalid credential error exposed credential material".to_string());
        }
        if snapshot_local_files(&root)? != local_before {
            return Err("Invalid credentials changed local files".to_string());
        }
        if !valid.list_files().await?.is_empty() {
            return Err("Invalid credentials created a MinIO object".to_string());
        }
        Ok(())
    }
    .await;
    finish_scenario(&config, &valid, &[root], scenario_result).await
}

async fn run_missing_bucket_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let valid = config.backend_for("missing-bucket")?;
    let mut missing_settings = config.settings_for("missing-bucket");
    missing_settings.bucket = format!("markra-missing-{}", config.run_id);
    let missing = S3Backend::new(missing_settings)?;
    let root = temp_root(&config, "missing-bucket")?;
    let scenario_result = async {
        write_local_file(&root, "local.md", b"must stay local")?;
        let local_before = snapshot_local_files(&root)?;
        let error = execute_remote_sync(&root, &missing)
            .await
            .expect_err("missing MinIO bucket must fail");
        if error.safe_code() != "s3-list-http-failed"
            || error.details().and_then(|details| details.http_status) != Some(404)
        {
            return Err(format!(
                "Missing bucket returned an unexpected error: {error}"
            ));
        }
        if snapshot_local_files(&root)? != local_before {
            return Err("Missing bucket changed local files".to_string());
        }
        if live_state_root(&root).join("s3-manifest.json").exists() {
            return Err("Missing bucket created a local manifest".to_string());
        }
        if !valid.list_files().await?.is_empty() {
            return Err("Missing bucket scenario changed the configured valid bucket".to_string());
        }
        Ok(())
    }
    .await;
    finish_scenario(&config, &valid, &[root], scenario_result).await
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_harness_uploads_reads_and_cleans_isolated_object() {
    tauri::async_runtime::block_on(run_harness_smoke()).expect("live MinIO harness smoke test");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_named_notebooks_catalog_restore_switch_and_cleanup_exact_root() {
    tauri::async_runtime::block_on(run_named_notebooks_scenario())
        .expect("live MinIO named-notebook scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_connection_test_preserves_remote_snapshot() {
    tauri::async_runtime::block_on(run_read_only_connection_test_scenario())
        .expect("live MinIO read-only connection-test scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_uploads_local_create_and_downloads_remote_create() {
    tauri::async_runtime::block_on(run_create_read_scenario())
        .expect("live MinIO create and read scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_propagates_local_and_remote_updates() {
    tauri::async_runtime::block_on(run_update_scenario())
        .expect("live MinIO local and remote update scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_handles_nested_unicode_reserved_empty_and_binary_files() {
    tauri::async_runtime::block_on(run_topology_scenario())
        .expect("live MinIO object topology scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_paginates_more_than_one_thousand_objects() {
    tauri::async_runtime::block_on(run_pagination_scenario())
        .expect("live MinIO pagination scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_propagates_local_delete_to_remote() {
    tauri::async_runtime::block_on(run_local_delete_scenario())
        .expect("live MinIO local deletion scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_propagates_remote_delete_to_local() {
    tauri::async_runtime::block_on(run_remote_delete_scenario())
        .expect("live MinIO remote deletion scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_preserves_remote_change_when_local_was_deleted() {
    tauri::async_runtime::block_on(run_remote_survivor_scenario())
        .expect("live MinIO changed remote survivor scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_preserves_local_change_when_remote_was_deleted() {
    tauri::async_runtime::block_on(run_local_survivor_scenario())
        .expect("live MinIO changed local survivor scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_preserves_both_first_sync_versions_as_conflict() {
    tauri::async_runtime::block_on(run_first_sync_conflict_scenario())
        .expect("live MinIO first-sync conflict scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_preserves_both_changed_versions_as_conflict() {
    tauri::async_runtime::block_on(run_changed_conflict_scenario())
        .expect("live MinIO changed-both conflict scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_uses_unique_conflict_name_and_does_not_repeat_conflict() {
    tauri::async_runtime::block_on(run_conflict_collision_scenario())
        .expect("live MinIO conflict collision scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_resets_manifest_when_prefix_target_changes() {
    tauri::async_runtime::block_on(run_target_reset_scenario())
        .expect("live MinIO target reset scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_rejects_malformed_manifest_without_mutation() {
    tauri::async_runtime::block_on(run_malformed_manifest_scenario())
        .expect("live MinIO malformed manifest scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_never_uploads_fixed_ignored_directories() {
    tauri::async_runtime::block_on(run_ignored_directories_scenario())
        .expect("live MinIO ignored directories scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_checkpoints_completed_upload_before_injected_failure() {
    tauri::async_runtime::block_on(run_checkpoint_recovery_scenario())
        .expect("live MinIO checkpoint recovery scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_rejects_remote_change_between_plan_and_upload() {
    tauri::async_runtime::block_on(run_concurrent_remote_change_scenario())
        .expect("live MinIO concurrent remote change scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_removes_temp_file_when_atomic_replace_fails() {
    tauri::async_runtime::block_on(run_atomic_replace_failure_scenario())
        .expect("live MinIO atomic replacement failure scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_redacts_invalid_credentials() {
    tauri::async_runtime::block_on(run_invalid_credentials_scenario())
        .expect("live MinIO invalid credentials scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_missing_bucket_does_not_mutate_local_state() {
    tauri::async_runtime::block_on(run_missing_bucket_scenario())
        .expect("live MinIO missing bucket scenario");
}
