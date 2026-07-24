mod document;
mod settings;
mod sync;
mod workspace;

use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rmcp::{
    model::{
        CallToolRequestMethod, CallToolRequestParams, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
        ToolAnnotations,
    },
    schemars::JsonSchema,
    service::{NotificationContext, Peer, RequestContext},
    ErrorData, RoleServer, ServerHandler,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    app_settings::AppSettingsService,
    markdown_files::{DocumentService, MutationOptions},
    remote_sync::mcp_service::SyncService,
};

use super::{
    audit::{AuditEvent, AuditOutcome, AuditSink},
    config::{McpConfig, McpConfigDocument, McpConfigManager, ToolCapability},
    confirmation::ConfirmationPresenter,
    error::McpToolFailure,
    policy::{OperationDescriptor, OperationRisk, PolicyEngine},
    workspaces::WorkspaceRegistry,
};

pub(super) type ToolResult = Result<Value, McpToolFailure>;

#[derive(Clone)]
pub(crate) struct McpServices {
    pub(crate) config: Arc<McpConfigManager>,
    pub(crate) workspaces: Arc<WorkspaceRegistry>,
    pub(crate) documents: Arc<DocumentService>,
    pub(crate) settings: Arc<AppSettingsService>,
    pub(crate) sync: Arc<SyncService>,
    pub(crate) policy: Arc<PolicyEngine>,
    pub(crate) audit: Arc<AuditSink>,
}

#[derive(Clone)]
struct HandlerRuntime {
    call_gate: CallGate,
    confirmation: Arc<dyn ConfirmationPresenter>,
    peers: Arc<std::sync::Mutex<Vec<Peer<RoleServer>>>>,
    services: McpServices,
}

#[derive(Clone)]
struct CallGate {
    state: Arc<Mutex<CallGateState>>,
}

struct CallGateState {
    active: usize,
    burst_requests: u32,
    concurrent_calls: usize,
    requests_per_minute: u32,
    tokens: f64,
    updated_at: Instant,
}

struct CallPermit(Arc<Mutex<CallGateState>>);

impl CallGate {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(CallGateState {
                active: 0,
                burst_requests: 0,
                concurrent_calls: 0,
                requests_per_minute: 0,
                tokens: 0.0,
                updated_at: Instant::now(),
            })),
        }
    }

    fn enter(&self, config: &McpConfig) -> Option<CallPermit> {
        let mut state = self.state.lock().ok()?;
        if state.requests_per_minute != config.requests_per_minute
            || state.burst_requests != config.burst_requests
            || state.concurrent_calls != config.concurrent_calls
        {
            state.requests_per_minute = config.requests_per_minute.max(1);
            state.burst_requests = config.burst_requests.max(1);
            state.concurrent_calls = config.concurrent_calls.max(1);
            state.tokens = f64::from(state.burst_requests);
            state.updated_at = Instant::now();
        }
        let now = Instant::now();
        let capacity = f64::from(state.burst_requests);
        let refill_per_second = f64::from(state.requests_per_minute) / 60.0;
        state.tokens = (state.tokens
            + now.duration_since(state.updated_at).as_secs_f64() * refill_per_second)
            .min(capacity);
        state.updated_at = now;
        if state.active >= state.concurrent_calls || state.tokens < 1.0 {
            return None;
        }
        state.active += 1;
        state.tokens -= 1.0;
        Some(CallPermit(Arc::clone(&self.state)))
    }
}

impl Drop for CallPermit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            state.active = state.active.saturating_sub(1);
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct QingYuMcpHandler {
    runtime: Option<Arc<HandlerRuntime>>,
}

impl QingYuMcpHandler {
    pub(crate) fn new(services: McpServices, confirmation: Arc<dyn ConfirmationPresenter>) -> Self {
        Self {
            runtime: Some(Arc::new(HandlerRuntime {
                call_gate: CallGate::new(),
                confirmation,
                peers: Arc::new(std::sync::Mutex::new(Vec::new())),
                services,
            })),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        services: McpServices,
        confirmation: Arc<dyn ConfirmationPresenter>,
    ) -> Self {
        Self::new(services, confirmation)
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::default()
    }

    pub(crate) fn list_tools_current(&self) -> Result<Vec<Tool>, McpToolFailure> {
        let runtime = self.runtime()?;
        let document = runtime
            .services
            .config
            .snapshot()
            .map_err(|_| failure_from_code("mcp_disabled", None))?;
        if !document.config.enabled {
            return Ok(Vec::new());
        }
        Ok(tool_catalog(&document.config))
    }

    pub(crate) fn invalidate_previews(&self) {
        if let Some(runtime) = &self.runtime {
            runtime.services.policy.invalidate_previews();
        }
    }

    pub(crate) async fn notify_tools_changed(&self) {
        let Some(runtime) = &self.runtime else {
            return;
        };
        let peers = runtime
            .peers
            .lock()
            .map(|mut peers| std::mem::take(&mut *peers))
            .unwrap_or_default();
        let mut live = Vec::new();
        for peer in peers {
            if peer.notify_tool_list_changed().await.is_ok() {
                live.push(peer);
            }
        }
        if let Ok(mut peers) = runtime.peers.lock() {
            peers.extend(live);
        }
    }

    pub(crate) async fn call_tool_current(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<CallToolResult, ErrorData> {
        let Some(spec) = tool_spec(name) else {
            return Err(ErrorData::method_not_found::<CallToolRequestMethod>());
        };
        let runtime = self
            .runtime()
            .map_err(|_| ErrorData::invalid_params("QingYu MCP is not initialized.", None))?;
        let started = Instant::now();
        let workspace_id = arguments
            .get("workspaceId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok());
        let dry_run = arguments
            .get("dryRun")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let snapshot = match runtime.services.config.snapshot() {
            Ok(snapshot) => snapshot,
            Err(_) => {
                return Ok(structured_failure(
                    failure_from_code("mcp_disabled", None),
                    8 * 1024 * 1024,
                ));
            }
        };
        let call_permit = snapshot
            .config
            .enabled
            .then(|| runtime.call_gate.enter(&snapshot.config))
            .flatten();
        let result = if !snapshot.config.enabled {
            Err(failure_from_code("mcp_disabled", None))
        } else if call_permit.is_none() {
            Err(failure_from_code("rate_limited", None))
        } else if name != "workspace_list" && !snapshot.config.permissions.allows(spec.capability) {
            Err(permission_failure(spec.capability))
        } else if !spec.read_only && runtime.services.audit.preflight().is_err() {
            Err(failure_from_code("audit_write_failed", None))
        } else {
            self.dispatch(name, arguments, &snapshot).await
        };
        let audit_outcome = match &result {
            Ok(value) if value.get("previewToken").is_some() => AuditOutcome::Previewed,
            Ok(_) => AuditOutcome::Succeeded,
            Err(_) => AuditOutcome::Failed,
        };
        let error_code = result
            .as_ref()
            .err()
            .map(|failure| failure.code.to_string());
        let _audit_result = runtime.services.audit.record(AuditEvent {
            request_id: Uuid::new_v4(),
            tool: name.to_string(),
            workspace_id,
            workspace_display_name: workspace_id.and_then(|workspace_id| {
                runtime
                    .services
                    .workspaces
                    .with_authority(|| {
                        runtime
                            .services
                            .workspaces
                            .list_safe()
                            .into_iter()
                            .find(|workspace| workspace.workspace_id == workspace_id)
                            .map(|workspace| workspace.display_name)
                    })
                    .ok()
                    .flatten()
            }),
            logical_target: None,
            dry_run,
            confirmation: None,
            outcome: audit_outcome,
            error_code,
            revision_before: arguments_revision(&result, false),
            revision_after: arguments_revision(&result, true),
            sync_run_id: result
                .as_ref()
                .ok()
                .and_then(|value| value.get("runId"))
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok()),
            duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            counts: BTreeMap::new(),
        });
        Ok(match result {
            Ok(value) => structured_success(name, value, snapshot.config.response_limit_bytes),
            Err(failure) => structured_failure(failure, snapshot.config.response_limit_bytes),
        })
    }

    async fn dispatch(
        &self,
        name: &str,
        arguments: Value,
        snapshot: &McpConfigDocument,
    ) -> ToolResult {
        let runtime = self.runtime()?;
        match name {
            "workspace_list" => {
                parse::<workspace::WorkspaceListInput>(arguments)?;
                workspace::list(&runtime.services)
            }
            "document_list" => document::list(
                &runtime.services,
                parse::<document::DocumentListInput>(arguments)?,
            ),
            "document_search" => document::search(
                &runtime.services,
                parse::<document::DocumentSearchInput>(arguments)?,
            ),
            "document_read" => document::read(
                &runtime.services,
                parse::<document::DocumentReadInput>(arguments)?,
                snapshot.config.document_limit_bytes,
            ),
            "document_create" => {
                let input = parse::<document::DocumentCreateInput>(arguments)?;
                let workspace_id = input.workspace_id;
                let target = Some(input.name.clone());
                self.guarded_mutation(
                    name,
                    &input,
                    workspace_id,
                    target,
                    None,
                    input.dry_run.unwrap_or(false),
                    input.preview_token.as_deref(),
                    OperationRisk::Write,
                    ToolCapability::DocumentsWrite,
                    snapshot,
                    || async {
                        document::create(
                            &runtime.services,
                            &input,
                            mutation_options(&runtime.services.sync, &snapshot.config),
                        )
                    },
                )
                .await
            }
            "document_update" => {
                let input = parse::<document::DocumentUpdateInput>(arguments)?;
                let workspace_id = document_workspace_id(&runtime.services, &input.document_id)?;
                self.guarded_mutation(
                    name,
                    &input,
                    workspace_id,
                    Some(input.document_id.clone()),
                    Some(input.expected_revision.clone()),
                    input.dry_run.unwrap_or(false),
                    input.preview_token.as_deref(),
                    OperationRisk::Write,
                    ToolCapability::DocumentsWrite,
                    snapshot,
                    || async {
                        document::update(
                            &runtime.services,
                            &input,
                            mutation_options(&runtime.services.sync, &snapshot.config),
                        )
                    },
                )
                .await
            }
            "document_move" => {
                let input = parse::<document::DocumentMoveInput>(arguments)?;
                let workspace_id = document_workspace_id(&runtime.services, &input.document_id)?;
                self.guarded_mutation(
                    name,
                    &input,
                    workspace_id,
                    Some(input.new_name.clone()),
                    Some(input.expected_revision.clone()),
                    input.dry_run.unwrap_or(false),
                    input.preview_token.as_deref(),
                    OperationRisk::Write,
                    ToolCapability::DocumentsMove,
                    snapshot,
                    || async {
                        document::move_document(
                            &runtime.services,
                            &input,
                            mutation_options(&runtime.services.sync, &snapshot.config),
                        )
                    },
                )
                .await
            }
            "document_delete" => {
                let input = parse::<document::DocumentDeleteInput>(arguments)?;
                let workspace_id = document_workspace_id(&runtime.services, &input.document_id)?;
                self.guarded_mutation(
                    name,
                    &input,
                    workspace_id,
                    Some(input.document_id.clone()),
                    Some(input.expected_revision.clone()),
                    input.dry_run.unwrap_or(false),
                    input.preview_token.as_deref(),
                    OperationRisk::Destructive,
                    ToolCapability::DocumentsDelete,
                    snapshot,
                    || async {
                        document::delete(
                            &runtime.services,
                            &input,
                            mutation_options(&runtime.services.sync, &snapshot.config),
                            snapshot.config.deletion,
                        )
                    },
                )
                .await
            }
            "settings_get" => {
                parse::<settings::SettingsGetInput>(arguments)?;
                settings::get(&runtime.services)
            }
            "settings_update" => {
                let input = parse::<settings::SettingsUpdateInput>(arguments)?;
                self.guarded_mutation(
                    name,
                    &input,
                    Uuid::nil(),
                    Some("application settings".to_string()),
                    Some(input.expected_revision.clone()),
                    input.dry_run.unwrap_or(false),
                    input.preview_token.as_deref(),
                    OperationRisk::Write,
                    ToolCapability::SettingsWrite,
                    snapshot,
                    || async { settings::update(&runtime.services, &input) },
                )
                .await
            }
            "sync_config_get" => sync::get_config(
                &runtime.services,
                parse::<sync::SyncConfigGetInput>(arguments)?,
            ),
            "sync_config_update" => {
                let input = parse::<sync::SyncConfigUpdateInput>(arguments)?;
                let risk = if input.changes_remote_target() {
                    OperationRisk::HighRisk
                } else {
                    OperationRisk::Write
                };
                self.guarded_mutation(
                    name,
                    &input,
                    Uuid::nil(),
                    Some("sync configuration".to_string()),
                    Some(input.expected_revision.clone()),
                    input.dry_run.unwrap_or(false),
                    input.preview_token.as_deref(),
                    risk,
                    ToolCapability::SyncWrite,
                    snapshot,
                    || async { sync::update_config(&runtime.services, &input) },
                )
                .await
            }
            "sync_credentials_update" => {
                let input = parse::<sync::SyncCredentialsUpdateInput>(arguments)?;
                self.guarded_mutation(
                    name,
                    &input,
                    Uuid::nil(),
                    Some("sync credentials".to_string()),
                    Some(input.expected_revision.clone()),
                    input.dry_run.unwrap_or(false),
                    input.preview_token.as_deref(),
                    OperationRisk::HighRisk,
                    ToolCapability::SyncCredentialsWrite,
                    snapshot,
                    || async { sync::update_credentials(&runtime.services, &input) },
                )
                .await
            }
            "sync_test" => {
                sync::test(&runtime.services, parse::<sync::SyncTestInput>(arguments)?).await
            }
            "sync_run" => {
                let input = parse::<sync::SyncRunInput>(arguments)?;
                self.guarded_mutation(
                    name,
                    &input,
                    Uuid::nil(),
                    Some("workspace synchronization".to_string()),
                    Some(input.expected_revision.clone()),
                    input.dry_run.unwrap_or(false),
                    input.preview_token.as_deref(),
                    OperationRisk::Destructive,
                    ToolCapability::SyncRun,
                    snapshot,
                    || async {
                        sync::run(
                            &runtime.services,
                            &input,
                            snapshot.config.sync_execution,
                            Duration::from_secs(snapshot.config.tool_timeout_secs),
                        )
                        .await
                    },
                )
                .await
            }
            "sync_status" => sync::status(
                &runtime.services,
                parse::<sync::SyncStatusInput>(arguments)?,
            ),
            _ => Err(failure_from_code("invalid_arguments", None)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn guarded_mutation<T, F, Fut>(
        &self,
        tool: &str,
        input: &T,
        workspace_id: Uuid,
        target: Option<String>,
        expected_revision: Option<String>,
        dry_run: bool,
        preview_token: Option<&str>,
        risk: OperationRisk,
        capability: ToolCapability,
        snapshot: &McpConfigDocument,
        action: F,
    ) -> ToolResult
    where
        T: Serialize,
        F: FnOnce() -> Fut,
        Fut: Future<Output = ToolResult>,
    {
        let runtime = self.runtime()?;
        let canonical_arguments = canonical_arguments(input)?;
        let workspace = runtime
            .services
            .workspaces
            .with_authority(|| {
                runtime
                    .services
                    .workspaces
                    .list_safe()
                    .into_iter()
                    .find(|workspace| workspace.workspace_id == workspace_id)
            })
            .map_err(|error| failure_from_code(error.code, None))?;
        let descriptor = OperationDescriptor {
            tool: tool.to_string(),
            workspace_id: (workspace_id != Uuid::nil()).then_some(workspace_id),
            workspace_display_name: workspace.map(|workspace| workspace.display_name),
            target,
            expected_revision,
            risk,
            canonical_arguments,
        };
        let requirements =
            PolicyEngine::requirements(snapshot.config.confirmation, snapshot.config.dry_run, risk);
        if dry_run || (requirements.preview_required && preview_token.is_none()) {
            let preview = runtime
                .services
                .policy
                .preview(
                    &descriptor,
                    runtime.services.config.generation(),
                    runtime.services.workspaces.generation(),
                )
                .map_err(|error| failure_from_code(error.code, None))?;
            return Ok(serde_json::json!({
                "previewToken": preview.token,
                "expiresAt": preview.expires_at,
                "tool": preview.tool,
                "target": preview.target,
                "expectedRevision": preview.expected_revision,
            }));
        }
        if let Some(preview_token) = preview_token {
            runtime
                .services
                .policy
                .consume_preview(
                    preview_token,
                    &descriptor,
                    runtime.services.config.generation(),
                    runtime.services.workspaces.generation(),
                )
                .map_err(|error| failure_from_code(error.code, None))?;
        }
        runtime
            .services
            .policy
            .confirm_if_required(
                snapshot.config.confirmation,
                &descriptor,
                runtime.confirmation.as_ref(),
            )
            .await
            .map_err(|error| failure_from_code(error.code, None))?;
        let current = runtime
            .services
            .config
            .snapshot()
            .map_err(|_| failure_from_code("mcp_disabled", None))?;
        if !current.config.enabled {
            return Err(failure_from_code("mcp_disabled", None));
        }
        if !current.config.permissions.allows(capability) {
            return Err(permission_failure(capability));
        }
        action().await
    }

    fn runtime(&self) -> Result<&HandlerRuntime, McpToolFailure> {
        self.runtime
            .as_deref()
            .ok_or_else(|| failure_from_code("mcp_disabled", None))
    }
}

impl ServerHandler for QingYuMcpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("qingyu", env!("CARGO_PKG_VERSION")).with_title("QingYu"),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.list_tools_current()
            .map(|tools| ListToolsResult {
                tools,
                ..Default::default()
            })
            .map_err(|_| ErrorData::invalid_params("QingYu MCP tool catalog unavailable.", None))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call_tool_current(
            request.name.as_ref(),
            Value::Object(request.arguments.unwrap_or_default()),
        )
        .await
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        if let Some(runtime) = &self.runtime {
            if let Ok(mut peers) = runtime.peers.lock() {
                peers.push(context.peer);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct ToolSpec {
    capability: ToolCapability,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    open_world: bool,
}

fn tool_spec(name: &str) -> Option<ToolSpec> {
    let spec = match name {
        "workspace_list" | "document_list" | "document_search" | "document_read" => ToolSpec {
            capability: ToolCapability::DocumentsRead,
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
        },
        "document_create" => ToolSpec {
            capability: ToolCapability::DocumentsWrite,
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: false,
        },
        "document_update" => write_spec(ToolCapability::DocumentsWrite),
        "document_move" => write_spec(ToolCapability::DocumentsMove),
        "document_delete" => ToolSpec {
            capability: ToolCapability::DocumentsDelete,
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: false,
        },
        "settings_get" => read_spec(ToolCapability::SettingsRead, false),
        "settings_update" => write_spec(ToolCapability::SettingsWrite),
        "sync_config_get" | "sync_status" => read_spec(ToolCapability::SyncRead, false),
        "sync_config_update" => write_spec(ToolCapability::SyncWrite),
        "sync_credentials_update" => write_spec(ToolCapability::SyncCredentialsWrite),
        "sync_test" => read_spec(ToolCapability::SyncRun, true),
        "sync_run" => ToolSpec {
            capability: ToolCapability::SyncRun,
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: true,
        },
        _ => return None,
    };
    Some(spec)
}

fn read_spec(capability: ToolCapability, open_world: bool) -> ToolSpec {
    ToolSpec {
        capability,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world,
    }
}

fn write_spec(capability: ToolCapability) -> ToolSpec {
    ToolSpec {
        capability,
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: false,
    }
}

fn tool_catalog(config: &McpConfig) -> Vec<Tool> {
    let mut tools = Vec::new();
    push_tool::<workspace::WorkspaceListInput>(
        &mut tools,
        config,
        "workspace_list",
        "List authorized QingYu workspaces.",
    );
    push_tool::<document::DocumentListInput>(
        &mut tools,
        config,
        "document_list",
        "List visible Markdown documents and folders.",
    );
    push_tool::<document::DocumentSearchInput>(
        &mut tools,
        config,
        "document_search",
        "Search visible Markdown documents.",
    );
    push_tool::<document::DocumentReadInput>(
        &mut tools,
        config,
        "document_read",
        "Read one Markdown document by opaque ID.",
    );
    push_tool::<document::DocumentCreateInput>(
        &mut tools,
        config,
        "document_create",
        "Create one Markdown document.",
    );
    push_tool::<document::DocumentUpdateInput>(
        &mut tools,
        config,
        "document_update",
        "Update one Markdown document with revision protection.",
    );
    push_tool::<document::DocumentMoveInput>(
        &mut tools,
        config,
        "document_move",
        "Move or rename one Markdown document.",
    );
    push_tool::<document::DocumentDeleteInput>(
        &mut tools,
        config,
        "document_delete",
        "Delete one Markdown document under QingYu policy.",
    );
    push_tool::<settings::SettingsGetInput>(
        &mut tools,
        config,
        "settings_get",
        "Read the exposed QingYu settings.",
    );
    push_tool::<settings::SettingsUpdateInput>(
        &mut tools,
        config,
        "settings_update",
        "Update exposed QingYu settings atomically.",
    );
    push_tool::<sync::SyncConfigGetInput>(
        &mut tools,
        config,
        "sync_config_get",
        "Read sanitized application sync configuration.",
    );
    push_tool::<sync::SyncConfigUpdateInput>(
        &mut tools,
        config,
        "sync_config_update",
        "Update non-secret application sync configuration.",
    );
    push_tool::<sync::SyncCredentialsUpdateInput>(
        &mut tools,
        config,
        "sync_credentials_update",
        "Write or clear application sync credentials.",
    );
    push_tool::<sync::SyncTestInput>(
        &mut tools,
        config,
        "sync_test",
        "Test the configured remote sync connection.",
    );
    push_tool::<sync::SyncRunInput>(
        &mut tools,
        config,
        "sync_run",
        "Synchronize the primary notes workspace and portable settings.",
    );
    push_tool::<sync::SyncStatusInput>(
        &mut tools,
        config,
        "sync_status",
        "Read a sync run or persisted application status.",
    );
    tools
}

fn push_tool<T: JsonSchema + 'static>(
    tools: &mut Vec<Tool>,
    config: &McpConfig,
    name: &'static str,
    description: &'static str,
) {
    let Some(spec) = tool_spec(name) else {
        return;
    };
    if name != "workspace_list" && !config.permissions.allows(spec.capability) {
        return;
    }
    let mut tool = Tool::new(name, description, Arc::new(Default::default()))
        .with_input_schema::<T>()
        .with_annotations(ToolAnnotations::from_raw(
            Some(name.replace('_', " ")),
            Some(spec.read_only),
            Some(spec.destructive),
            Some(spec.idempotent),
            Some(spec.open_world),
        ));
    Arc::make_mut(&mut tool.input_schema)
        .insert("additionalProperties".to_string(), Value::Bool(false));
    tool.output_schema = Some(Arc::new(serde_json::Map::from_iter([(
        "type".to_string(),
        Value::String("object".to_string()),
    )])));
    tools.push(tool);
}

fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, McpToolFailure> {
    serde_json::from_value(arguments).map_err(|_| failure_from_code("invalid_arguments", None))
}

fn canonical_arguments<T: Serialize>(input: &T) -> Result<String, McpToolFailure> {
    let mut value =
        serde_json::to_value(input).map_err(|_| failure_from_code("invalid_arguments", None))?;
    if let Some(object) = value.as_object_mut() {
        object.remove("dryRun");
        object.remove("previewToken");
    }
    serde_json::to_string(&value).map_err(|_| failure_from_code("invalid_arguments", None))
}

fn document_workspace_id(
    services: &McpServices,
    document_id: &str,
) -> Result<Uuid, McpToolFailure> {
    services
        .workspaces
        .with_authority(|| {
            services
                .documents
                .verify_document(document_id, &services.workspaces)
                .map(|document| document.workspace_id())
                .map_err(|error| failure_from_code(error.code, None))
        })
        .map_err(|error| failure_from_code(error.code, None))?
}

fn mutation_options(sync: &SyncService, config: &McpConfig) -> MutationOptions {
    MutationOptions {
        sync_after_write: config.sync_after_write,
        workspace_sync_enabled: sync.sync_enabled_for_authoritative_primary(),
    }
}

fn structured_success(tool: &str, value: Value, limit: u64) -> CallToolResult {
    if serialized_size(&value) > limit {
        return structured_failure(failure_from_code("response_too_large", None), limit);
    }
    let mut result = CallToolResult::structured(value);
    result.content = vec![ContentBlock::text(format!("QingYu completed {tool}."))];
    result
}

fn structured_failure(failure: McpToolFailure, limit: u64) -> CallToolResult {
    let value = serde_json::to_value(&failure).unwrap_or_else(|_| {
        serde_json::json!({
            "code": "response_too_large",
            "message": "The QingYu MCP response could not be returned safely.",
            "retryable": false,
            "recoveryHint": "Reduce the requested result size."
        })
    });
    let value = if serialized_size(&value) > limit {
        serde_json::json!({
            "code": "response_too_large",
            "message": "The QingYu MCP response exceeds the configured limit.",
            "retryable": false,
            "recoveryHint": "Reduce the requested result size."
        })
    } else {
        value
    };
    let mut result = CallToolResult::structured_error(value);
    result.content = vec![ContentBlock::text(format!(
        "QingYu could not complete the operation: {}",
        failure.message
    ))];
    result
}

fn serialized_size(value: &Value) -> u64 {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len().try_into().unwrap_or(u64::MAX))
        .unwrap_or(u64::MAX)
}

fn permission_failure(capability: ToolCapability) -> McpToolFailure {
    if capability == ToolCapability::SyncCredentialsWrite {
        failure_from_code("credential_write_denied", None)
    } else {
        failure_from_code("permission_denied", None)
    }
}

pub(super) fn failure_from_code(code: &str, current_revision: Option<&str>) -> McpToolFailure {
    let normalized = match code {
        "document_already_exists" => "target_already_exists",
        "invalid_settings_field" => "settings_field_not_exposed",
        "settings_revision_conflict" | "sync_revision_conflict" => "revision_conflict",
        "sync_config_unavailable" | "sync_run_unavailable" => "sync_not_configured",
        "invalid_sync_config_patch" | "invalid_sync_credentials" => "invalid_arguments",
        other => other,
    };
    let (code, message, retryable, recovery_hint) = failure_details(normalized);
    McpToolFailure {
        code,
        message: message.to_string(),
        retryable,
        recovery_hint: Some(recovery_hint.to_string()),
        current_revision: current_revision.map(str::to_string),
    }
}

fn failure_details(code: &str) -> (&'static str, &'static str, bool, &'static str) {
    match code {
        "mcp_disabled" => (
            "mcp_disabled",
            "QingYu MCP is disabled.",
            false,
            "Enable MCP in QingYu settings.",
        ),
        "permission_denied" => (
            "permission_denied",
            "The current QingYu MCP policy denies this operation.",
            false,
            "Enable the required permission for the current QingYu project.",
        ),
        "credential_write_denied" => (
            "credential_write_denied",
            "Writing sync credentials is not allowed.",
            false,
            "Enable sync credential writes in QingYu.",
        ),
        "workspace_not_authorized" => (
            "workspace_not_authorized",
            "The workspace is not authorized.",
            false,
            "Authorize the folder in QingYu MCP settings.",
        ),
        "workspace_unavailable" => (
            "workspace_unavailable",
            "The authorized workspace is unavailable.",
            true,
            "Restore access to the authorized folder and retry.",
        ),
        "mcp-workspace-unavailable" => (
            "mcp-workspace-unavailable",
            "QingYu MCP document tools require a valid primary notes workspace.",
            false,
            "Choose or restore the primary notes workspace in QingYu settings.",
        ),
        "mcp-handle-stale" => (
            "mcp-handle-stale",
            "The object identifier belongs to an older primary notes workspace.",
            false,
            "List the primary workspace again to obtain current object identifiers.",
        ),
        "invalid_handle" => (
            "invalid_handle",
            "The object identifier is invalid.",
            false,
            "List the workspace again to obtain a current object ID.",
        ),
        "document_not_found" => (
            "document_not_found",
            "The document is unavailable.",
            false,
            "List the containing folder again.",
        ),
        "revision_conflict" => (
            "revision_conflict",
            "The object changed after it was read.",
            true,
            "Read the current revision and retry.",
        ),
        "target_already_exists" => (
            "target_already_exists",
            "The destination already exists.",
            false,
            "Choose a different document name.",
        ),
        "path_boundary_violation" => (
            "path_boundary_violation",
            "The request crosses the authorized workspace boundary.",
            false,
            "Use only QingYu-issued object IDs.",
        ),
        "protected_path" => (
            "protected_path",
            "The requested path is protected.",
            false,
            "Choose an unprotected Markdown location.",
        ),
        "document_too_large" => (
            "document_too_large",
            "The document exceeds the configured limit.",
            false,
            "Increase the QingYu limit or use a smaller document.",
        ),
        "settings_field_not_exposed" => (
            "settings_field_not_exposed",
            "The setting is not exposed through MCP.",
            false,
            "Use a field returned by settings_get.",
        ),
        "confirmation_rejected" => (
            "confirmation_rejected",
            "The operation was rejected in QingYu.",
            false,
            "Submit the request again if the user wants to reconsider.",
        ),
        "confirmation_timeout" => (
            "confirmation_timeout",
            "The QingYu confirmation timed out.",
            true,
            "Retry while QingYu is available.",
        ),
        "preview_required" => (
            "preview_required",
            "A matching preview is required.",
            false,
            "Use the returned preview token without changing arguments.",
        ),
        "preview_expired" => (
            "preview_expired",
            "The preview token expired.",
            true,
            "Request a new preview.",
        ),
        "sync_not_configured" => (
            "sync_not_configured",
            "Workspace sync is not configured and ready.",
            false,
            "Configure workspace sync in QingYu first.",
        ),
        "sync_in_progress" => (
            "sync_in_progress",
            "A matching synchronization is already running.",
            true,
            "Poll sync_status for the existing run.",
        ),
        "rate_limited" => (
            "rate_limited",
            "The MCP operation rate is limited.",
            true,
            "Wait briefly and retry.",
        ),
        "response_too_large" => (
            "response_too_large",
            "The MCP response exceeds the configured limit.",
            false,
            "Request a smaller page or document.",
        ),
        "audit_write_failed" => (
            "audit_write_failed",
            "The MCP audit log is unavailable, so the write was not performed.",
            true,
            "Restore access to the QingYu audit log and retry.",
        ),
        "invalid_arguments"
        | "invalid_query"
        | "invalid_cursor"
        | "invalid_document_name"
        | "invalid_proxy_url" => (
            "invalid_arguments",
            "The tool arguments are invalid.",
            false,
            "Use the published closed tool schema and current IDs.",
        ),
        _ => (
            "operation_failed",
            "QingYu could not complete the operation safely.",
            true,
            "Refresh current state and retry.",
        ),
    }
}

fn arguments_revision(result: &ToolResult, after: bool) -> Option<String> {
    let value = result.as_ref().ok()?;
    if after {
        value
            .get("revision")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::{future::Future, path::PathBuf, pin::Pin, sync::Arc};

    use rmcp::ServerHandler;

    use crate::{
        mcp::{
            config::{McpConfig, SyncAfterWritePolicy},
            workspaces::WorkspaceRegistry,
        },
        remote_sync::mcp_service::{SyncRunner, SyncService},
        sync_config::{
            model::SyncConfigPatch,
            status::SyncRunResult,
            storage::{enable_at_app_data, patch_at_app_data},
        },
    };

    struct UnusedSyncRunner;

    impl SyncRunner for UnusedSyncRunner {
        fn run(
            &self,
            _notes_root: PathBuf,
            _revision: String,
        ) -> Pin<Box<dyn Future<Output = Result<SyncRunResult, String>> + Send + 'static>> {
            Box::pin(async { panic!("mutation option preparation must not start synchronization") })
        }
    }

    #[test]
    fn server_identity_uses_qingyu_without_resources_or_prompts() {
        let handler = super::QingYuMcpHandler::for_test();
        let info = handler.get_info();

        assert_eq!(info.server_info.name, "qingyu");
        assert_eq!(info.server_info.title.as_deref(), Some("QingYu"));
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_none());
        assert!(info.capabilities.prompts.is_none());
    }

    #[test]
    fn mutation_options_ignore_stale_workspace_after_primary_switch() {
        let app_data = tempfile::tempdir().expect("app data");
        let created =
            enable_at_app_data(app_data.path(), None).expect("create application sync config");
        patch_at_app_data(
            app_data.path(),
            &created.document.revision,
            SyncConfigPatch::Enabled(true),
        )
        .expect("enable application sync");
        let sync = SyncService::new_for_test_with_app_data(
            Arc::new(UnusedSyncRunner),
            app_data.path().to_path_buf(),
            None,
        );
        let config = McpConfig {
            sync_after_write: SyncAfterWritePolicy::FollowWorkspace,
            ..McpConfig::default()
        };
        let previous_root = tempfile::tempdir().expect("previous primary workspace");
        let current_root = tempfile::tempdir().expect("current primary workspace");
        let workspaces = WorkspaceRegistry::new(Vec::new());
        let previous = workspaces
            .activate_current(previous_root.path())
            .expect("activate previous primary workspace");
        workspaces
            .activate_current(current_root.path())
            .expect("switch primary workspace");
        let stale = match workspaces.resolve(previous.workspace_id) {
            Ok(_) => panic!("previous workspace must be stale"),
            Err(error) => error,
        };
        assert_eq!(stale.code, "mcp-handle-stale");

        let options = super::mutation_options(&sync, &config);

        assert_eq!(
            options.sync_after_write,
            SyncAfterWritePolicy::FollowWorkspace
        );
        assert!(options.workspace_sync_enabled);
    }
}
