use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::markdown_files::{
    CreateDocument, DeleteDocument, DocumentScope, MoveDocument, MutationOptions, UpdateDocument,
};

use super::{failure_from_code, McpServices, ToolResult};

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

pub(super) fn list(services: &McpServices, input: DocumentListInput) -> ToolResult {
    with_authority(services, || {
        let workspace = services
            .workspaces
            .resolve(input.workspace_id)
            .map_err(|error| failure_from_code(error.code, None))?;
        let scope = DocumentScope::authorized(workspace);
        let parent = input
            .parent_folder_id
            .as_deref()
            .map(|folder_id| {
                services
                    .documents
                    .verify_folder(folder_id, &services.workspaces)
            })
            .transpose()
            .map_err(|error| failure_from_code(error.code, None))?;
        if parent
            .as_ref()
            .is_some_and(|parent| parent.workspace_id() != input.workspace_id)
        {
            return Err(failure_from_code("invalid_handle", None));
        }
        services
            .documents
            .list(
                &scope,
                parent.as_ref(),
                input.cursor.as_deref(),
                input.limit.unwrap_or(100),
            )
            .and_then(|page| serde_json::to_value(page).map_err(|_| unreachable!()))
            .map_err(|error| failure_from_code(error.code, None))
    })
}

pub(super) fn search(services: &McpServices, input: DocumentSearchInput) -> ToolResult {
    with_authority(services, || {
        let workspace = services
            .workspaces
            .resolve(input.workspace_id)
            .map_err(|error| failure_from_code(error.code, None))?;
        services
            .documents
            .search(
                &DocumentScope::authorized(workspace),
                &input.query,
                input.cursor.as_deref(),
                input.limit.unwrap_or(100),
            )
            .and_then(|page| serde_json::to_value(page).map_err(|_| unreachable!()))
            .map_err(|error| failure_from_code(error.code, None))
    })
}

pub(super) fn read(
    services: &McpServices,
    input: DocumentReadInput,
    document_limit_bytes: u64,
) -> ToolResult {
    with_authority(services, || {
        let document = services
            .documents
            .verify_document(&input.document_id, &services.workspaces)
            .map_err(|error| failure_from_code(error.code, None))?;
        let scope = DocumentScope::authorized(document.workspace().clone());
        services
            .documents
            .read(&scope, &document, document_limit_bytes)
            .and_then(|snapshot| serde_json::to_value(snapshot).map_err(|_| unreachable!()))
            .map_err(|error| failure_from_code(error.code, None))
    })
}

pub(super) fn create(
    services: &McpServices,
    input: &DocumentCreateInput,
    options: MutationOptions,
) -> ToolResult {
    with_authority(services, || {
        let workspace = services
            .workspaces
            .resolve(input.workspace_id)
            .map_err(|error| failure_from_code(error.code, None))?;
        let parent = services
            .documents
            .verify_folder(&input.parent_folder_id, &services.workspaces)
            .map_err(|error| failure_from_code(error.code, None))?;
        if parent.workspace_id() != input.workspace_id {
            return Err(failure_from_code("invalid_handle", None));
        }
        services
            .documents
            .create(
                &DocumentScope::authorized(workspace),
                CreateDocument {
                    parent: &parent,
                    name: &input.name,
                    contents: &input.contents,
                },
                options,
            )
            .and_then(|mutation| serde_json::to_value(mutation).map_err(|_| unreachable!()))
            .map_err(|error| failure_from_code(error.code, None))
    })
}

pub(super) fn update(
    services: &McpServices,
    input: &DocumentUpdateInput,
    options: MutationOptions,
) -> ToolResult {
    with_authority(services, || {
        let document = services
            .documents
            .verify_document(&input.document_id, &services.workspaces)
            .map_err(|error| failure_from_code(error.code, None))?;
        let scope = DocumentScope::authorized(document.workspace().clone());
        services
            .documents
            .update(
                &scope,
                UpdateDocument {
                    document: &document,
                    contents: &input.contents,
                    expected_revision: &input.expected_revision,
                },
                options,
            )
            .and_then(|mutation| serde_json::to_value(mutation).map_err(|_| unreachable!()))
            .map_err(|error| failure_from_code(error.code, None))
    })
}

pub(super) fn move_document(
    services: &McpServices,
    input: &DocumentMoveInput,
    options: MutationOptions,
) -> ToolResult {
    with_authority(services, || {
        let document = services
            .documents
            .verify_document(&input.document_id, &services.workspaces)
            .map_err(|error| failure_from_code(error.code, None))?;
        let target = services
            .documents
            .verify_folder(&input.target_folder_id, &services.workspaces)
            .map_err(|error| failure_from_code(error.code, None))?;
        let scope = DocumentScope::authorized(document.workspace().clone());
        services
            .documents
            .move_document(
                &scope,
                MoveDocument {
                    document: &document,
                    target_parent: &target,
                    new_name: &input.new_name,
                    expected_revision: &input.expected_revision,
                },
                options,
            )
            .and_then(|mutation| serde_json::to_value(mutation).map_err(|_| unreachable!()))
            .map_err(|error| failure_from_code(error.code, None))
    })
}

pub(super) fn delete(
    services: &McpServices,
    input: &DocumentDeleteInput,
    options: MutationOptions,
    deletion: crate::mcp::config::DeletionPolicy,
) -> ToolResult {
    with_authority(services, || {
        let document = services
            .documents
            .verify_document(&input.document_id, &services.workspaces)
            .map_err(|error| failure_from_code(error.code, None))?;
        let scope = DocumentScope::authorized(document.workspace().clone());
        services
            .documents
            .delete(
                &scope,
                DeleteDocument {
                    document: &document,
                    expected_revision: &input.expected_revision,
                    deletion,
                },
                options,
            )
            .and_then(|mutation| serde_json::to_value(mutation).map_err(|_| unreachable!()))
            .map_err(|error| failure_from_code(error.code, None))
    })
}

fn with_authority(services: &McpServices, operation: impl FnOnce() -> ToolResult) -> ToolResult {
    services
        .workspaces
        .with_authority(operation)
        .map_err(|error| failure_from_code(error.code, None))?
}
