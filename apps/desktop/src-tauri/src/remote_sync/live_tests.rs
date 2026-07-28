use std::env;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use super::backend::{RemoteSyncBackend, ValidRemoteRoot};
use super::s3_backend::{S3Backend, S3SyncSettings};
use super::{create_webdav_backend, create_webdav_backend_at_validated_prefix, WebDavSyncSettings};
use crate::notebook_scope::notes_remote_prefix;

static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

    fn backend_for(&self, scenario: &str) -> Result<S3Backend, String> {
        S3Backend::new(S3SyncSettings {
            access_key_id: self.access_key_id.clone(),
            bucket: self.bucket.clone(),
            endpoint_url: self.endpoint_url.clone(),
            region: self.region.clone(),
            remote_path: format!("{}/{}/{scenario}/app", self.prefix_root, self.run_id),
            secret_access_key: self.secret_access_key.clone(),
        })
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
    if backend.list_files().await?.is_empty() {
        Ok(())
    } else {
        Err("Live S3 cleanup left objects below the isolated settings prefix".to_string())
    }
}

async fn finish_s3_scenario(
    config: &LiveS3Config,
    backend: &S3Backend,
    scenario: Result<(), String>,
) -> Result<(), String> {
    let cleanup = cleanup_backend_prefix(backend).await;
    match (scenario, cleanup) {
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

async fn run_protected_settings_transport_smoke() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("protected-settings-transport")?;
    let scenario = async {
        let expected = br#"{"language":"en","themeMode":"system"}"#;
        backend.upload("settings.json", expected, None).await?;
        let files = backend.list_files().await?;
        let settings = files
            .get("settings.json")
            .ok_or_else(|| "Live S3 settings object was not listed after upload".to_string())?;
        if backend
            .download("settings.json", &settings.identity)
            .await?
            != expected
        {
            return Err("Live S3 settings bytes did not match the uploaded bytes".to_string());
        }
        Ok(())
    }
    .await;
    finish_s3_scenario(&config, &backend, scenario).await
}

async fn run_read_only_connection_test_scenario() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = config.backend_for("connection-test")?;
    let scenario = async {
        backend
            .upload("settings.json", br#"{"language":"en"}"#, None)
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
    finish_s3_scenario(&config, &backend, scenario).await
}

async fn verify_isolated_prefix_root_is_empty() -> Result<(), String> {
    let config = LiveS3Config::from_env()?;
    let backend = S3Backend::new(S3SyncSettings {
        access_key_id: config.access_key_id,
        bucket: config.bucket,
        endpoint_url: config.endpoint_url,
        region: config.region,
        remote_path: config.prefix_root,
        secret_access_key: config.secret_access_key,
    })?;
    if backend.list_files().await?.is_empty() {
        Ok(())
    } else {
        Err("Live S3 isolated prefix root still contains objects".to_string())
    }
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_s3_protected_settings_transport_round_trips_and_cleans() {
    tauri::async_runtime::block_on(run_protected_settings_transport_smoke())
        .expect("live MinIO protected-settings transport smoke test");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_s3_connection_test_preserves_remote_snapshot() {
    tauri::async_runtime::block_on(run_read_only_connection_test_scenario())
        .expect("live MinIO read-only connection-test scenario");
}

#[test]
#[ignore = "requires MARKRA_TEST_S3_* and a real MinIO server"]
fn live_minio_s3_isolated_prefix_root_is_empty() {
    tauri::async_runtime::block_on(verify_isolated_prefix_root_is_empty())
        .expect("live MinIO isolated prefix root should be empty");
}
