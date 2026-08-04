use std::time::Duration;

use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::contract::{
    CreateDocumentRequest, CreatedDocumentDto, DeleteDocumentRequest, DeletionPolicy,
    DocumentContents, DocumentEntryDto, DocumentId, DocumentKind, FileDocumentName,
    ListDocumentsQuery, MoveDocumentRequest, PageCursor, PageLimit, Revision, SearchQuery,
    SearchWorkspaceQuery, TriggerSyncRunRequest, UpdateDocumentRequest, WorkspaceGeneration,
    WorkspaceRelativePath,
};

use crate::mcp::config::SyncAfterWritePolicy;

use super::{failure_from_code, failure_from_kernel, McpServices, ToolResult};

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct DocumentListInput {
    #[schemars(with = "String")]
    pub(super) workspace_id: uuid::Uuid,
    pub(super) parent_folder_id: Option<String>,
    pub(super) cursor: Option<String>,
    pub(super) limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct DocumentSearchInput {
    #[schemars(with = "String")]
    pub(super) workspace_id: uuid::Uuid,
    pub(super) query: String,
    pub(super) cursor: Option<String>,
    pub(super) limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct DocumentReadInput {
    pub(super) document_id: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct DocumentCreateInput {
    #[schemars(with = "String")]
    pub(super) workspace_id: uuid::Uuid,
    pub(super) parent_folder_id: String,
    pub(super) name: String,
    pub(super) contents: String,
    pub(super) dry_run: Option<bool>,
    pub(super) preview_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct DocumentUpdateInput {
    pub(super) document_id: String,
    pub(super) contents: String,
    pub(super) expected_revision: String,
    pub(super) dry_run: Option<bool>,
    pub(super) preview_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct DocumentMoveInput {
    pub(super) document_id: String,
    pub(super) target_folder_id: String,
    pub(super) new_name: String,
    pub(super) expected_revision: String,
    pub(super) dry_run: Option<bool>,
    pub(super) preview_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct DocumentDeleteInput {
    pub(super) document_id: String,
    pub(super) expected_revision: String,
    pub(super) dry_run: Option<bool>,
    pub(super) preview_token: Option<String>,
}

#[derive(Clone, Copy)]
struct LocalAuthority {
    workspace_id: uuid::Uuid,
    generation: u64,
}

pub(super) async fn list(
    services: &McpServices,
    input: DocumentListInput,
    cancellation: &CancellationToken,
) -> ToolResult {
    let authority = resolve_workspace(services, input.workspace_id)?;
    let parent = input
        .parent_folder_id
        .as_deref()
        .map(|folder_id| verify_folder(services, folder_id, authority))
        .transpose()?
        .unwrap_or_else(WorkspaceRelativePath::default);
    let query = ListDocumentsQuery {
        cursor: input.cursor.map(parse_cursor).transpose()?,
        limit: input.limit.map(parse_limit).transpose()?,
        parent,
    };
    let page = services
        .kernel
        .list_documents(&query, cancellation)
        .await
        .map_err(failure_from_kernel)?;
    revalidate_workspace(services, authority)?;
    let entries = page
        .items
        .iter()
        .map(|entry| entry_value(services, authority, entry))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::json!({
        "entries": entries,
        "nextCursor": page.next_cursor.into_option().map(|cursor| cursor.as_str().to_owned()),
    }))
}

pub(super) async fn search(
    services: &McpServices,
    input: DocumentSearchInput,
    cancellation: &CancellationToken,
) -> ToolResult {
    let authority = resolve_workspace(services, input.workspace_id)?;
    let query_text = input.query.clone();
    let query = SearchWorkspaceQuery {
        cursor: input.cursor.map(parse_cursor).transpose()?,
        limit: input.limit.map(parse_limit).transpose()?,
        query: SearchQuery::parse(input.query)
            .map_err(|_| failure_from_code("invalid_query", None))?,
    };
    let page = services
        .kernel
        .search_documents(&query, cancellation)
        .await
        .map_err(failure_from_kernel)?;
    revalidate_workspace(services, authority)?;
    let results = page
        .items
        .iter()
        .map(|hit| {
            let document_id = issue_document(services, authority, &hit.document)?;
            let column = hit.column.get();
            Ok(serde_json::json!({
                "documentId": document_id,
                "relativePath": hit.document.path.as_str(),
                "lineNumber": hit.line.get(),
                "columnNumber": column,
                "snippet": hit.preview,
                "matchedFrom": column.saturating_sub(1),
                "matchedTo": column.saturating_sub(1).saturating_add(query_text.chars().count() as u64),
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::json!({
        "results": results,
        "nextCursor": page.next_cursor.into_option().map(|cursor| cursor.as_str().to_owned()),
        "searchedDocumentCount": results.len(),
        "unreadableDocumentCount": 0,
        "truncated": false,
    }))
}

pub(super) async fn read(
    services: &McpServices,
    input: DocumentReadInput,
    document_limit_bytes: u64,
    cancellation: &CancellationToken,
) -> ToolResult {
    let (authority, document_id) = verify_document(services, &input.document_id)?;
    let document = services
        .kernel
        .get_document(&document_id, cancellation)
        .await
        .map_err(failure_from_kernel)?;
    revalidate_workspace(services, authority)?;
    if document.size_bytes.get() > document_limit_bytes {
        return Err(failure_from_code("document_too_large", None));
    }
    Ok(serde_json::json!({
        "documentId": input.document_id,
        "relativePath": document.path.as_str(),
        "contents": document.contents.as_str(),
        "sizeBytes": document.size_bytes.get(),
        "revision": document.revision.as_str(),
    }))
}

pub(super) async fn create(
    services: &McpServices,
    input: &DocumentCreateInput,
    sync_after_write: SyncAfterWritePolicy,
    cancellation: &CancellationToken,
) -> ToolResult {
    let authority = resolve_workspace(services, input.workspace_id)?;
    let parent = verify_folder(services, &input.parent_folder_id, authority)?;
    let workspace_generation = kernel_workspace_generation(services, cancellation).await?;
    let request = CreateDocumentRequest::File {
        workspace_generation,
        parent,
        name: FileDocumentName::parse(input.name.clone())
            .map_err(|_| failure_from_code("invalid_document_name", None))?,
        contents: DocumentContents::parse(input.contents.clone())
            .map_err(|_| failure_from_code("document_too_large", None))?,
    };
    let created = services
        .kernel
        .create_document(&request, cancellation)
        .await
        .map_err(failure_from_kernel)?;
    revalidate_workspace(services, authority)?;
    let CreatedDocumentDto::File {
        id, path, revision, ..
    } = created
    else {
        return Err(failure_from_code("operation_failed", None));
    };
    mutation_value(
        services,
        authority,
        &id,
        &path,
        &revision,
        sync_after_write,
        cancellation,
    )
    .await
}

pub(super) async fn update(
    services: &McpServices,
    input: &DocumentUpdateInput,
    sync_after_write: SyncAfterWritePolicy,
    cancellation: &CancellationToken,
) -> ToolResult {
    let (authority, document_id) = verify_document(services, &input.document_id)?;
    let request = UpdateDocumentRequest {
        workspace_generation: kernel_workspace_generation(services, cancellation).await?,
        expected_revision: parse_revision(&input.expected_revision)?,
        contents: DocumentContents::parse(input.contents.clone())
            .map_err(|_| failure_from_code("document_too_large", None))?,
    };
    let updated = services
        .kernel
        .update_document(&document_id, &request, cancellation)
        .await
        .map_err(failure_from_kernel)?;
    revalidate_workspace(services, authority)?;
    mutation_value(
        services,
        authority,
        &updated.id,
        &updated.path,
        &updated.revision,
        sync_after_write,
        cancellation,
    )
    .await
}

pub(super) async fn move_document(
    services: &McpServices,
    input: &DocumentMoveInput,
    sync_after_write: SyncAfterWritePolicy,
    cancellation: &CancellationToken,
) -> ToolResult {
    let (authority, document_id) = verify_document(services, &input.document_id)?;
    let target_parent = verify_folder(services, &input.target_folder_id, authority)?;
    let request = MoveDocumentRequest {
        workspace_generation: kernel_workspace_generation(services, cancellation).await?,
        expected_revision: parse_revision(&input.expected_revision)?,
        target_parent,
        name: crate::contract::DocumentName::parse(input.new_name.clone())
            .map_err(|_| failure_from_code("invalid_document_name", None))?,
    };
    let moved = services
        .kernel
        .move_document(&document_id, &request, cancellation)
        .await
        .map_err(failure_from_kernel)?;
    revalidate_workspace(services, authority)?;
    mutation_value(
        services,
        authority,
        &moved.id,
        &moved.path,
        &moved.revision,
        sync_after_write,
        cancellation,
    )
    .await
}

pub(super) async fn delete(
    services: &McpServices,
    input: &DocumentDeleteInput,
    deletion: crate::mcp::config::DeletionPolicy,
    sync_after_write: SyncAfterWritePolicy,
    cancellation: &CancellationToken,
) -> ToolResult {
    let (authority, document_id) = verify_document(services, &input.document_id)?;
    let request = DeleteDocumentRequest {
        workspace_generation: kernel_workspace_generation(services, cancellation).await?,
        expected_revision: parse_revision(&input.expected_revision)?,
        deletion_policy: match deletion {
            crate::mcp::config::DeletionPolicy::Permanent => DeletionPolicy::Permanent,
            crate::mcp::config::DeletionPolicy::Recoverable => DeletionPolicy::Recoverable,
        },
    };
    services
        .kernel
        .delete_document(&document_id, &request, cancellation)
        .await
        .map_err(failure_from_kernel)?;
    revalidate_workspace(services, authority)?;
    let sync = request_sync_after_write(services, sync_after_write, cancellation).await;
    let mut value = serde_json::json!({
        "documentId": input.document_id,
        "deleted": true,
        "syncRequest": sync.label(),
        "syncRequestState": sync.state_label(),
    });
    if let Some(run_id) = sync.run_id {
        value["runId"] = serde_json::json!(run_id);
    }
    Ok(value)
}

fn resolve_workspace(
    services: &McpServices,
    workspace_id: uuid::Uuid,
) -> Result<LocalAuthority, crate::mcp::error::McpToolFailure> {
    services
        .workspaces
        .with_authority(|| {
            services
                .workspaces
                .resolve(workspace_id)
                .map(|workspace| LocalAuthority {
                    workspace_id,
                    generation: workspace.workspace_generation,
                })
                .map_err(|error| failure_from_code(error.code, None))
        })
        .map_err(|error| failure_from_code(error.code, None))?
}

fn revalidate_workspace(
    services: &McpServices,
    authority: LocalAuthority,
) -> Result<(), crate::mcp::error::McpToolFailure> {
    services
        .workspaces
        .with_authority(|| {
            services
                .workspaces
                .resolve_at_generation(authority.workspace_id, authority.generation)
                .map(|_| ())
                .map_err(|error| failure_from_code(error.code, None))
        })
        .map_err(|error| failure_from_code(error.code, None))?
}

fn verify_folder(
    services: &McpServices,
    folder_id: &str,
    authority: LocalAuthority,
) -> Result<WorkspaceRelativePath, crate::mcp::error::McpToolFailure> {
    services
        .workspaces
        .with_authority(|| {
            services
                .handles
                .verify_folder(folder_id, &services.workspaces)
                .map_err(|error| failure_from_code(error.code, None))
        })
        .map_err(|error| failure_from_code(error.code, None))??
        .then_relative(authority)
}

trait VerifiedFolderAuthority {
    fn then_relative(
        self,
        authority: LocalAuthority,
    ) -> Result<WorkspaceRelativePath, crate::mcp::error::McpToolFailure>;
}

impl VerifiedFolderAuthority for crate::mcp::handles::VerifiedFolderHandle {
    fn then_relative(
        self,
        authority: LocalAuthority,
    ) -> Result<WorkspaceRelativePath, crate::mcp::error::McpToolFailure> {
        if self.workspace_id() != authority.workspace_id
            || self.workspace().workspace_generation != authority.generation
        {
            return Err(failure_from_code("mcp-handle-stale", None));
        }
        WorkspaceRelativePath::parse(self.relative_path().to_string_lossy().replace('\\', "/"))
            .map_err(|_| failure_from_code("invalid_handle", None))
    }
}

fn verify_document(
    services: &McpServices,
    document_id: &str,
) -> Result<(LocalAuthority, DocumentId), crate::mcp::error::McpToolFailure> {
    let verified = services
        .workspaces
        .with_authority(|| {
            services
                .handles
                .verify_document(document_id, &services.workspaces)
                .map_err(|error| failure_from_code(error.code, None))
        })
        .map_err(|error| failure_from_code(error.code, None))??;
    let authority = LocalAuthority {
        workspace_id: verified.workspace_id(),
        generation: verified.workspace().workspace_generation,
    };
    let kernel_document_id = verified
        .kernel_document_id()
        .cloned()
        .ok_or_else(|| failure_from_code("mcp-handle-stale", None))?;
    Ok((authority, kernel_document_id))
}

async fn kernel_workspace_generation(
    services: &McpServices,
    cancellation: &CancellationToken,
) -> Result<WorkspaceGeneration, crate::mcp::error::McpToolFailure> {
    services
        .kernel
        .get_workspace(cancellation)
        .await
        .map(|workspace| workspace.generation)
        .map_err(failure_from_kernel)
}

fn entry_value(
    services: &McpServices,
    authority: LocalAuthority,
    entry: &DocumentEntryDto,
) -> Result<serde_json::Value, crate::mcp::error::McpToolFailure> {
    let id = match entry.kind {
        DocumentKind::File => issue_document(services, authority, entry)?,
        DocumentKind::Directory => services
            .handles
            .issue_folder(
                authority.workspace_id,
                authority.generation,
                entry.path.as_str(),
            )
            .map_err(|error| failure_from_code(error.code, None))?,
    };
    Ok(serde_json::json!({
        "id": id,
        "name": entry.name.as_str(),
        "relativePath": entry.path.as_str(),
        "kind": if entry.kind == DocumentKind::File { "document" } else { "folder" },
        "sizeBytes": entry.size_bytes.get(),
    }))
}

fn issue_document(
    services: &McpServices,
    authority: LocalAuthority,
    entry: &DocumentEntryDto,
) -> Result<String, crate::mcp::error::McpToolFailure> {
    services
        .handles
        .issue_kernel_document(
            authority.workspace_id,
            authority.generation,
            entry.path.as_str(),
            &entry.id,
        )
        .map_err(|error| failure_from_code(error.code, None))
}

async fn mutation_value(
    services: &McpServices,
    authority: LocalAuthority,
    id: &DocumentId,
    path: &WorkspaceRelativePath,
    revision: &Revision,
    sync_after_write: SyncAfterWritePolicy,
    cancellation: &CancellationToken,
) -> ToolResult {
    let document_id = services
        .handles
        .issue_kernel_document(
            authority.workspace_id,
            authority.generation,
            path.as_str(),
            id,
        )
        .map_err(|error| failure_from_code(error.code, None))?;
    let sync = request_sync_after_write(services, sync_after_write, cancellation).await;
    let mut value = serde_json::json!({
        "documentId": document_id,
        "relativePath": path.as_str(),
        "revision": revision.as_str(),
        "syncRequest": sync.label(),
        "syncRequestState": sync.state_label(),
    });
    if let Some(run_id) = sync.run_id {
        value["runId"] = serde_json::json!(run_id);
    }
    Ok(value)
}

struct SyncAfterWriteResult {
    requested: bool,
    run_id: Option<crate::contract::RunId>,
    state: SyncAfterWriteState,
}

enum SyncAfterWriteState {
    Accepted,
    Failed,
    NotRequested,
}

impl SyncAfterWriteResult {
    fn label(&self) -> &'static str {
        if self.requested {
            "requested"
        } else {
            "not_requested"
        }
    }

    fn state_label(&self) -> &'static str {
        match self.state {
            SyncAfterWriteState::Accepted => "accepted",
            SyncAfterWriteState::Failed => "failed",
            SyncAfterWriteState::NotRequested => "not_requested",
        }
    }
}

async fn request_sync_after_write(
    services: &McpServices,
    policy: SyncAfterWritePolicy,
    _request_cancellation: &CancellationToken,
) -> SyncAfterWriteResult {
    if policy == SyncAfterWritePolicy::Never {
        return SyncAfterWriteResult {
            requested: false,
            run_id: None,
            state: SyncAfterWriteState::NotRequested,
        };
    }
    let post_commit_cancellation = CancellationToken::new();
    let attempt = tokio::time::timeout(Duration::from_secs(5), async {
        let config = services
            .kernel
            .get_sync_config(&post_commit_cancellation)
            .await
            .map_err(|_| policy == SyncAfterWritePolicy::Always)?;
        if policy == SyncAfterWritePolicy::FollowWorkspace && !config.enabled {
            return Ok(None);
        }
        services
            .kernel
            .trigger_sync_run(
                &TriggerSyncRunRequest {
                    expected_config_revision: config.revision,
                },
                &post_commit_cancellation,
            )
            .await
            .map(|accepted| Some(accepted.run_id))
            .map_err(|_| true)
    })
    .await;
    match attempt {
        Ok(Ok(Some(run_id))) => SyncAfterWriteResult {
            requested: true,
            run_id: Some(run_id),
            state: SyncAfterWriteState::Accepted,
        },
        Ok(Ok(None)) => SyncAfterWriteResult {
            requested: false,
            run_id: None,
            state: SyncAfterWriteState::NotRequested,
        },
        Ok(Err(requested)) => SyncAfterWriteResult {
            requested,
            run_id: None,
            state: SyncAfterWriteState::Failed,
        },
        Err(_) => SyncAfterWriteResult {
            requested: policy == SyncAfterWritePolicy::Always,
            run_id: None,
            state: SyncAfterWriteState::Failed,
        },
    }
}

fn parse_revision(value: &str) -> Result<Revision, crate::mcp::error::McpToolFailure> {
    Revision::parse(value.to_owned()).map_err(|_| failure_from_code("invalid_arguments", None))
}

fn parse_cursor(value: String) -> Result<PageCursor, crate::mcp::error::McpToolFailure> {
    serde_json::from_value(serde_json::Value::String(value))
        .map_err(|_| failure_from_code("invalid_cursor", None))
}

fn parse_limit(value: usize) -> Result<PageLimit, crate::mcp::error::McpToolFailure> {
    let value = u16::try_from(value).map_err(|_| failure_from_code("invalid_arguments", None))?;
    PageLimit::new(value).map_err(|_| failure_from_code("invalid_arguments", None))
}
