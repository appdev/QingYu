//! Authenticated MCP-to-Kernel adapter staged for coordinator-owned runtime wiring.

use std::{fmt, future::Future, pin::Pin, sync::Arc, time::Duration};

use qingyu_kernel::{
    contract::{
        ApiErrorEnvelope, CreateDocumentRequest, CreatedDocumentDto, DeleteDocumentRequest,
        DocumentContentDto, DocumentEntryDto, DocumentId, DocumentPageDto, ErrorCode, ErrorDetails,
        InstanceId, ListDocumentsQuery, MoveDocumentRequest, PatchSettingsRequest,
        PatchSyncConfigRequest, PositiveSafeInteger, RequestId, SearchPageDto,
        SearchWorkspaceQuery, SettingsSnapshotDto, SyncConfigViewDto, SyncConnectionTestDto,
        SyncRunAcceptedDto, SyncRunStatusDto, SyncStatusDto, TestSyncConnectionRequest,
        TriggerSyncRunRequest, UpdateDocumentRequest, WorkspaceDto,
    },
    error::{http_status_for_error_code, safe_error_envelope},
};
use reqwest::{header, redirect::Policy, Client, ClientBuilder, Method};
use serde::{de::DeserializeOwned, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::config::McpConfigManager;
use crate::{
    kernel_host::kernel_endpoint_record::KernelEndpointRecordReader,
    kernel_host::{NativeKernelAccess, NativeKernelCredentialLease},
};

const MAXIMUM_KERNEL_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_KERNEL_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

trait KernelRequestTimeoutSource: Send + Sync + 'static {
    fn current_timeout(&self) -> Result<Duration, KernelHttpFailure>;
}

struct FixedKernelRequestTimeout(Duration);

impl KernelRequestTimeoutSource for FixedKernelRequestTimeout {
    fn current_timeout(&self) -> Result<Duration, KernelHttpFailure> {
        Ok(self.0)
    }
}

impl KernelRequestTimeoutSource for McpConfigManager {
    fn current_timeout(&self) -> Result<Duration, KernelHttpFailure> {
        let seconds = self
            .snapshot()
            .map_err(|_| KernelHttpFailure)?
            .config
            .tool_timeout_secs;
        if !(5..=600).contains(&seconds) {
            return Err(KernelHttpFailure);
        }
        Ok(Duration::from_secs(seconds))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EndpointReadFailure;

trait KernelEndpointSource: Send + Sync + 'static {
    fn read(&self) -> Result<Option<KernelEndpointAccess>, EndpointReadFailure>;
}

#[derive(Clone)]
struct EndpointRecordSource(KernelEndpointRecordReader);

impl KernelEndpointSource for EndpointRecordSource {
    fn read(&self) -> Result<Option<KernelEndpointAccess>, EndpointReadFailure> {
        self.0
            .read()
            .map_err(|_| EndpointReadFailure)
            .map(|access| access.map(KernelEndpointAccess::from_native))
    }
}

#[derive(Clone)]
struct KernelEndpointAccess {
    generation: u64,
    port: u16,
    instance_id: InstanceId,
    credential: KernelCredentialLease,
}

impl KernelEndpointAccess {
    fn from_native(access: NativeKernelAccess) -> Self {
        Self {
            generation: access.endpoint.generation,
            port: access.endpoint.port,
            instance_id: access.endpoint.instance_id,
            credential: KernelCredentialLease::Native(access.credential),
        }
    }

    fn same_endpoint(&self, other: &Self) -> bool {
        self.generation == other.generation
            && self.port == other.port
            && self.instance_id == other.instance_id
    }

    fn with_secret<T>(&self, use_secret: impl FnOnce(&str) -> T) -> Result<T, KernelHttpFailure> {
        self.credential.with_secret(use_secret)
    }

    #[cfg(test)]
    fn for_test(generation: u64, port: u16, instance_id: Uuid, secret: &str) -> Self {
        Self {
            generation,
            port,
            instance_id: InstanceId::new(instance_id),
            credential: KernelCredentialLease::Test(Arc::new(TestCredentialLease {
                secret: secret.to_owned(),
            })),
        }
    }
}

impl fmt::Debug for KernelEndpointAccess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KernelEndpointAccess")
            .field("generation", &self.generation)
            .field("port", &self.port)
            .field("instance_id", &self.instance_id)
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
enum KernelCredentialLease {
    Native(NativeKernelCredentialLease),
    #[cfg(test)]
    Test(Arc<TestCredentialLease>),
}

impl KernelCredentialLease {
    fn with_secret<T>(&self, use_secret: impl FnOnce(&str) -> T) -> Result<T, KernelHttpFailure> {
        match self {
            Self::Native(lease) => lease.with_secret(use_secret).map_err(|_| KernelHttpFailure),
            #[cfg(test)]
            Self::Test(lease) => Ok(use_secret(&lease.secret)),
        }
    }
}

#[cfg(test)]
struct TestCredentialLease {
    secret: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KernelHttpMethod {
    Get,
    Post,
    Put,
    Patch,
}

impl KernelHttpMethod {
    fn reqwest(self) -> Method {
        match self {
            Self::Get => Method::GET,
            Self::Post => Method::POST,
            Self::Put => Method::PUT,
            Self::Patch => Method::PATCH,
        }
    }
}

#[derive(Clone)]
struct KernelHttpRequest {
    method: KernelHttpMethod,
    path: String,
    query: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    expected_status: u16,
}

impl KernelHttpRequest {
    fn get(path: &str, query: Vec<(String, String)>) -> Self {
        Self {
            method: KernelHttpMethod::Get,
            path: path.to_owned(),
            query,
            body: None,
            expected_status: 200,
        }
    }

    fn json<Request: Serialize>(
        method: KernelHttpMethod,
        path: String,
        body: &Request,
        expected_status: u16,
    ) -> Result<Self, McpKernelFailure> {
        Ok(Self {
            method,
            path,
            query: Vec::new(),
            body: Some(serde_json::to_vec(body).map_err(|_| McpKernelFailure::InvalidRequest)?),
            expected_status,
        })
    }

    fn method(&self) -> KernelHttpMethod {
        self.method
    }

    fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Debug for KernelHttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KernelHttpRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("query", &self.query)
            .field("body", &self.body.as_ref().map(|_| "[REDACTED]"))
            .field("expected_status", &self.expected_status)
            .finish()
    }
}

struct KernelHttpResponse {
    status: u16,
    request_id: RequestId,
    retry_after_seconds: Option<u64>,
    body: Option<Vec<u8>>,
}

impl KernelHttpResponse {
    #[cfg(test)]
    fn for_test(status: u16, request_id: &str, body: Option<Vec<u8>>) -> Self {
        Self {
            status,
            request_id: RequestId::new(Uuid::parse_str(request_id).expect("test request UUID")),
            retry_after_seconds: None,
            body,
        }
    }

    #[cfg(test)]
    fn for_test_with_retry_after(
        status: u16,
        request_id: &str,
        retry_after_seconds: u64,
        body: Option<Vec<u8>>,
    ) -> Self {
        Self {
            status,
            request_id: RequestId::new(Uuid::parse_str(request_id).expect("test request UUID")),
            retry_after_seconds: Some(retry_after_seconds),
            body,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KernelHttpFailure;

type KernelHttpFuture<'a> =
    Pin<Box<dyn Future<Output = Result<KernelHttpResponse, KernelHttpFailure>> + Send + 'a>>;

pub(crate) type McpKernelFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, McpKernelFailure>> + Send + 'a>>;

/// The only production data-plane seam used by MCP tools. Implementations must
/// preserve cancellation and fail closed when the child Kernel endpoint is not
/// current. Tests use an in-memory fake rather than a legacy fallback.
pub(crate) trait McpKernelPort: Send + Sync + 'static {
    fn get_workspace<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, WorkspaceDto>;
    fn list_documents<'a>(
        &'a self,
        query: &'a ListDocumentsQuery,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentPageDto>;
    fn search_documents<'a>(
        &'a self,
        query: &'a SearchWorkspaceQuery,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SearchPageDto>;
    fn create_document<'a>(
        &'a self,
        request: &'a CreateDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, CreatedDocumentDto>;
    fn get_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentContentDto>;
    fn update_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        request: &'a UpdateDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentContentDto>;
    fn move_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        request: &'a MoveDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentEntryDto>;
    fn delete_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        request: &'a DeleteDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, ()>;
    fn get_settings<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SettingsSnapshotDto>;
    fn patch_settings<'a>(
        &'a self,
        request: &'a PatchSettingsRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SettingsSnapshotDto>;
    fn get_sync_config<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncConfigViewDto>;
    fn patch_sync_config<'a>(
        &'a self,
        request: &'a PatchSyncConfigRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncConfigViewDto>;
    fn test_sync_connection<'a>(
        &'a self,
        request: &'a TestSyncConnectionRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncConnectionTestDto>;
    fn get_sync_status<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncStatusDto>;
    fn trigger_sync_run<'a>(
        &'a self,
        request: &'a TriggerSyncRunRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncRunAcceptedDto>;
    fn get_sync_run<'a>(
        &'a self,
        run_id: qingyu_kernel::contract::RunId,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncRunStatusDto>;
}

trait KernelHttpTransport: Send + Sync + 'static {
    fn send<'a>(
        &'a self,
        endpoint: &'a KernelEndpointAccess,
        request: KernelHttpRequest,
    ) -> KernelHttpFuture<'a>;
}

struct ReqwestKernelHttpTransport {
    client: Client,
}

impl ReqwestKernelHttpTransport {
    fn new() -> Result<Self, McpKernelFailure> {
        secure_kernel_http_client(Client::builder()).map(|client| Self { client })
    }
}

fn secure_kernel_http_client(builder: ClientBuilder) -> Result<Client, McpKernelFailure> {
    builder
        .no_proxy()
        .redirect(Policy::none())
        .build()
        .map_err(|_| McpKernelFailure::TransportUnavailable)
}

impl KernelHttpTransport for ReqwestKernelHttpTransport {
    fn send<'a>(
        &'a self,
        endpoint: &'a KernelEndpointAccess,
        request: KernelHttpRequest,
    ) -> KernelHttpFuture<'a> {
        Box::pin(async move {
            let url = format!("http://127.0.0.1:{}{}", endpoint.port, request.path);
            let mut builder = self
                .client
                .request(request.method.reqwest(), url)
                .query(&request.query);
            if let Some(body) = request.body {
                builder = builder
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body);
            }
            builder = endpoint.with_secret(|secret| builder.bearer_auth(secret))?;
            let mut response = builder.send().await.map_err(|_| KernelHttpFailure)?;
            let status = response.status().as_u16();
            let request_id = exact_request_id(response.headers())?;
            let retry_after_seconds = retry_after_seconds(response.headers());
            let content_length = response.content_length().unwrap_or(0);
            if content_length > MAXIMUM_KERNEL_RESPONSE_BYTES as u64 {
                return Err(KernelHttpFailure);
            }
            let mut body = Vec::with_capacity(content_length as usize);
            while let Some(chunk) = response.chunk().await.map_err(|_| KernelHttpFailure)? {
                let next_length = body
                    .len()
                    .checked_add(chunk.len())
                    .ok_or(KernelHttpFailure)?;
                if next_length > MAXIMUM_KERNEL_RESPONSE_BYTES {
                    return Err(KernelHttpFailure);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(KernelHttpResponse {
                status,
                request_id,
                retry_after_seconds,
                body: (!body.is_empty()).then_some(body),
            })
        })
    }
}

fn exact_request_id(headers: &header::HeaderMap) -> Result<RequestId, KernelHttpFailure> {
    let values = headers.get_all("x-request-id");
    if values.iter().count() != 1 {
        return Err(KernelHttpFailure);
    }
    let request_id = values
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(KernelHttpFailure)?;
    Ok(RequestId::new(request_id))
}

fn retry_after_seconds(headers: &header::HeaderMap) -> Option<u64> {
    let values = headers.get_all(header::RETRY_AFTER);
    if values.iter().count() != 1 {
        return None;
    }
    let value = values.iter().next()?.to_str().ok()?;
    let bytes = value.as_bytes();
    if bytes
        .first()
        .is_none_or(|byte| !byte.is_ascii_digit() || *byte == b'0')
        || !bytes.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    value
        .parse::<u64>()
        .ok()
        .and_then(|value| PositiveSafeInteger::new(value).ok())
        .map(PositiveSafeInteger::get)
}

#[derive(Clone)]
pub(crate) struct McpKernelClient {
    endpoints: Arc<dyn KernelEndpointSource>,
    transport: Arc<dyn KernelHttpTransport>,
    request_timeout: Arc<dyn KernelRequestTimeoutSource>,
}

impl McpKernelClient {
    pub(crate) fn new(endpoints: KernelEndpointRecordReader) -> Result<Self, McpKernelFailure> {
        Ok(Self {
            endpoints: Arc::new(EndpointRecordSource(endpoints)),
            transport: Arc::new(ReqwestKernelHttpTransport::new()?),
            request_timeout: Arc::new(FixedKernelRequestTimeout(DEFAULT_KERNEL_REQUEST_TIMEOUT)),
        })
    }

    pub(crate) fn with_configured_request_timeout(mut self, config: Arc<McpConfigManager>) -> Self {
        self.request_timeout = config;
        self
    }

    #[cfg(test)]
    fn with_transport_for_test(
        endpoints: impl KernelEndpointSource,
        transport: impl KernelHttpTransport,
    ) -> Self {
        Self {
            endpoints: Arc::new(endpoints),
            transport: Arc::new(transport),
            request_timeout: Arc::new(FixedKernelRequestTimeout(DEFAULT_KERNEL_REQUEST_TIMEOUT)),
        }
    }

    pub(crate) async fn list_documents(
        &self,
        query: &ListDocumentsQuery,
        cancellation: &CancellationToken,
    ) -> Result<DocumentPageDto, McpKernelFailure> {
        let mut parameters = Vec::new();
        if let Some(cursor) = query.cursor.as_ref() {
            parameters.push(("cursor".to_owned(), cursor.as_str().to_owned()));
        }
        if let Some(limit) = query.limit {
            parameters.push(("limit".to_owned(), limit.get().to_string()));
        }
        if !query.parent.as_str().is_empty() {
            parameters.push(("parent".to_owned(), query.parent.as_str().to_owned()));
        }
        self.request_json(
            KernelHttpRequest::get("/api/v1/documents", parameters),
            cancellation,
        )
        .await
    }

    pub(crate) async fn get_workspace(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<WorkspaceDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::get("/api/v1/workspace", Vec::new()),
            cancellation,
        )
        .await
    }

    pub(crate) async fn search_documents(
        &self,
        query: &SearchWorkspaceQuery,
        cancellation: &CancellationToken,
    ) -> Result<SearchPageDto, McpKernelFailure> {
        let mut parameters = vec![("query".to_owned(), query.query.as_str().to_owned())];
        if let Some(cursor) = query.cursor.as_ref() {
            parameters.push(("cursor".to_owned(), cursor.as_str().to_owned()));
        }
        if let Some(limit) = query.limit {
            parameters.push(("limit".to_owned(), limit.get().to_string()));
        }
        self.request_json(
            KernelHttpRequest::get("/api/v1/search", parameters),
            cancellation,
        )
        .await
    }

    pub(crate) async fn create_document(
        &self,
        request: &CreateDocumentRequest,
        cancellation: &CancellationToken,
    ) -> Result<CreatedDocumentDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::json(
                KernelHttpMethod::Post,
                "/api/v1/documents".to_owned(),
                request,
                201,
            )?,
            cancellation,
        )
        .await
    }

    pub(crate) async fn get_document(
        &self,
        document_id: &DocumentId,
        cancellation: &CancellationToken,
    ) -> Result<DocumentContentDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::get(&document_path(document_id), Vec::new()),
            cancellation,
        )
        .await
    }

    pub(crate) async fn update_document(
        &self,
        document_id: &DocumentId,
        request: &UpdateDocumentRequest,
        cancellation: &CancellationToken,
    ) -> Result<DocumentContentDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::json(
                KernelHttpMethod::Put,
                document_path(document_id),
                request,
                200,
            )?,
            cancellation,
        )
        .await
    }

    pub(crate) async fn move_document(
        &self,
        document_id: &DocumentId,
        request: &MoveDocumentRequest,
        cancellation: &CancellationToken,
    ) -> Result<DocumentEntryDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::json(
                KernelHttpMethod::Post,
                format!("{}/move", document_path(document_id)),
                request,
                200,
            )?,
            cancellation,
        )
        .await
    }

    pub(crate) async fn delete_document(
        &self,
        document_id: &DocumentId,
        request: &DeleteDocumentRequest,
        cancellation: &CancellationToken,
    ) -> Result<(), McpKernelFailure> {
        self.request_empty(
            KernelHttpRequest::json(
                KernelHttpMethod::Post,
                format!("{}/delete", document_path(document_id)),
                request,
                204,
            )?,
            cancellation,
        )
        .await
    }

    pub(crate) async fn get_settings(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<SettingsSnapshotDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::get("/api/v1/settings", Vec::new()),
            cancellation,
        )
        .await
    }

    pub(crate) async fn patch_settings(
        &self,
        request: &PatchSettingsRequest,
        cancellation: &CancellationToken,
    ) -> Result<SettingsSnapshotDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::json(
                KernelHttpMethod::Patch,
                "/api/v1/settings".to_owned(),
                request,
                200,
            )?,
            cancellation,
        )
        .await
    }

    pub(crate) async fn get_sync_config(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<SyncConfigViewDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::get("/api/v1/sync/config", Vec::new()),
            cancellation,
        )
        .await
    }

    pub(crate) async fn patch_sync_config(
        &self,
        request: &PatchSyncConfigRequest,
        cancellation: &CancellationToken,
    ) -> Result<SyncConfigViewDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::json(
                KernelHttpMethod::Patch,
                "/api/v1/sync/config".to_owned(),
                request,
                200,
            )?,
            cancellation,
        )
        .await
    }

    pub(crate) async fn test_sync_connection(
        &self,
        request: &TestSyncConnectionRequest,
        cancellation: &CancellationToken,
    ) -> Result<SyncConnectionTestDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::json(
                KernelHttpMethod::Post,
                "/api/v1/sync/connection-test".to_owned(),
                request,
                200,
            )?,
            cancellation,
        )
        .await
    }

    pub(crate) async fn get_sync_status(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<SyncStatusDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::get("/api/v1/sync/status", Vec::new()),
            cancellation,
        )
        .await
    }

    pub(crate) async fn trigger_sync_run(
        &self,
        request: &TriggerSyncRunRequest,
        cancellation: &CancellationToken,
    ) -> Result<SyncRunAcceptedDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::json(
                KernelHttpMethod::Post,
                "/api/v1/sync/runs".to_owned(),
                request,
                202,
            )?,
            cancellation,
        )
        .await
    }

    pub(crate) async fn get_sync_run(
        &self,
        run_id: qingyu_kernel::contract::RunId,
        cancellation: &CancellationToken,
    ) -> Result<SyncRunStatusDto, McpKernelFailure> {
        self.request_json(
            KernelHttpRequest::get(
                &format!("/api/v1/sync/runs/{}", run_id.as_uuid()),
                Vec::new(),
            ),
            cancellation,
        )
        .await
    }

    async fn request_json<Response: DeserializeOwned>(
        &self,
        request: KernelHttpRequest,
        cancellation: &CancellationToken,
    ) -> Result<Response, McpKernelFailure> {
        let response = self.execute(request, cancellation).await?;
        let body = response.body.ok_or(McpKernelFailure::InvalidResponse)?;
        serde_json::from_slice(&body).map_err(|_| McpKernelFailure::InvalidResponse)
    }

    async fn request_empty(
        &self,
        request: KernelHttpRequest,
        cancellation: &CancellationToken,
    ) -> Result<(), McpKernelFailure> {
        let response = self.execute(request, cancellation).await?;
        if response.body.is_some() {
            return Err(McpKernelFailure::InvalidResponse);
        }
        Ok(())
    }

    async fn execute(
        &self,
        request: KernelHttpRequest,
        cancellation: &CancellationToken,
    ) -> Result<KernelHttpResponse, McpKernelFailure> {
        if cancellation.is_cancelled() {
            return Err(McpKernelFailure::RequestCancelled);
        }
        let request_timeout = self
            .request_timeout
            .current_timeout()
            .map_err(|_| McpKernelFailure::TransportUnavailable)?;
        let endpoint = self.read_endpoint(McpKernelFailure::EndpointUnavailable)?;
        endpoint
            .with_secret(|_| ())
            .map_err(|_| McpKernelFailure::EndpointUnavailable)?;
        let expected_status = request.expected_status;
        let response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(McpKernelFailure::RequestCancelled),
            _ = tokio::time::sleep(request_timeout) => {
                return Err(McpKernelFailure::TransportUnavailable);
            }
            response = self.transport.send(&endpoint, request) => {
                response.map_err(|_| McpKernelFailure::TransportUnavailable)?
            }
        };
        let current = self.read_endpoint(McpKernelFailure::EndpointStale)?;
        if !endpoint.same_endpoint(&current) || current.with_secret(|_| ()).is_err() {
            return Err(McpKernelFailure::EndpointStale);
        }
        if response.status != expected_status {
            return Err(api_failure(response));
        }
        Ok(response)
    }

    fn read_endpoint(
        &self,
        unavailable: McpKernelFailure,
    ) -> Result<KernelEndpointAccess, McpKernelFailure> {
        self.endpoints
            .read()
            .map_err(|_| unavailable.clone())?
            .filter(|endpoint| endpoint.port != 0)
            .ok_or(unavailable)
    }
}

impl McpKernelPort for McpKernelClient {
    fn get_workspace<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, WorkspaceDto> {
        Box::pin(async move { McpKernelClient::get_workspace(self, cancellation).await })
    }

    fn list_documents<'a>(
        &'a self,
        query: &'a ListDocumentsQuery,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentPageDto> {
        Box::pin(async move { McpKernelClient::list_documents(self, query, cancellation).await })
    }

    fn search_documents<'a>(
        &'a self,
        query: &'a SearchWorkspaceQuery,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SearchPageDto> {
        Box::pin(async move { McpKernelClient::search_documents(self, query, cancellation).await })
    }

    fn create_document<'a>(
        &'a self,
        request: &'a CreateDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, CreatedDocumentDto> {
        Box::pin(async move { McpKernelClient::create_document(self, request, cancellation).await })
    }

    fn get_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentContentDto> {
        Box::pin(
            async move { McpKernelClient::get_document(self, document_id, cancellation).await },
        )
    }

    fn update_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        request: &'a UpdateDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentContentDto> {
        Box::pin(async move {
            McpKernelClient::update_document(self, document_id, request, cancellation).await
        })
    }

    fn move_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        request: &'a MoveDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentEntryDto> {
        Box::pin(async move {
            McpKernelClient::move_document(self, document_id, request, cancellation).await
        })
    }

    fn delete_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        request: &'a DeleteDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, ()> {
        Box::pin(async move {
            McpKernelClient::delete_document(self, document_id, request, cancellation).await
        })
    }

    fn get_settings<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SettingsSnapshotDto> {
        Box::pin(async move { McpKernelClient::get_settings(self, cancellation).await })
    }

    fn patch_settings<'a>(
        &'a self,
        request: &'a PatchSettingsRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SettingsSnapshotDto> {
        Box::pin(async move { McpKernelClient::patch_settings(self, request, cancellation).await })
    }

    fn get_sync_config<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncConfigViewDto> {
        Box::pin(async move { McpKernelClient::get_sync_config(self, cancellation).await })
    }

    fn patch_sync_config<'a>(
        &'a self,
        request: &'a PatchSyncConfigRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncConfigViewDto> {
        Box::pin(
            async move { McpKernelClient::patch_sync_config(self, request, cancellation).await },
        )
    }

    fn test_sync_connection<'a>(
        &'a self,
        request: &'a TestSyncConnectionRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncConnectionTestDto> {
        Box::pin(
            async move { McpKernelClient::test_sync_connection(self, request, cancellation).await },
        )
    }

    fn get_sync_status<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncStatusDto> {
        Box::pin(async move { McpKernelClient::get_sync_status(self, cancellation).await })
    }

    fn trigger_sync_run<'a>(
        &'a self,
        request: &'a TriggerSyncRunRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncRunAcceptedDto> {
        Box::pin(
            async move { McpKernelClient::trigger_sync_run(self, request, cancellation).await },
        )
    }

    fn get_sync_run<'a>(
        &'a self,
        run_id: qingyu_kernel::contract::RunId,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncRunStatusDto> {
        Box::pin(async move { McpKernelClient::get_sync_run(self, run_id, cancellation).await })
    }
}

fn document_path(document_id: &DocumentId) -> String {
    format!(
        "/api/v1/documents/{}",
        percent_encoding::utf8_percent_encode(
            document_id.as_str(),
            percent_encoding::NON_ALPHANUMERIC
        )
    )
}

fn api_failure(response: KernelHttpResponse) -> McpKernelFailure {
    let Some(body) = response.body else {
        return McpKernelFailure::InvalidResponse;
    };
    let Ok(envelope) = serde_json::from_slice::<ApiErrorEnvelope>(&body) else {
        return McpKernelFailure::InvalidResponse;
    };
    let code = envelope.code();
    let Ok(expected_envelope) =
        safe_error_envelope(code, response.request_id, envelope.details().cloned())
    else {
        return McpKernelFailure::InvalidResponse;
    };
    let retry_after_matches = match envelope.details() {
        Some(ErrorDetails::RateLimit {
            retry_after_seconds,
        }) => response.retry_after_seconds == Some(retry_after_seconds.get()),
        _ => true,
    };
    if response.status != http_status_for_error_code(code)
        || envelope != expected_envelope
        || !retry_after_matches
    {
        return McpKernelFailure::InvalidResponse;
    }
    match envelope.details().and_then(ErrorDetails::current_revision) {
        Some(current_revision) => McpKernelFailure::ApiRevisionConflict {
            code,
            current_revision: current_revision.as_str().to_owned(),
        },
        None => McpKernelFailure::Api(code),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum McpKernelFailure {
    EndpointUnavailable,
    EndpointStale,
    RequestCancelled,
    TransportUnavailable,
    InvalidRequest,
    InvalidResponse,
    Api(ErrorCode),
    ApiRevisionConflict {
        code: ErrorCode,
        current_revision: String,
    },
}

impl McpKernelFailure {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::EndpointUnavailable => "kernel_endpoint_unavailable",
            Self::EndpointStale => "kernel_endpoint_stale",
            Self::RequestCancelled => "request_cancelled",
            Self::TransportUnavailable => "kernel_transport_unavailable",
            Self::InvalidRequest => "invalid_arguments",
            Self::InvalidResponse => "kernel_invalid_response",
            Self::Api(code) | Self::ApiRevisionConflict { code, .. } => error_code(*code),
        }
    }
}

impl fmt::Display for McpKernelFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Kernel request failed safely ({}).", self.code())
    }
}

impl std::error::Error for McpKernelFailure {}

const fn error_code(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidRequest => "invalid_request",
        ErrorCode::InvalidWorkspacePath => "invalid_workspace_path",
        ErrorCode::InvalidDocumentName => "invalid_document_name",
        ErrorCode::Unauthorized => "unauthorized",
        ErrorCode::InitializationRequired => "initialization_required",
        ErrorCode::AlreadyInitialized => "already_initialized",
        ErrorCode::InvalidCredentials => "invalid_credentials",
        ErrorCode::CsrfRejected => "csrf_rejected",
        ErrorCode::AuthenticationRateLimited => "authentication_rate_limited",
        ErrorCode::AuthenticationUnavailable => "authentication_unavailable",
        ErrorCode::HostNotAllowed => "host_not_allowed",
        ErrorCode::OriginNotAllowed => "origin_not_allowed",
        ErrorCode::KernelNotReady => "kernel_not_ready",
        ErrorCode::WorkspaceUnavailable => "workspace_unavailable",
        ErrorCode::WorkspaceLocked => "workspace_locked",
        ErrorCode::WorkspaceGenerationStale => "workspace_generation_stale",
        ErrorCode::DocumentNotFound => "document_not_found",
        ErrorCode::ResourceNotFound => "resource_not_found",
        ErrorCode::DocumentAlreadyExists => "document_already_exists",
        ErrorCode::DocumentTooLarge => "document_too_large",
        ErrorCode::ResourceTooLarge => "resource_too_large",
        ErrorCode::DocumentInvalidEncoding => "document_invalid_encoding",
        ErrorCode::RevisionConflict => "revision_conflict",
        ErrorCode::SettingsRevisionConflict => "settings_revision_conflict",
        ErrorCode::SyncConfigRevisionConflict => "sync_config_revision_conflict",
        ErrorCode::InvalidSettingsField => "invalid_settings_field",
        ErrorCode::InvalidAppConfigState => "invalid_app_config_state",
        ErrorCode::SettingsUnavailable => "settings_unavailable",
        ErrorCode::AppConfigUnavailable => "app_config_unavailable",
        ErrorCode::SyncConfigAbsent => "sync_config_absent",
        ErrorCode::SyncConfigInvalid => "sync_config_invalid",
        ErrorCode::SyncNotReady => "sync_not_ready",
        ErrorCode::SyncRunUnavailable => "sync_run_unavailable",
        ErrorCode::InternalError => "internal_error",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use qingyu_kernel::contract::{
        DeleteDocumentRequest, DeletionPolicy, DocumentKind, ErrorCode, ListDocumentsQuery,
        PatchSettingsRequest, RequestId, Revision, SettingsSnapshotDto, SyncStatusDto,
        TriggerSyncRunRequest, WireIdentityKey, WorkspaceGeneration, WorkspaceId,
        WorkspaceRelativePath,
    };
    use reqwest::{Client, Proxy};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::Notify,
    };
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    use crate::{
        kernel_host::kernel_endpoint_record::KernelEndpointRecord, mcp::config::McpConfigManager,
    };

    use super::{
        api_failure, secure_kernel_http_client, EndpointReadFailure, EndpointRecordSource,
        KernelEndpointAccess, KernelEndpointSource, KernelHttpFailure, KernelHttpFuture,
        KernelHttpMethod, KernelHttpRequest, KernelHttpResponse, KernelHttpTransport,
        McpKernelClient, McpKernelFailure, ReqwestKernelHttpTransport,
    };

    const REQUEST_ID: &str = "10000000-0000-4000-8000-000000000001";
    const INSTANCE_ID: &str = "20000000-0000-4000-8000-000000000002";

    #[derive(Clone)]
    struct MutableEndpointSource {
        state: Arc<Mutex<KernelEndpointAccess>>,
    }

    impl MutableEndpointSource {
        fn ready(generation: u64, port: u16, secret: &str) -> Self {
            Self {
                state: Arc::new(Mutex::new(KernelEndpointAccess::for_test(
                    generation,
                    port,
                    Uuid::parse_str(INSTANCE_ID).expect("instance UUID"),
                    secret,
                ))),
            }
        }

        fn replace(&self, generation: u64, port: u16, secret: &str) {
            *self.state.lock().expect("endpoint state") = KernelEndpointAccess::for_test(
                generation,
                port,
                Uuid::parse_str(INSTANCE_ID).expect("instance UUID"),
                secret,
            );
        }
    }

    impl KernelEndpointSource for MutableEndpointSource {
        fn read(&self) -> Result<Option<KernelEndpointAccess>, EndpointReadFailure> {
            self.state
                .lock()
                .map_err(|_| EndpointReadFailure)
                .map(|access| Some(access.clone()))
        }
    }

    #[derive(Clone)]
    struct RecordingTransport {
        requests: Arc<Mutex<Vec<KernelHttpRequest>>>,
        responses: Arc<Mutex<VecDeque<Result<KernelHttpResponse, KernelHttpFailure>>>>,
        on_send: Option<Arc<dyn Fn() + Send + Sync>>,
    }

    impl RecordingTransport {
        fn with_json_responses(values: impl IntoIterator<Item = (u16, serde_json::Value)>) -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(
                    values
                        .into_iter()
                        .map(|(status, value)| {
                            Ok(KernelHttpResponse::for_test(
                                status,
                                REQUEST_ID,
                                Some(serde_json::to_vec(&value).expect("response JSON")),
                            ))
                        })
                        .collect(),
                )),
                on_send: None,
            }
        }

        fn with_hook(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
            self.on_send = Some(Arc::new(hook));
            self
        }

        fn with_responses(responses: impl IntoIterator<Item = KernelHttpResponse>) -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(responses.into_iter().map(Ok).collect())),
                on_send: None,
            }
        }

        fn requests(&self) -> Vec<KernelHttpRequest> {
            self.requests.lock().expect("recorded requests").clone()
        }
    }

    impl KernelHttpTransport for RecordingTransport {
        fn send<'a>(
            &'a self,
            _endpoint: &'a KernelEndpointAccess,
            request: KernelHttpRequest,
        ) -> KernelHttpFuture<'a> {
            self.requests.lock().expect("record request").push(request);
            if let Some(hook) = &self.on_send {
                hook();
            }
            Box::pin(async move {
                self.responses
                    .lock()
                    .map_err(|_| KernelHttpFailure)?
                    .pop_front()
                    .unwrap_or(Err(KernelHttpFailure))
            })
        }
    }

    #[tokio::test]
    async fn routes_document_settings_and_sync_reads_to_kernel_contract_paths() {
        let source = MutableEndpointSource::ready(7, 41007, "route-secret");
        let transport = RecordingTransport::with_json_responses([
            (200, serde_json::json!({ "items": [], "nextCursor": null })),
            (
                200,
                serde_json::json!({ "revision": "settings-1", "values": [] }),
            ),
            (
                200,
                serde_json::json!({
                    "completionState": "idle",
                    "provider": "s3",
                    "configRevision": null,
                    "activeRunId": null,
                    "lastAttemptAt": null,
                    "lastSuccessfulSyncAt": null,
                    "lastTrigger": null,
                    "summary": null,
                    "error": null
                }),
            ),
        ]);
        let client = McpKernelClient::with_transport_for_test(source, transport.clone());
        let cancellation = CancellationToken::new();

        let document_query = serde_json::from_value::<ListDocumentsQuery>(serde_json::json!({}))
            .expect("empty document query");
        let documents = client
            .list_documents(&document_query, &cancellation)
            .await
            .expect("document list");
        let settings: SettingsSnapshotDto = client
            .get_settings(&cancellation)
            .await
            .expect("settings read");
        let sync: SyncStatusDto = client
            .get_sync_status(&cancellation)
            .await
            .expect("sync status");

        assert!(documents.items.is_empty());
        assert_eq!(settings.revision.as_str(), "settings-1");
        assert!(serde_json::to_value(sync).expect("sync JSON")["activeRunId"].is_null());
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method(), KernelHttpMethod::Get);
        assert_eq!(requests[0].path(), "/api/v1/documents");
        assert_eq!(requests[1].method(), KernelHttpMethod::Get);
        assert_eq!(requests[1].path(), "/api/v1/settings");
        assert_eq!(requests[2].method(), KernelHttpMethod::Get);
        assert_eq!(requests[2].path(), "/api/v1/sync/status");
    }

    #[tokio::test]
    async fn routes_workspace_and_exact_sync_run_reads_without_using_global_status() {
        let run_id = Uuid::parse_str("30000000-0000-4000-8000-000000000003").expect("run UUID");
        let transport = RecordingTransport::with_json_responses([
            (
                200,
                serde_json::json!({
                    "id": "40000000-0000-4000-8000-000000000004",
                    "generation": "workspace-13",
                    "displayName": "Notes",
                    "readiness": "ready",
                    "revision": "workspace-revision-13"
                }),
            ),
            (
                200,
                serde_json::json!({
                    "runId": run_id,
                    "provider": "s3",
                    "configRevision": "sync-13",
                    "completionState": "succeeded",
                    "acceptedAt": "2026-07-31T00:00:00Z",
                    "finishedAt": "2026-07-31T00:00:01Z",
                    "summary": {
                        "bytesDownloaded": 0,
                        "bytesUploaded": 0,
                        "conflictFiles": 0,
                        "downloadedFiles": 0,
                        "scannedFiles": 0,
                        "skippedFiles": 0,
                        "uploadedFiles": 0
                    },
                    "error": null
                }),
            ),
        ]);
        let client = McpKernelClient::with_transport_for_test(
            MutableEndpointSource::ready(13, 41013, "route-secret"),
            transport.clone(),
        );
        let cancellation = CancellationToken::new();

        let workspace = client
            .get_workspace(&cancellation)
            .await
            .expect("workspace read");
        let status = client
            .get_sync_run(qingyu_kernel::contract::RunId::new(run_id), &cancellation)
            .await
            .expect("exact sync run read");

        assert_eq!(workspace.generation.as_str(), "workspace-13");
        assert_eq!(status.run_id.as_uuid(), &run_id);
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path(), "/api/v1/workspace");
        assert_eq!(
            requests[1].path(),
            "/api/v1/sync/runs/30000000-0000-4000-8000-000000000003"
        );
        assert_ne!(requests[1].path(), "/api/v1/sync/status");
    }

    #[tokio::test]
    async fn routes_document_settings_and_sync_mutations_with_typed_bodies() {
        let transport = RecordingTransport::with_responses([
            KernelHttpResponse::for_test(204, REQUEST_ID, None),
            KernelHttpResponse::for_test(
                200,
                REQUEST_ID,
                Some(
                    serde_json::to_vec(&serde_json::json!({
                        "revision": "settings-3",
                        "values": []
                    }))
                    .expect("settings response"),
                ),
            ),
            KernelHttpResponse::for_test(
                202,
                REQUEST_ID,
                Some(
                    serde_json::to_vec(&serde_json::json!({
                        "runId": "30000000-0000-4000-8000-000000000003",
                        "acceptedAt": "2026-07-31T00:00:00Z",
                        "configRevision": "sync-3"
                    }))
                    .expect("sync response"),
                ),
            ),
        ]);
        let client = McpKernelClient::with_transport_for_test(
            MutableEndpointSource::ready(13, 41013, "mutation-secret"),
            transport.clone(),
        );
        let workspace_id = WorkspaceId::new(
            Uuid::parse_str("40000000-0000-4000-8000-000000000004").expect("workspace UUID"),
        );
        let workspace_generation =
            WorkspaceGeneration::parse("workspace-13").expect("workspace generation");
        let path = WorkspaceRelativePath::parse("note.md").expect("document path");
        let document_id = WireIdentityKey::generate()
            .expect("identity key")
            .issue_document_id(
                workspace_id,
                &workspace_generation,
                DocumentKind::File,
                &path,
            )
            .expect("document ID");
        let delete = DeleteDocumentRequest {
            workspace_generation,
            expected_revision: Revision::parse("document-13").expect("document revision"),
            deletion_policy: DeletionPolicy::Recoverable,
        };
        let settings = serde_json::from_value::<PatchSettingsRequest>(serde_json::json!({
            "expectedRevision": "settings-2",
            "values": [{
                "key": "language",
                "value": { "type": "string", "value": "zh-CN" }
            }]
        }))
        .expect("settings patch");
        let sync = TriggerSyncRunRequest {
            expected_config_revision: Revision::parse("sync-3").expect("sync revision"),
        };
        let cancellation = CancellationToken::new();

        client
            .delete_document(&document_id, &delete, &cancellation)
            .await
            .expect("document delete");
        let patched = client
            .patch_settings(&settings, &cancellation)
            .await
            .expect("settings patch");
        let triggered = client
            .trigger_sync_run(&sync, &cancellation)
            .await
            .expect("sync trigger");

        assert_eq!(patched.revision.as_str(), "settings-3");
        assert_eq!(triggered.config_revision.as_str(), "sync-3");
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method(), KernelHttpMethod::Post);
        assert!(requests[0].path().ends_with("/delete"));
        assert_eq!(requests[0].expected_status, 204);
        assert_eq!(requests[1].method(), KernelHttpMethod::Patch);
        assert_eq!(requests[1].path(), "/api/v1/settings");
        assert_eq!(requests[2].method(), KernelHttpMethod::Post);
        assert_eq!(requests[2].path(), "/api/v1/sync/runs");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                requests[2].body.as_deref().expect("sync request body")
            )
            .expect("sync request JSON")["expectedConfigRevision"],
            "sync-3"
        );
    }

    #[tokio::test]
    async fn reqwest_transport_authenticates_from_the_credential_lease() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback test server");
        let port = listener.local_addr().expect("test address").port();
        let proxy_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind proxy trap");
        let proxy_port = proxy_listener.local_addr().expect("proxy address").port();
        let credential = "lease-only-bearer";
        let authenticated = Arc::new(AtomicBool::new(false));
        let authenticated_server = Arc::clone(&authenticated);
        let proxy_contacted = Arc::new(AtomicBool::new(false));
        let proxy_contacted_server = Arc::clone(&proxy_contacted);
        let proxy_server = tokio::spawn(async move {
            let (mut stream, _) = proxy_listener.accept().await.expect("accept proxy request");
            proxy_contacted_server.store(true, Ordering::SeqCst);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 502 Bad Gateway\r\nx-request-id: {REQUEST_ID}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .expect("write proxy rejection");
        });
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut bytes = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).await.expect("read request");
                assert!(read > 0, "request headers ended early");
                bytes.extend_from_slice(&chunk[..read]);
            }
            let request = String::from_utf8(bytes).expect("HTTP request is UTF-8");
            authenticated_server.store(
                request.lines().any(|line| {
                    line.eq_ignore_ascii_case("authorization: Bearer lease-only-bearer")
                }),
                Ordering::SeqCst,
            );
            let body = br#"{"revision":"settings-2","values":[]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nx-request-id: {REQUEST_ID}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write headers");
            stream.write_all(body).await.expect("write body");
        });
        let source = MutableEndpointSource::ready(8, port, credential);
        let http =
            secure_kernel_http_client(Client::builder().proxy(
                Proxy::all(format!("http://127.0.0.1:{proxy_port}")).expect("test proxy URL"),
            ))
            .expect("HTTP client");
        let client = McpKernelClient::with_transport_for_test(
            source,
            ReqwestKernelHttpTransport { client: http },
        );

        let settings = client
            .get_settings(&CancellationToken::new())
            .await
            .expect("authenticated settings read");

        server.await.expect("test server");
        proxy_server.abort();
        let _ = proxy_server.await;
        assert_eq!(settings.revision.as_str(), "settings-2");
        assert!(authenticated.load(Ordering::SeqCst));
        assert!(!proxy_contacted.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn missing_and_closed_endpoints_fail_before_transport() {
        let (writer, reader) = KernelEndpointRecord::create();
        let transport = RecordingTransport::with_json_responses([]);
        let client = McpKernelClient::with_transport_for_test(
            EndpointRecordSource(reader),
            transport.clone(),
        );
        let patch = serde_json::from_value::<PatchSettingsRequest>(serde_json::json!({
            "expectedRevision": "settings-closed",
            "values": []
        }))
        .expect("settings patch");

        let missing = client
            .patch_settings(&patch, &CancellationToken::new())
            .await
            .expect_err("missing endpoint must fail closed");
        drop(writer);
        let closed = client
            .patch_settings(&patch, &CancellationToken::new())
            .await
            .expect_err("closed endpoint must fail closed");

        assert_eq!(missing, McpKernelFailure::EndpointUnavailable);
        assert_eq!(closed, McpKernelFailure::EndpointUnavailable);
        assert!(transport.requests().is_empty());
    }

    #[tokio::test]
    async fn endpoint_generation_change_invalidates_the_completed_response() {
        let source = MutableEndpointSource::ready(9, 41009, "old-secret");
        let replacement = source.clone();
        let transport = RecordingTransport::with_json_responses([(
            200,
            serde_json::json!({ "revision": "stale-settings", "values": [] }),
        )])
        .with_hook(move || replacement.replace(10, 41010, "new-secret"));
        let client = McpKernelClient::with_transport_for_test(source, transport.clone());

        let error = client
            .get_settings(&CancellationToken::new())
            .await
            .expect_err("stale endpoint response must be rejected");

        assert_eq!(error, McpKernelFailure::EndpointStale);
        assert_eq!(transport.requests().len(), 1);
    }

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[derive(Clone)]
    struct PendingTransport {
        started: Arc<Notify>,
        dropped: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
    }

    impl KernelHttpTransport for PendingTransport {
        fn send<'a>(
            &'a self,
            _endpoint: &'a KernelEndpointAccess,
            _request: KernelHttpRequest,
        ) -> KernelHttpFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                let _drop_flag = DropFlag(Arc::clone(&self.dropped));
                self.started.notify_one();
                std::future::pending::<Result<KernelHttpResponse, KernelHttpFailure>>().await
            })
        }
    }

    #[tokio::test]
    async fn request_cancellation_drops_the_in_flight_transport_future() {
        let transport = PendingTransport {
            started: Arc::new(Notify::new()),
            dropped: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let client = McpKernelClient::with_transport_for_test(
            MutableEndpointSource::ready(11, 41011, "cancel-secret"),
            transport.clone(),
        );
        let cancellation = CancellationToken::new();
        let request_cancellation = cancellation.clone();
        let request = tokio::spawn(async move { client.get_settings(&request_cancellation).await });
        transport.started.notified().await;

        cancellation.cancel();
        let error = request
            .await
            .expect("request task")
            .expect_err("cancelled request must fail closed");

        assert_eq!(error, McpKernelFailure::RequestCancelled);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert!(transport.dropped.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn uncancelled_request_times_out_without_exposing_endpoint_or_credential() {
        let secret = "timeout-secret-must-stay-redacted";
        let port = 41_016;
        let transport = PendingTransport {
            started: Arc::new(Notify::new()),
            dropped: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let timeout_config = Arc::new(
            McpConfigManager::memory_for_test().expect("in-memory MCP timeout configuration"),
        );
        let client = McpKernelClient::with_transport_for_test(
            MutableEndpointSource::ready(16, port, secret),
            transport.clone(),
        )
        .with_configured_request_timeout(Arc::clone(&timeout_config));
        let current = timeout_config.snapshot().expect("current timeout config");
        let mut updated = current.config;
        updated.tool_timeout_secs = 5;
        timeout_config
            .update(updated, &current.revision)
            .expect("update live timeout config after client construction");
        let request =
            tokio::spawn(async move { client.get_settings(&CancellationToken::new()).await });
        transport.started.notified().await;

        tokio::time::advance(Duration::from_secs(6)).await;
        let error = tokio::time::timeout(Duration::from_secs(1), request)
            .await
            .expect("adapter must enforce its own request timeout")
            .expect("request task")
            .expect_err("hanging transport must fail closed");
        let rendered = format!("{error:?} {error}");

        assert_eq!(error, McpKernelFailure::TransportUnavailable);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert!(transport.dropped.load(Ordering::SeqCst));
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains(&port.to_string()));
        assert!(!rendered.contains("http://"));
    }

    #[tokio::test]
    async fn untrusted_error_payload_and_endpoint_debug_are_redacted() {
        let secret = "never-surface-this-credential";
        let source = MutableEndpointSource::ready(12, 41012, secret);
        let endpoint = source
            .read()
            .expect("endpoint read")
            .expect("ready endpoint");
        let transport = RecordingTransport::with_json_responses([(
            500,
            serde_json::json!({
                "code": "internal_error",
                "message": secret,
                "requestId": REQUEST_ID
            }),
        )]);
        let client = McpKernelClient::with_transport_for_test(source, transport);

        let error = client
            .get_settings(&CancellationToken::new())
            .await
            .expect_err("invalid error envelope must be rejected");
        let rendered = format!("{endpoint:?} {error:?} {error}");

        assert_eq!(error, McpKernelFailure::InvalidResponse);
        assert!(!rendered.contains(secret));
    }

    #[test]
    fn api_failure_codes_are_stable_without_exposing_server_messages() {
        let failure = McpKernelFailure::Api(ErrorCode::Unauthorized);
        assert_eq!(failure.code(), "unauthorized");
        assert_eq!(
            format!("{failure}"),
            "Kernel request failed safely (unauthorized)."
        );
        let request_id = RequestId::new(Uuid::parse_str(REQUEST_ID).expect("request UUID"));
        assert_eq!(request_id.as_uuid().to_string(), REQUEST_ID);
        assert_eq!(
            McpKernelFailure::Api(ErrorCode::ResourceTooLarge).code(),
            "resource_too_large"
        );
    }

    #[test]
    fn api_revision_conflicts_preserve_only_the_safe_current_revision() {
        let body = serde_json::to_vec(&serde_json::json!({
            "code": "sync_config_revision_conflict",
            "message": "The sync configuration changed since it was loaded.",
            "requestId": REQUEST_ID,
            "details": {
                "type": "revision-conflict",
                "currentRevision": "sync-2"
            }
        }))
        .expect("revision conflict envelope");

        let failure = api_failure(KernelHttpResponse::for_test(409, REQUEST_ID, Some(body)));

        assert_eq!(
            failure,
            McpKernelFailure::ApiRevisionConflict {
                code: ErrorCode::SyncConfigRevisionConflict,
                current_revision: "sync-2".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn api_error_details_must_match_the_error_code() {
        let transport = RecordingTransport::with_json_responses([(
            500,
            serde_json::json!({
                "code": "internal_error",
                "message": "An unexpected error occurred.",
                "requestId": REQUEST_ID,
                "details": {
                    "type": "rate-limit",
                    "retryAfterSeconds": 31
                }
            }),
        )]);
        let client = McpKernelClient::with_transport_for_test(
            MutableEndpointSource::ready(14, 41014, "details-secret"),
            transport,
        );

        let error = client
            .get_settings(&CancellationToken::new())
            .await
            .expect_err("mismatched error details must fail closed");

        assert_eq!(error, McpKernelFailure::InvalidResponse);
    }

    #[tokio::test]
    async fn rate_limited_error_requires_a_matching_retry_after_header() {
        let body = serde_json::to_vec(&serde_json::json!({
            "code": "authentication_rate_limited",
            "message": "Authentication is temporarily limited.",
            "requestId": REQUEST_ID,
            "details": {
                "type": "rate-limit",
                "retryAfterSeconds": 31
            }
        }))
        .expect("rate-limit envelope");
        let transport = RecordingTransport::with_responses([
            KernelHttpResponse::for_test(429, REQUEST_ID, Some(body.clone())),
            KernelHttpResponse::for_test_with_retry_after(429, REQUEST_ID, 31, Some(body)),
        ]);
        let client = McpKernelClient::with_transport_for_test(
            MutableEndpointSource::ready(15, 41015, "retry-secret"),
            transport,
        );

        let error = client
            .get_settings(&CancellationToken::new())
            .await
            .expect_err("rate limit without Retry-After must fail closed");

        assert_eq!(error, McpKernelFailure::InvalidResponse);
        let matched = client
            .get_settings(&CancellationToken::new())
            .await
            .expect_err("matching rate-limit envelope remains an API failure");
        assert_eq!(
            matched,
            McpKernelFailure::Api(ErrorCode::AuthenticationRateLimited)
        );
    }
}
