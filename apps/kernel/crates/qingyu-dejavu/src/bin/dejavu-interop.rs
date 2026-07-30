use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use qingyu_dejavu::{
    Cloud, CloudError, CloudObject, CloudOperation, CloudUploadSource, Device, LocalCloud,
    NoopWorkingTreeCoordinator, Repo, RepoError, RepoOptions, RepoPaths,
};
use serde::{Deserialize, Serialize};

const MAX_REQUEST_BYTES: u64 = 1024 * 1024;
const LATEST_REF: &str = "refs/latest";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation: String,
    device_id: String,
    data_path: String,
    repo_path: String,
    history_path: String,
    temp_path: String,
    key_base64: String,
    cloud_root: String,
    fail_before_ref_publication: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    index_id: Option<String>,
    upserts: usize,
    removes: usize,
    conflicts: usize,
    error_code: Option<&'static str>,
}

struct ValidatedRequest {
    operation: Operation,
    device_id: String,
    paths: RepoPaths,
    key: [u8; 32],
    cloud_root: String,
    fail_before_ref_publication: bool,
}

enum Operation {
    IndexAndSync,
    Inspect,
}

struct FailBeforeLatestCloud {
    inner: Arc<LocalCloud>,
    enabled: bool,
    failed: AtomicBool,
}

impl FailBeforeLatestCloud {
    fn new(inner: Arc<LocalCloud>, enabled: bool) -> Self {
        Self {
            inner,
            enabled,
            failed: AtomicBool::new(false),
        }
    }

    fn reject_latest(&self, key: &str) -> Result<(), CloudError> {
        if self.enabled && key == LATEST_REF && !self.failed.swap(true, Ordering::SeqCst) {
            return Err(CloudError::Injected(CloudOperation::Put));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Cloud for FailBeforeLatestCloud {
    async fn get_bounded(&self, key: &str, max_bytes: u64) -> Result<Vec<u8>, CloudError> {
        self.inner.get_bounded(key, max_bytes).await
    }

    async fn download_to(
        &self,
        key: &str,
        destination: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
    ) -> Result<u64, CloudError> {
        self.inner.download_to(key, destination).await
    }

    async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError> {
        self.reject_latest(key)?;
        self.inner.put(key, bytes, overwrite).await
    }

    async fn upload_from(
        &self,
        key: &str,
        source: &dyn CloudUploadSource,
        overwrite: bool,
    ) -> Result<u64, CloudError> {
        self.reject_latest(key)?;
        self.inner.upload_from(key, source, overwrite).await
    }

    async fn remove(&self, key: &str) -> Result<(), CloudError> {
        self.inner.remove(key).await
    }

    async fn list(&self, prefix: &str) -> Result<Vec<CloudObject>, CloudError> {
        self.inner.list(prefix).await
    }

    async fn available_size(&self) -> Result<u64, CloudError> {
        self.inner.available_size().await
    }
}

#[tokio::main]
async fn main() {
    let result = read_request().and_then(validate_request);
    let response = match result {
        Ok(request) => run(request).await,
        Err(error_code) => Err(error_code),
    };
    let (response, failed) = match response {
        Ok(response) => (response, false),
        Err(error_code) => (
            Response {
                index_id: None,
                upserts: 0,
                removes: 0,
                conflicts: 0,
                error_code: Some(error_code),
            },
            true,
        ),
    };
    emit_response(&response);
    if failed {
        eprintln!(
            "dejavu-interop: request failed ({})",
            response.error_code.unwrap_or("failed")
        );
        std::process::exit(1);
    }
}

fn read_request() -> Result<Request, &'static str> {
    let mut input = Vec::new();
    io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut input)
        .map_err(|_| "request_read_failed")?;
    if input.len() as u64 > MAX_REQUEST_BYTES {
        return Err("request_too_large");
    }
    serde_json::from_slice(&input).map_err(|_| "request_invalid")
}

fn validate_request(request: Request) -> Result<ValidatedRequest, &'static str> {
    let operation = match request.operation.as_str() {
        "index-and-sync" => Operation::IndexAndSync,
        "inspect" => Operation::Inspect,
        _ => return Err("operation_invalid"),
    };
    if request.device_id.is_empty() {
        return Err("request_invalid");
    }
    for path in [
        &request.data_path,
        &request.repo_path,
        &request.history_path,
        &request.temp_path,
        &request.cloud_root,
    ] {
        if !Path::new(path).is_absolute() {
            return Err("path_invalid");
        }
    }
    let key_bytes = STANDARD
        .decode(&request.key_base64)
        .map_err(|_| "key_invalid")?;
    let key: [u8; 32] = key_bytes.try_into().map_err(|_| "key_invalid")?;
    if STANDARD.encode(key) != request.key_base64 {
        return Err("key_invalid");
    }
    Ok(ValidatedRequest {
        operation,
        device_id: request.device_id,
        paths: RepoPaths {
            data: request.data_path.into(),
            repo: request.repo_path.into(),
            history: request.history_path.into(),
            temp: request.temp_path.into(),
        },
        key,
        cloud_root: request.cloud_root,
        fail_before_ref_publication: request.fail_before_ref_publication,
    })
}

async fn run(request: ValidatedRequest) -> Result<Response, &'static str> {
    let repo = Repo::open(
        request.paths,
        Device {
            id: request.device_id.clone(),
            name: request.device_id,
            os: std::env::consts::OS.to_owned(),
        },
        request.key,
        RepoOptions::default(),
    )
    .map_err(repo_error_code)?;
    match request.operation {
        Operation::Inspect => {
            let index_id = repo
                .latest()
                .map_err(repo_error_code)?
                .map(|index| index.id);
            Ok(Response {
                index_id,
                upserts: 0,
                removes: 0,
                conflicts: 0,
                error_code: None,
            })
        }
        Operation::IndexAndSync => {
            let local =
                Arc::new(LocalCloud::new(request.cloud_root).map_err(|_| "cloud_open_failed")?);
            let cloud: Arc<dyn Cloud> = Arc::new(FailBeforeLatestCloud::new(
                local,
                request.fail_before_ref_publication,
            ));
            let (merged, _) = repo
                .sync(cloud, Arc::new(NoopWorkingTreeCoordinator))
                .await
                .map_err(repo_error_code)?;
            let index_id = repo
                .latest()
                .map_err(repo_error_code)?
                .map(|index| index.id);
            Ok(Response {
                index_id,
                upserts: merged.upserts.len(),
                removes: merged.removes.len(),
                conflicts: merged.conflicts.len(),
                error_code: None,
            })
        }
    }
}

fn repo_error_code(error: RepoError) -> &'static str {
    match error {
        RepoError::Cloud(CloudError::Injected(CloudOperation::Put)) => "ref_publication_injected",
        RepoError::EmptyIndex => "empty_index",
        RepoError::UnsafePath => "path_invalid",
        RepoError::RepositoryBusy => "repository_busy",
        _ => "operation_failed",
    }
}

fn emit_response(response: &Response) {
    let bytes = serde_json::to_vec(response).unwrap_or_else(|_| {
        br#"{"indexId":null,"upserts":0,"removes":0,"conflicts":0,"errorCode":"response_failed"}"#
            .to_vec()
    });
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(&bytes);
    let _ = stdout.write_all(b"\n");
}
