use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use tauri::{Emitter, Manager};

use crate::{
    kernel_bootstrap::NativeKernelBootstrapOwner,
    kernel_host::{
        desktop_kernel_owner::{DesktopKernelEdgeEmitter, DesktopKernelOwner},
        kernel_endpoint_record::KernelEndpointRecordReader,
        KernelHostPhase, KernelHostSnapshot, KernelHostSupervisor, KernelHostTimeouts,
        NativeKernelLaunch,
    },
    kernel_process::NativeKernelProcessFactory,
    writer_authority::{KernelWriterPublicationGate, WorkspaceRootIdentity, WriterAuthority},
};

pub(crate) const DESKTOP_KERNEL_STARTUP_CHANGED_EVENT: &str =
    "qingyu://desktop-kernel-startup-changed";
const NATIVE_KERNEL_BOOTSTRAP_CHANGED_EVENT: &str = "qingyu://kernel-bootstrap-changed";

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DesktopKernelStartupStatus {
    Resolving,
    Unselected,
    Invalid,
    UnsupportedVersion,
    Unavailable,
    Starting,
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopKernelStartupSnapshot {
    status: DesktopKernelStartupStatus,
}

struct DesktopKernelRuntimeInner {
    closed: bool,
    owner: Option<Arc<DesktopKernelOwner>>,
    owner_token: Option<u64>,
    owner_publication_sequence: u64,
    mcp_authority_epoch: u64,
    selection: Option<DesktopKernelSelection>,
    status: DesktopKernelStartupStatus,
    next_attempt_token: u64,
    active_attempt_token: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DesktopKernelSelection {
    workspace_root: PathBuf,
    renderer_origin: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DesktopKernelStartAttempt {
    token: u64,
    selection: DesktopKernelSelection,
}

pub(crate) struct DesktopKernelSwitchAttempt {
    start: DesktopKernelStartAttempt,
}

struct AcceptedKernelPublication {
    mcp_authority_epoch: u64,
    status_changed: bool,
    workspace_root: Option<PathBuf>,
}

pub(crate) struct DesktopKernelRuntimeState {
    bootstrap: NativeKernelBootstrapOwner,
    inner: Mutex<DesktopKernelRuntimeInner>,
}

impl DesktopKernelRuntimeState {
    pub(crate) fn new(
        bootstrap: NativeKernelBootstrapOwner,
        status: DesktopKernelStartupStatus,
    ) -> Self {
        Self {
            bootstrap,
            inner: Mutex::new(DesktopKernelRuntimeInner {
                closed: false,
                owner: None,
                owner_token: None,
                owner_publication_sequence: 0,
                mcp_authority_epoch: 0,
                selection: None,
                status,
                next_attempt_token: 0,
                active_attempt_token: None,
            }),
        }
    }

    pub(crate) fn snapshot(&self) -> Result<DesktopKernelStartupSnapshot, String> {
        self.inner
            .lock()
            .map(|inner| DesktopKernelStartupSnapshot {
                status: inner.status,
            })
            .map_err(|_| runtime_unavailable())
    }

    pub(crate) fn is_invalid(&self) -> Result<bool, String> {
        self.inner
            .lock()
            .map(|inner| inner.status == DesktopKernelStartupStatus::Invalid)
            .map_err(|_| runtime_unavailable())
    }

    pub(crate) fn endpoint_reader(&self) -> KernelEndpointRecordReader {
        self.bootstrap.endpoint_reader()
    }

    pub(crate) fn reserve_resolution_retry(&self, app: &tauri::AppHandle) -> Result<bool, String> {
        let mut inner = self.inner.lock().map_err(|_| runtime_unavailable())?;
        match inner.status {
            DesktopKernelStartupStatus::Unavailable
                if !inner.closed && inner.owner.is_none() && inner.selection.is_none() =>
            {
                inner.status = DesktopKernelStartupStatus::Resolving;
                drop(inner);
                emit_startup_changed(app);
                Ok(true)
            }
            DesktopKernelStartupStatus::Failed if !inner.closed => Ok(false),
            _ => Err(runtime_unavailable()),
        }
    }

    pub(crate) fn complete_initial_resolution(
        &self,
        app: &tauri::AppHandle,
        status: DesktopKernelStartupStatus,
    ) -> Result<(), String> {
        if !matches!(
            status,
            DesktopKernelStartupStatus::Unselected
                | DesktopKernelStartupStatus::Invalid
                | DesktopKernelStartupStatus::UnsupportedVersion
                | DesktopKernelStartupStatus::Unavailable
        ) {
            return Err(runtime_unavailable());
        }
        let mut inner = self.inner.lock().map_err(|_| runtime_unavailable())?;
        if inner.closed
            || inner.status != DesktopKernelStartupStatus::Resolving
            || inner.owner.is_some()
            || inner.selection.is_some()
        {
            return Err(runtime_unavailable());
        }
        inner.status = status;
        drop(inner);
        emit_startup_changed(app);
        Ok(())
    }

    pub(crate) fn start_selected(
        self: &Arc<Self>,
        app: &tauri::AppHandle,
        workspace_root: PathBuf,
        renderer_origin: String,
    ) -> Result<(), String> {
        let selection = DesktopKernelSelection {
            workspace_root,
            renderer_origin,
        };
        let attempt = self.reserve_initial_start(selection, false)?;
        emit_startup_changed(app);
        self.launch_reserved(app, attempt, None);
        Ok(())
    }

    pub(crate) fn start_resolved(
        self: &Arc<Self>,
        app: &tauri::AppHandle,
        workspace_root: PathBuf,
        renderer_origin: String,
    ) -> Result<(), String> {
        let selection = DesktopKernelSelection {
            workspace_root,
            renderer_origin,
        };
        let attempt = self.reserve_initial_start(selection, true)?;
        emit_startup_changed(app);
        self.launch_reserved(app, attempt, None);
        Ok(())
    }

    pub(crate) fn retry_selected(self: &Arc<Self>, app: &tauri::AppHandle) -> Result<(), String> {
        let (attempt, replaced_owner) = self.reserve_retry()?;
        emit_startup_changed(app);
        self.launch_reserved(app, attempt, replaced_owner);
        Ok(())
    }

    pub(crate) async fn begin_workspace_switch(
        self: &Arc<Self>,
        app: &tauri::AppHandle,
        expected_root: &std::path::Path,
        workspace_root: PathBuf,
        renderer_origin: String,
    ) -> Result<Option<DesktopKernelSwitchAttempt>, String> {
        if self.selected_workspace_root()?.as_deref() != Some(expected_root) {
            return Err(runtime_unavailable());
        }
        if expected_root == workspace_root {
            return Ok(None);
        }
        let selection = DesktopKernelSelection {
            workspace_root,
            renderer_origin,
        };
        let (start, replaced_owner, previous_selection) = self.reserve_switch(selection)?;
        if previous_selection.workspace_root != expected_root {
            self.fail_workspace_switch(app, start.token, Some(previous_selection));
            return Err(runtime_unavailable());
        }
        if let Some(epoch) = self.advance_mcp_authority_epoch() {
            schedule_mcp_workspace_refresh(app, epoch, None);
        }
        emit_startup_changed(app);

        let Some(owner) = replaced_owner else {
            self.fail_workspace_switch(app, start.token, Some(previous_selection));
            return Err(runtime_unavailable());
        };
        let stopped = owner.stop().await.is_ok();
        drop_replaced_owner_off_runtime_thread(Some(owner)).await;
        if !stopped {
            self.fail_workspace_switch(app, start.token, Some(previous_selection));
            return Err(runtime_unavailable());
        }
        Ok(Some(DesktopKernelSwitchAttempt { start }))
    }

    pub(crate) fn complete_workspace_switch(
        self: &Arc<Self>,
        app: &tauri::AppHandle,
        attempt: DesktopKernelSwitchAttempt,
    ) -> Result<(), String> {
        if !self.attempt_is_active(attempt.start.token) {
            return Err(runtime_unavailable());
        }
        self.launch_reserved(app, attempt.start, None);
        Ok(())
    }

    pub(crate) fn reconcile_failed_workspace_switch(
        &self,
        app: &tauri::AppHandle,
        attempt: DesktopKernelSwitchAttempt,
        authoritative_root: Option<PathBuf>,
    ) {
        let selection = authoritative_root.map(|workspace_root| DesktopKernelSelection {
            workspace_root,
            renderer_origin: attempt.start.selection.renderer_origin,
        });
        self.fail_workspace_switch(app, attempt.start.token, selection);
    }

    fn selected_workspace_root(&self) -> Result<Option<PathBuf>, String> {
        self.inner
            .lock()
            .map(|inner| {
                inner
                    .selection
                    .as_ref()
                    .map(|selection| selection.workspace_root.clone())
            })
            .map_err(|_| runtime_unavailable())
    }

    fn launch_reserved(
        self: &Arc<Self>,
        app: &tauri::AppHandle,
        attempt: DesktopKernelStartAttempt,
        replaced_owner: Option<Arc<DesktopKernelOwner>>,
    ) {
        let runtime = Arc::clone(self);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            drop_replaced_owner_off_runtime_thread(replaced_owner).await;
            if !runtime.attempt_is_active(attempt.token) {
                return;
            }

            let compose_app = app.clone();
            let selection = attempt.selection;
            let bootstrap = runtime.bootstrap.clone();
            let runtime_for_edges = Arc::downgrade(&runtime);
            let owner_token = attempt.token;
            let composed = run_off_runtime_thread(move || {
                compose_owner_blocking(
                    &compose_app,
                    selection.workspace_root,
                    selection.renderer_origin,
                    bootstrap,
                    runtime_for_edges,
                    owner_token,
                )
            })
            .await;
            let (owner, launch) = match composed {
                Ok(Ok(composed)) => composed,
                Ok(Err(_error)) => {
                    if runtime.replace_status_for_attempt(
                        attempt.token,
                        DesktopKernelStartupStatus::Failed,
                    ) {
                        if let Some(epoch) = runtime.advance_mcp_authority_epoch() {
                            schedule_mcp_workspace_refresh(&app, epoch, None);
                        }
                        emit_startup_changed(&app);
                    }
                    return;
                }
                Err(_join_error) => {
                    if runtime.replace_status_for_attempt(
                        attempt.token,
                        DesktopKernelStartupStatus::Failed,
                    ) {
                        if let Some(epoch) = runtime.advance_mcp_authority_epoch() {
                            schedule_mcp_workspace_refresh(&app, epoch, None);
                        }
                        emit_startup_changed(&app);
                    }
                    return;
                }
            };

            if runtime
                .start_composed_owner_for_attempt(attempt.token, owner, launch)
                .await
                .is_some()
            {
                if let Some(epoch) = runtime.advance_mcp_authority_epoch() {
                    schedule_mcp_workspace_refresh(&app, epoch, None);
                }
                emit_startup_changed(&app);
            }
        });
    }

    fn reserve_initial_start(
        &self,
        selection: DesktopKernelSelection,
        initial_resolution: bool,
    ) -> Result<DesktopKernelStartAttempt, String> {
        let mut inner = self.inner.lock().map_err(|_| runtime_unavailable())?;
        let status_is_eligible = if initial_resolution {
            inner.status == DesktopKernelStartupStatus::Resolving
        } else {
            matches!(
                inner.status,
                DesktopKernelStartupStatus::Unselected | DesktopKernelStartupStatus::Invalid
            )
        };
        if inner.closed || inner.owner.is_some() || inner.selection.is_some() || !status_is_eligible
        {
            return Err(runtime_unavailable());
        }
        let token = reserve_attempt_token(&mut inner)?;
        inner.selection = Some(selection.clone());
        inner.status = DesktopKernelStartupStatus::Starting;
        inner.active_attempt_token = Some(token);
        Ok(DesktopKernelStartAttempt { token, selection })
    }

    fn reserve_retry(
        &self,
    ) -> Result<(DesktopKernelStartAttempt, Option<Arc<DesktopKernelOwner>>), String> {
        let mut inner = self.inner.lock().map_err(|_| runtime_unavailable())?;
        if inner.closed || inner.status != DesktopKernelStartupStatus::Failed {
            return Err(runtime_unavailable());
        }
        let selection = inner.selection.clone().ok_or_else(runtime_unavailable)?;
        let token = reserve_attempt_token(&mut inner)?;
        let replaced_owner = inner.owner.take();
        inner.owner_token = None;
        inner.owner_publication_sequence = 0;
        inner.status = DesktopKernelStartupStatus::Starting;
        inner.active_attempt_token = Some(token);
        Ok((
            DesktopKernelStartAttempt { token, selection },
            replaced_owner,
        ))
    }

    fn reserve_switch(
        &self,
        selection: DesktopKernelSelection,
    ) -> Result<
        (
            DesktopKernelStartAttempt,
            Option<Arc<DesktopKernelOwner>>,
            DesktopKernelSelection,
        ),
        String,
    > {
        let mut inner = self.inner.lock().map_err(|_| runtime_unavailable())?;
        if inner.closed
            || inner.status != DesktopKernelStartupStatus::Ready
            || inner.owner.is_none()
            || inner.active_attempt_token.is_some()
        {
            return Err(runtime_unavailable());
        }
        let previous_selection = inner.selection.clone().ok_or_else(runtime_unavailable)?;
        let token = reserve_attempt_token(&mut inner)?;
        let replaced_owner = inner.owner.take();
        inner.owner_token = None;
        inner.owner_publication_sequence = 0;
        inner.selection = Some(selection.clone());
        inner.status = DesktopKernelStartupStatus::Starting;
        inner.active_attempt_token = Some(token);
        Ok((
            DesktopKernelStartAttempt { token, selection },
            replaced_owner,
            previous_selection,
        ))
    }

    fn attempt_is_active(&self, token: u64) -> bool {
        let inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(_) => return false,
        };
        !inner.closed
            && inner.status == DesktopKernelStartupStatus::Starting
            && inner.active_attempt_token == Some(token)
    }

    fn install_owner_for_attempt(
        &self,
        token: u64,
        owner: Arc<DesktopKernelOwner>,
    ) -> Result<Arc<DesktopKernelOwner>, Arc<DesktopKernelOwner>> {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(_) => return Err(owner),
        };
        if inner.closed
            || inner.status != DesktopKernelStartupStatus::Starting
            || inner.active_attempt_token != Some(token)
            || inner.owner.is_some()
        {
            return Err(owner);
        }
        inner.owner = Some(owner.clone());
        inner.owner_token = Some(token);
        inner.owner_publication_sequence = 0;
        Ok(owner)
    }

    async fn start_composed_owner_for_attempt(
        self: &Arc<Self>,
        token: u64,
        owner: Arc<DesktopKernelOwner>,
        launch: NativeKernelLaunch,
    ) -> Option<DesktopKernelStartupStatus> {
        let owner = match self.install_owner_for_attempt(token, owner) {
            Ok(active_owner) => active_owner,
            Err(stale_owner) => {
                drop_replaced_owner_off_runtime_thread(Some(stale_owner)).await;
                return None;
            }
        };
        let observed_sequence = owner.latest_publication_sequence();
        let _start_result = owner.start(launch).await;
        if owner.latest_publication_sequence() > observed_sequence {
            return None;
        }
        self.replace_status_for_attempt(token, DesktopKernelStartupStatus::Failed)
            .then_some(DesktopKernelStartupStatus::Failed)
    }

    pub(crate) fn take_owner_for_shutdown(&self) -> Option<Arc<DesktopKernelOwner>> {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        inner.closed = true;
        inner.active_attempt_token = None;
        inner.owner_token = None;
        inner.owner_publication_sequence = 0;
        inner.owner.take()
    }

    fn replace_status_for_owner_publication(
        &self,
        owner_token: u64,
        sequence: u64,
        phase: KernelHostPhase,
    ) -> Option<AcceptedKernelPublication> {
        let next_status = match phase {
            KernelHostPhase::Starting | KernelHostPhase::Retrying => {
                DesktopKernelStartupStatus::Starting
            }
            KernelHostPhase::Ready => DesktopKernelStartupStatus::Ready,
            KernelHostPhase::Dormant | KernelHostPhase::Stopping | KernelHostPhase::Failed => {
                DesktopKernelStartupStatus::Failed
            }
        };
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        if inner.closed
            || inner.owner.is_none()
            || inner.owner_token != Some(owner_token)
            || sequence == 0
            || sequence <= inner.owner_publication_sequence
        {
            return None;
        }
        inner.owner_publication_sequence = sequence;
        inner.mcp_authority_epoch = inner.mcp_authority_epoch.saturating_add(1);
        let status_changed = inner.status != next_status;
        if status_changed {
            inner.status = next_status;
            if matches!(phase, KernelHostPhase::Ready | KernelHostPhase::Failed) {
                inner.active_attempt_token = None;
            }
        }
        let workspace_root = matches!(phase, KernelHostPhase::Ready)
            .then(|| {
                inner
                    .selection
                    .as_ref()
                    .map(|selection| selection.workspace_root.clone())
            })
            .flatten();
        Some(AcceptedKernelPublication {
            mcp_authority_epoch: inner.mcp_authority_epoch,
            status_changed,
            workspace_root,
        })
    }

    fn advance_mcp_authority_epoch(&self) -> Option<u64> {
        let mut inner = self.inner.lock().ok()?;
        if inner.closed {
            return None;
        }
        inner.mcp_authority_epoch = inner.mcp_authority_epoch.saturating_add(1);
        Some(inner.mcp_authority_epoch)
    }

    #[cfg(test)]
    fn mcp_authority_epoch_is_current(&self, epoch: u64) -> bool {
        self.inner
            .lock()
            .is_ok_and(|inner| !inner.closed && inner.mcp_authority_epoch == epoch)
    }

    fn replace_status_for_attempt(&self, token: u64, status: DesktopKernelStartupStatus) -> bool {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        if inner.closed || inner.active_attempt_token != Some(token) {
            return false;
        }
        inner.status = status;
        inner.active_attempt_token = None;
        true
    }

    fn fail_workspace_switch(
        &self,
        app: &tauri::AppHandle,
        token: u64,
        selection: Option<DesktopKernelSelection>,
    ) {
        if self.replace_failed_workspace_switch(token, selection) {
            if let Some(epoch) = self.advance_mcp_authority_epoch() {
                schedule_mcp_workspace_refresh(app, epoch, None);
            }
            emit_startup_changed(app);
        }
    }

    fn replace_failed_workspace_switch(
        &self,
        token: u64,
        selection: Option<DesktopKernelSelection>,
    ) -> bool {
        match self.inner.lock() {
            Ok(mut inner)
                if !inner.closed
                    && inner.status == DesktopKernelStartupStatus::Starting
                    && inner.active_attempt_token == Some(token) =>
            {
                inner.owner = None;
                inner.owner_token = None;
                inner.owner_publication_sequence = 0;
                inner.selection = selection;
                inner.status = if inner.selection.is_some() {
                    DesktopKernelStartupStatus::Failed
                } else {
                    DesktopKernelStartupStatus::Unavailable
                };
                inner.active_attempt_token = None;
                true
            }
            _ => false,
        }
    }
}

fn reserve_attempt_token(inner: &mut DesktopKernelRuntimeInner) -> Result<u64, String> {
    let token = inner
        .next_attempt_token
        .checked_add(1)
        .ok_or_else(runtime_unavailable)?;
    inner.next_attempt_token = token;
    Ok(token)
}

async fn drop_replaced_owner_off_runtime_thread<Owner>(owner: Option<Owner>)
where
    Owner: Send + 'static,
{
    if let Some(owner) = owner {
        let _drop_result = run_off_runtime_thread(move || drop(owner)).await;
    }
}

async fn run_off_runtime_thread<Output, Task>(task: Task) -> Result<Output, String>
where
    Output: Send + 'static,
    Task: FnOnce() -> Output + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| runtime_unavailable())
}

struct TauriDesktopKernelEdgeEmitter {
    app: tauri::AppHandle,
    owner_token: u64,
    runtime: std::sync::Weak<DesktopKernelRuntimeState>,
}

impl DesktopKernelEdgeEmitter for TauriDesktopKernelEdgeEmitter {
    fn emit_kernel_state_changed(&self, sequence: u64, snapshot: KernelHostSnapshot) {
        if let Some(accepted) = self.runtime.upgrade().and_then(|runtime| {
            runtime.replace_status_for_owner_publication(self.owner_token, sequence, snapshot.phase)
        }) {
            schedule_mcp_workspace_refresh(
                &self.app,
                accepted.mcp_authority_epoch,
                accepted.workspace_root,
            );
            if accepted.status_changed {
                emit_startup_changed(&self.app);
            }
        }
        let _emit_result = self.app.emit(NATIVE_KERNEL_BOOTSTRAP_CHANGED_EVENT, ());
    }
}

fn schedule_mcp_workspace_refresh(
    app: &tauri::AppHandle,
    epoch: u64,
    workspace_root: Option<PathBuf>,
) {
    let app = app.clone();
    let Some(state) = app.try_state::<Arc<crate::mcp::McpState>>() else {
        return;
    };
    let previous_generation = match state.workspaces.publish_authority_epoch(epoch) {
        Ok(generation) => generation,
        Err(error) => {
            eprintln!("QingYu MCP authority publication failed: {error}");
            return;
        }
    };
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::mcp::refresh_kernel_workspace(
            &app,
            &state,
            workspace_root.as_deref(),
            epoch,
            previous_generation,
        )
        .await
        {
            eprintln!("QingYu MCP workspace refresh failed: {error}");
        }
    });
}

fn compose_owner_blocking(
    app: &tauri::AppHandle,
    workspace_root: PathBuf,
    renderer_origin: String,
    bootstrap: NativeKernelBootstrapOwner,
    runtime: std::sync::Weak<DesktopKernelRuntimeState>,
    owner_token: u64,
) -> Result<(Arc<DesktopKernelOwner>, NativeKernelLaunch), String> {
    let app_data_root = app
        .path()
        .app_data_dir()
        .map_err(|_| runtime_unavailable())?;
    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|_| runtime_unavailable())?;
    std::fs::create_dir_all(&app_data_root).map_err(|_| runtime_unavailable())?;
    std::fs::create_dir_all(&cache_root).map_err(|_| runtime_unavailable())?;

    let paths =
        qingyu_kernel::paths::KernelPaths::desktop(&workspace_root, &app_data_root, &cache_root)
            .map_err(|_| runtime_unavailable())?;
    let durable_operation_config =
        qingyu_kernel::config::KernelConfig::generate().map_err(|_| runtime_unavailable())?;
    let native_store = crate::primary_workspace::open_native_host_workspace_store(
        &paths,
        &durable_operation_config,
    )
    .map_err(|_| runtime_unavailable())?;
    let workspace_state = crate::primary_workspace::load_or_create_native_host_workspace_state(
        app,
        &workspace_root,
        &native_store,
    )?;

    let root = WorkspaceRootIdentity::open(&workspace_root).map_err(|_| runtime_unavailable())?;
    let authority = WriterAuthority::new(root.clone());
    let writer_gate =
        KernelWriterPublicationGate::new(authority, root).map_err(|_| runtime_unavailable())?;
    let factory = Arc::new(
        NativeKernelProcessFactory::for_current_application().map_err(|_| runtime_unavailable())?,
    );
    let endpoint_reader = bootstrap.endpoint_reader();
    let runtime_handle = tauri::async_runtime::handle().inner().clone();
    let supervisor = Arc::new(KernelHostSupervisor::new_with_bootstrap_on_handle(
        factory,
        KernelHostTimeouts::production(
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(25),
            Duration::from_secs(5),
        )
        .with_recovery(Duration::from_secs(1), Duration::from_secs(4), 3),
        bootstrap,
        writer_gate,
        &runtime_handle,
    ));
    let owner = Arc::new(DesktopKernelOwner::new_on_handle(
        supervisor,
        endpoint_reader,
        Arc::new(TauriDesktopKernelEdgeEmitter {
            app: app.clone(),
            owner_token,
            runtime,
        }),
        &runtime_handle,
    ));
    let launch = NativeKernelLaunch::desktop(
        workspace_root,
        app_data_root,
        cache_root,
        workspace_state,
        renderer_origin,
    )
    .map_err(|_| runtime_unavailable())?;
    Ok((owner, launch))
}

fn emit_startup_changed(app: &tauri::AppHandle) {
    let _emit_result = app.emit(DESKTOP_KERNEL_STARTUP_CHANGED_EVENT, ());
}

fn runtime_unavailable() -> String {
    "desktop Kernel runtime is unavailable".to_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        pin::Pin,
        sync::atomic::{AtomicUsize, Ordering},
        sync::mpsc,
        sync::Arc,
        thread::{self, ThreadId},
    };

    use crate::kernel_host::{
        desktop_kernel_owner::{DesktopKernelDriver, DesktopKernelEdgeEmitter},
        KernelHostFailure, KernelHostPhase, KernelHostPublicationReader,
        KernelHostPublicationSender, KernelHostPublicationSubscription, NativeKernelAccess,
    };

    use super::*;

    struct CountingStartDriver {
        publications: KernelHostPublicationReader,
        starts: AtomicUsize,
        stops: AtomicUsize,
        closes: AtomicUsize,
    }

    struct QueuedFailureDriver {
        publications: KernelHostPublicationSender,
        reader: KernelHostPublicationReader,
    }

    impl DesktopKernelDriver for CountingStartDriver {
        fn subscribe(&self) -> KernelHostPublicationSubscription {
            self.publications.subscribe()
        }

        fn latest_publication_sequence(&self) -> u64 {
            self.publications.latest_sequence()
        }

        fn start(
            &self,
            _launch: NativeKernelLaunch,
        ) -> Pin<Box<dyn Future<Output = Result<NativeKernelAccess, KernelHostFailure>> + Send + '_>>
        {
            Box::pin(async move {
                self.starts.fetch_add(1, Ordering::SeqCst);
                Err(KernelHostFailure::Spawn)
            })
        }

        fn stop(&self) -> Pin<Box<dyn Future<Output = Result<(), KernelHostFailure>> + Send + '_>> {
            Box::pin(async move {
                self.stops.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }

        fn close_fail_safe(&self) {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl DesktopKernelDriver for QueuedFailureDriver {
        fn subscribe(&self) -> KernelHostPublicationSubscription {
            self.reader.subscribe()
        }

        fn latest_publication_sequence(&self) -> u64 {
            self.reader.latest_sequence()
        }

        fn start(
            &self,
            _launch: NativeKernelLaunch,
        ) -> Pin<Box<dyn Future<Output = Result<NativeKernelAccess, KernelHostFailure>> + Send + '_>>
        {
            Box::pin(async move {
                self.publications
                    .publish_snapshot_for_test(KernelHostSnapshot {
                        phase: KernelHostPhase::Retrying,
                        generation: 1,
                        endpoint: None,
                        failure: Some(KernelHostFailure::Spawn),
                    });
                Err(KernelHostFailure::Spawn)
            })
        }

        fn stop(&self) -> Pin<Box<dyn Future<Output = Result<(), KernelHostFailure>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn close_fail_safe(&self) {}
    }

    struct NoopEmitter;

    impl DesktopKernelEdgeEmitter for NoopEmitter {
        fn emit_kernel_state_changed(&self, _sequence: u64, _snapshot: KernelHostSnapshot) {}
    }

    #[test]
    fn stable_runtime_snapshot_preserves_typed_dormant_failures() {
        for status in [
            DesktopKernelStartupStatus::Resolving,
            DesktopKernelStartupStatus::Unselected,
            DesktopKernelStartupStatus::Invalid,
            DesktopKernelStartupStatus::UnsupportedVersion,
            DesktopKernelStartupStatus::Unavailable,
        ] {
            let runtime = DesktopKernelRuntimeState::new(NativeKernelBootstrapOwner::new(), status);
            assert_eq!(runtime.snapshot().unwrap().status, status);
            assert!(runtime.take_owner_for_shutdown().is_none());
        }
    }

    #[test]
    fn failed_start_preserves_selection_for_an_explicit_fresh_owner_retry() {
        let runtime = DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Unselected,
        );
        let selection = DesktopKernelSelection {
            workspace_root: PathBuf::from("/tmp/qingyu-retry-workspace"),
            renderer_origin: "tauri://localhost".to_owned(),
        };

        let first = runtime
            .reserve_initial_start(selection.clone(), false)
            .unwrap();
        assert!(runtime.replace_status_for_attempt(first.token, DesktopKernelStartupStatus::Failed));
        let (retried, replaced_owner) = runtime.reserve_retry().unwrap();

        assert_eq!(retried.selection, selection);
        assert!(retried.token > first.token);
        assert!(replaced_owner.is_none());
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Starting
        );
        assert!(runtime.reserve_retry().is_err());
    }

    #[test]
    fn unsupported_workspace_versions_cannot_enter_selection_or_recovery() {
        let selection = DesktopKernelSelection {
            workspace_root: PathBuf::from("/tmp/qingyu-unsupported-workspace"),
            renderer_origin: "tauri://localhost".to_owned(),
        };
        let unsupported = DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::UnsupportedVersion,
        );
        assert!(unsupported
            .reserve_initial_start(selection.clone(), false)
            .is_err());
        assert!(!unsupported.is_invalid().unwrap());

        let resolving = DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Resolving,
        );
        assert!(resolving.reserve_initial_start(selection, true).is_ok());
    }

    #[test]
    fn stale_attempt_completion_cannot_install_owner_or_replace_retry_status() {
        let runtime = DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Unselected,
        );
        let selection = DesktopKernelSelection {
            workspace_root: PathBuf::from("/tmp/qingyu-stale-workspace"),
            renderer_origin: "tauri://localhost".to_owned(),
        };

        let first = runtime.reserve_initial_start(selection, false).unwrap();
        assert!(runtime.replace_status_for_attempt(first.token, DesktopKernelStartupStatus::Failed));
        let (retry, _replaced_owner) = runtime.reserve_retry().unwrap();

        assert!(!runtime.replace_status_for_attempt(first.token, DesktopKernelStartupStatus::Ready));
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Starting
        );
        assert!(runtime.attempt_is_active(retry.token));
    }

    #[test]
    fn shutdown_cancels_active_attempt_and_rejects_late_completion() {
        let runtime = DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Unselected,
        );
        let attempt = runtime
            .reserve_initial_start(
                DesktopKernelSelection {
                    workspace_root: PathBuf::from("/tmp/qingyu-closed-workspace"),
                    renderer_origin: "tauri://localhost".to_owned(),
                },
                false,
            )
            .unwrap();

        assert!(runtime.take_owner_for_shutdown().is_none());
        assert!(!runtime.attempt_is_active(attempt.token));
        assert!(
            !runtime.replace_status_for_attempt(attempt.token, DesktopKernelStartupStatus::Ready)
        );
        assert!(runtime.reserve_retry().is_err());
    }

    struct DropThreadProbe(mpsc::Sender<ThreadId>);

    impl Drop for DropThreadProbe {
        fn drop(&mut self) {
            let _send_result = self.0.send(thread::current().id());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn retry_replaced_owner_drop_runs_off_the_calling_thread() {
        let calling_thread = thread::current().id();
        let (dropped_tx, dropped_rx) = mpsc::channel();

        drop_replaced_owner_off_runtime_thread(Some(DropThreadProbe(dropped_tx))).await;

        assert_ne!(dropped_rx.recv().unwrap(), calling_thread);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_composer_runs_off_the_calling_thread() {
        let calling_thread = thread::current().id();

        let composer_thread = run_off_runtime_thread(|| thread::current().id())
            .await
            .unwrap();

        assert_ne!(composer_thread, calling_thread);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn only_the_active_attempt_can_start_a_composed_owner() {
        let runtime = Arc::new(DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Unselected,
        ));
        let selection = DesktopKernelSelection {
            workspace_root: std::env::temp_dir(),
            renderer_origin: "tauri://localhost".to_owned(),
        };
        let first = runtime.reserve_initial_start(selection, false).unwrap();
        assert!(runtime.replace_status_for_attempt(first.token, DesktopKernelStartupStatus::Failed));
        let (retry, _replaced_owner) = runtime.reserve_retry().unwrap();
        let stale_driver = test_driver();

        let stale_completion = runtime
            .start_composed_owner_for_attempt(
                first.token,
                test_owner(stale_driver.clone()),
                test_launch(),
            )
            .await;

        assert_eq!(stale_completion, None);
        assert_eq!(stale_driver.starts.load(Ordering::SeqCst), 0);
        assert_eq!(stale_driver.closes.load(Ordering::SeqCst), 1);

        let active_driver = test_driver();
        let active_completion = runtime
            .start_composed_owner_for_attempt(
                retry.token,
                test_owner(active_driver.clone()),
                test_launch(),
            )
            .await;

        assert_eq!(active_completion, Some(DesktopKernelStartupStatus::Failed));
        assert_eq!(active_driver.starts.load(Ordering::SeqCst), 1);
        let retained_owner = runtime.take_owner_for_shutdown();
        drop_replaced_owner_off_runtime_thread(retained_owner).await;
        assert_eq!(active_driver.closes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn installed_owner_publications_drive_authoritative_recovery_status() {
        let runtime = Arc::new(DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Unselected,
        ));
        let attempt = runtime
            .reserve_initial_start(
                DesktopKernelSelection {
                    workspace_root: std::env::temp_dir(),
                    renderer_origin: "tauri://localhost".to_owned(),
                },
                false,
            )
            .unwrap();
        let owner = test_owner(test_driver());
        assert!(runtime
            .install_owner_for_attempt(attempt.token, owner)
            .is_ok());

        let ready = runtime
            .replace_status_for_owner_publication(attempt.token, 1, KernelHostPhase::Ready)
            .unwrap();
        assert!(ready.status_changed);
        assert_eq!(ready.workspace_root, Some(std::env::temp_dir()));
        assert!(runtime.mcp_authority_epoch_is_current(ready.mcp_authority_epoch));
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Ready,
        );
        let retrying = runtime
            .replace_status_for_owner_publication(attempt.token, 2, KernelHostPhase::Retrying)
            .unwrap();
        assert!(retrying.status_changed);
        assert_eq!(retrying.workspace_root, None);
        assert!(!runtime.mcp_authority_epoch_is_current(ready.mcp_authority_epoch));
        assert!(runtime.mcp_authority_epoch_is_current(retrying.mcp_authority_epoch));
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Starting,
        );
        let failed = runtime
            .replace_status_for_owner_publication(attempt.token, 3, KernelHostPhase::Failed)
            .unwrap();
        assert!(failed.status_changed);
        assert_eq!(failed.workspace_root, None);
        assert!(!runtime.mcp_authority_epoch_is_current(retrying.mcp_authority_epoch));
        assert!(runtime.mcp_authority_epoch_is_current(failed.mcp_authority_epoch));
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Failed,
        );

        let (retry, replaced_owner) = runtime.reserve_retry().unwrap();
        assert!(replaced_owner.is_some());
        assert!(runtime
            .replace_status_for_owner_publication(attempt.token, 4, KernelHostPhase::Ready)
            .is_none());
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Starting,
        );
        assert!(runtime.attempt_is_active(retry.token));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ready_owner_is_detached_and_drained_before_one_workspace_switch_attempt() {
        let runtime = Arc::new(DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Unselected,
        ));
        let first_selection = DesktopKernelSelection {
            workspace_root: PathBuf::from("/tmp/qingyu-workspace-a"),
            renderer_origin: "tauri://localhost".to_owned(),
        };
        let first = runtime
            .reserve_initial_start(first_selection, false)
            .unwrap();
        let first_driver = test_driver();
        let first_owner = test_owner(first_driver.clone());
        assert!(runtime
            .install_owner_for_attempt(first.token, first_owner)
            .is_ok());
        runtime
            .replace_status_for_owner_publication(first.token, 1, KernelHostPhase::Ready)
            .unwrap();

        let second_selection = DesktopKernelSelection {
            workspace_root: PathBuf::from("/tmp/qingyu-workspace-b"),
            renderer_origin: "tauri://localhost".to_owned(),
        };
        let (switch, replaced_owner, previous_selection) =
            runtime.reserve_switch(second_selection.clone()).unwrap();

        assert_eq!(switch.selection, second_selection);
        assert_eq!(
            previous_selection.workspace_root,
            PathBuf::from("/tmp/qingyu-workspace-a")
        );
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Starting
        );
        assert!(runtime.reserve_switch(second_selection).is_err());
        assert!(runtime
            .replace_status_for_owner_publication(first.token, 2, KernelHostPhase::Ready)
            .is_none());

        replaced_owner.unwrap().stop().await.unwrap();
        assert_eq!(first_driver.stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failed_switch_reanchors_retry_to_the_authoritative_workspace_without_an_owner() {
        let runtime = DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Unselected,
        );
        let first_selection = DesktopKernelSelection {
            workspace_root: PathBuf::from("/tmp/qingyu-workspace-a"),
            renderer_origin: "tauri://localhost".to_owned(),
        };
        let first = runtime
            .reserve_initial_start(first_selection.clone(), false)
            .unwrap();
        let first_owner = test_owner(test_driver());
        assert!(runtime
            .install_owner_for_attempt(first.token, first_owner)
            .is_ok());
        runtime
            .replace_status_for_owner_publication(first.token, 1, KernelHostPhase::Ready)
            .unwrap();
        let second_selection = DesktopKernelSelection {
            workspace_root: PathBuf::from("/tmp/qingyu-workspace-b"),
            renderer_origin: "tauri://localhost".to_owned(),
        };
        let (switch, _owner, previous_selection) =
            runtime.reserve_switch(second_selection).unwrap();

        assert!(
            runtime.replace_failed_workspace_switch(switch.token, Some(previous_selection.clone()))
        );
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Failed
        );
        let (retry, replaced_owner) = runtime.reserve_retry().unwrap();
        assert_eq!(retry.selection, previous_selection);
        assert!(replaced_owner.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queued_owner_publication_wins_over_start_completion() {
        let runtime = Arc::new(DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Unselected,
        ));
        let attempt = runtime
            .reserve_initial_start(
                DesktopKernelSelection {
                    workspace_root: std::env::temp_dir(),
                    renderer_origin: "tauri://localhost".to_owned(),
                },
                false,
            )
            .unwrap();
        let (publications, _snapshots, reader) = KernelHostPublicationSender::new();
        let owner = Arc::new(DesktopKernelOwner::new_on_handle(
            Arc::new(QueuedFailureDriver {
                publications,
                reader,
            }),
            runtime.endpoint_reader(),
            Arc::new(NoopEmitter),
            &tokio::runtime::Handle::current(),
        ));

        let completion = runtime
            .start_composed_owner_for_attempt(attempt.token, owner, test_launch())
            .await;

        assert_eq!(completion, None);
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Starting,
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn closed_runtime_rejects_composed_owner_without_starting_child() {
        let runtime = Arc::new(DesktopKernelRuntimeState::new(
            NativeKernelBootstrapOwner::new(),
            DesktopKernelStartupStatus::Unselected,
        ));
        let attempt = runtime
            .reserve_initial_start(
                DesktopKernelSelection {
                    workspace_root: std::env::temp_dir(),
                    renderer_origin: "tauri://localhost".to_owned(),
                },
                false,
            )
            .unwrap();
        assert!(runtime.take_owner_for_shutdown().is_none());
        let driver = test_driver();

        let completion = runtime
            .start_composed_owner_for_attempt(
                attempt.token,
                test_owner(driver.clone()),
                test_launch(),
            )
            .await;

        assert_eq!(completion, None);
        assert_eq!(driver.starts.load(Ordering::SeqCst), 0);
        assert_eq!(driver.closes.load(Ordering::SeqCst), 1);
    }

    fn test_driver() -> Arc<CountingStartDriver> {
        let (_publications, _snapshots, reader) = KernelHostPublicationSender::new();
        Arc::new(CountingStartDriver {
            publications: reader,
            starts: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            closes: AtomicUsize::new(0),
        })
    }

    fn test_owner(driver: Arc<CountingStartDriver>) -> Arc<DesktopKernelOwner> {
        let bootstrap = NativeKernelBootstrapOwner::new();
        Arc::new(DesktopKernelOwner::new_on_handle(
            driver,
            bootstrap.endpoint_reader(),
            Arc::new(NoopEmitter),
            &tokio::runtime::Handle::current(),
        ))
    }

    fn test_launch() -> NativeKernelLaunch {
        let workspace_root = std::env::temp_dir();
        NativeKernelLaunch::desktop(
            workspace_root.clone(),
            std::env::temp_dir().join("qingyu-runtime-test-app-data"),
            std::env::temp_dir().join("qingyu-runtime-test-cache"),
            qingyu_kernel::host::native::NativeHostWorkspaceState::for_workspace(
                &workspace_root,
                "Workspace",
            )
            .unwrap(),
            "tauri://localhost".to_owned(),
        )
        .unwrap()
    }
}
