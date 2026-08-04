use std::{fmt, future::Future, pin::Pin};


use tokio_util::sync::CancellationToken;

use crate::contract::{
    CreateDocumentRequest, CreatedDocumentDto, DeleteDocumentRequest, DocumentContentDto,
    DocumentEntryDto, DocumentId, DocumentPageDto, ErrorCode, ListDocumentsQuery,
    MoveDocumentRequest, PatchSettingsRequest, PatchSyncConfigRequest, RunId,
    SearchPageDto, SearchWorkspaceQuery, SettingsSnapshotDto, SyncConfigViewDto,
    SyncConnectionTestDto, SyncRunAcceptedDto, SyncRunStatusDto, SyncStatusDto,
    TestSyncConnectionRequest, TriggerSyncRunRequest, UpdateDocumentRequest, WorkspaceDto,
};

pub type McpKernelFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, McpKernelFailure>> + Send + 'a>>;

pub trait McpKernelPort: Send + Sync + 'static {
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
        run_id: RunId,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncRunStatusDto>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum McpKernelFailure {
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
    pub const fn code(&self) -> &'static str {
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
        ErrorCode::DocumentNotFound => "document_not_found",
        ErrorCode::DocumentAlreadyExists => "document_already_exists",
        ErrorCode::SettingsUnavailable => "settings_unavailable",
        ErrorCode::InvalidSettingsField => "invalid_settings_field",
        ErrorCode::SyncConfigAbsent => "sync_config_absent",
        ErrorCode::SyncConfigInvalid => "sync_config_invalid",
        ErrorCode::SyncNotReady => "sync_not_ready",
        ErrorCode::SyncRunUnavailable => "sync_run_unavailable",
        ErrorCode::RevisionConflict | ErrorCode::SyncConfigRevisionConflict => "revision_conflict",
        _ => "kernel_error",
    }
}


/// Direct in-process port backed by Kernel runtime services.
/// Used by headless/Docker server composition.
#[derive(Clone)]
pub struct DirectKernelMcpPort {
    composition: std::sync::Arc<crate::server::runtime_composition::ServerRuntimeComposition>,
}

impl DirectKernelMcpPort {
    pub fn new(
        composition: std::sync::Arc<crate::server::runtime_composition::ServerRuntimeComposition>,
    ) -> Self {
        Self { composition }
    }
}

impl McpKernelPort for DirectKernelMcpPort {
    fn get_workspace<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, WorkspaceDto> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .workspace_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .get_workspace()
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn list_documents<'a>(
        &'a self,
        query: &'a ListDocumentsQuery,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentPageDto> {
        let query = query.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .documents_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .list_documents(query)
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn create_document<'a>(
        &'a self,
        request: &'a CreateDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, CreatedDocumentDto> {
        let request = request.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .documents_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .create_document(request)
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn get_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentContentDto> {
        let document_id = document_id.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .documents_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .get_document(document_id)
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn update_document<'a>(
        &'a self,
        document_id: &'a DocumentId,
        request: &'a UpdateDocumentRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentContentDto> {
        let document_id = document_id.clone();
        let request = request.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .documents_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .update_document(document_id, request)
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn get_settings<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SettingsSnapshotDto> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .settings_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .get_settings()
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn patch_settings<'a>(
        &'a self,
        request: &'a PatchSettingsRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SettingsSnapshotDto> {
        let request = request.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .settings_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .patch_settings(request)
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn get_sync_config<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncConfigViewDto> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .sync_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .get_sync_config()
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn patch_sync_config<'a>(
        &'a self,
        request: &'a PatchSyncConfigRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncConfigViewDto> {
        let request = request.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .sync_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .patch_sync_config(request)
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn test_sync_connection<'a>(
        &'a self,
        request: &'a TestSyncConnectionRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncConnectionTestDto> {
        let request = request.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .sync_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .test_sync_connection(request)
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn get_sync_status<'a>(
        &'a self,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncStatusDto> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .sync_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .get_sync_status()
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn trigger_sync_run<'a>(
        &'a self,
        request: &'a TriggerSyncRunRequest,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncRunAcceptedDto> {
        let request = request.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .sync_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .trigger_sync_run(request)
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn get_sync_run<'a>(
        &'a self,
        run_id: RunId,
        cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SyncRunStatusDto> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(McpKernelFailure::RequestCancelled);
            }
            let service = self
                .composition
                .runtime()
                .sync_api_service()
                .ok_or(McpKernelFailure::EndpointUnavailable)?;
            service
                .get_sync_run(run_id)
                .await
                .map_err(|error| McpKernelFailure::Api(error.code()))
        })
    }

    fn search_documents<'a>(
        &'a self,
        _query: &'a SearchWorkspaceQuery,
        _cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, SearchPageDto> {
        Box::pin(async {
            Err(McpKernelFailure::EndpointUnavailable)
        })
    }

    fn move_document<'a>(
        &'a self,
        _document_id: &'a DocumentId,
        _request: &'a MoveDocumentRequest,
        _cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, DocumentEntryDto> {
        Box::pin(async {
            Err(McpKernelFailure::EndpointUnavailable)
        })
    }

    fn delete_document<'a>(
        &'a self,
        _document_id: &'a DocumentId,
        _request: &'a DeleteDocumentRequest,
        _cancellation: &'a CancellationToken,
    ) -> McpKernelFuture<'a, ()> {
        Box::pin(async {
            Err(McpKernelFailure::EndpointUnavailable)
        })
    }
}
