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
        KernelHostSupervisor, KernelHostTimeouts, NativeKernelLaunch,
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
    Unselected,
    Invalid,
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
    selection: Option<DesktopKernelSelection>,
    status: DesktopKernelStartupStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DesktopKernelSelection {
    workspace_root: PathBuf,
    renderer_origin: String,
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
                selection: None,
                status,
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
        self.reserve_initial_start(selection.clone())?;
        emit_startup_changed(app);
        self.launch_reserved(app, selection)
    }

    pub(crate) fn retry_selected(self: &Arc<Self>, app: &tauri::AppHandle) -> Result<(), String> {
        let (selection, replaced_owner) = self.reserve_retry()?;
        drop(replaced_owner);
        emit_startup_changed(app);
        self.launch_reserved(app, selection)
    }

    fn launch_reserved(
        self: &Arc<Self>,
        app: &tauri::AppHandle,
        selection: DesktopKernelSelection,
    ) -> Result<(), String> {
        let composed = compose_owner(
            app,
            selection.workspace_root,
            selection.renderer_origin,
            self.bootstrap.clone(),
        );
        let (owner, launch) = match composed {
            Ok(composed) => composed,
            Err(error) => {
                self.replace_status(DesktopKernelStartupStatus::Failed);
                emit_startup_changed(app);
                return Err(error);
            }
        };

        {
            let mut inner = self.inner.lock().map_err(|_| runtime_unavailable())?;
            if inner.closed || inner.owner.is_some() {
                return Err(runtime_unavailable());
            }
            inner.owner = Some(owner.clone());
        }

        let runtime = Arc::clone(self);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let next = match owner.start(launch).await {
                Ok(_access) => DesktopKernelStartupStatus::Ready,
                Err(_error) => DesktopKernelStartupStatus::Failed,
            };
            runtime.replace_status(next);
            emit_startup_changed(&app);
        });
        Ok(())
    }

    fn reserve_initial_start(&self, selection: DesktopKernelSelection) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| runtime_unavailable())?;
        if inner.closed
            || inner.owner.is_some()
            || inner.selection.is_some()
            || inner.status == DesktopKernelStartupStatus::Starting
        {
            return Err(runtime_unavailable());
        }
        inner.selection = Some(selection);
        inner.status = DesktopKernelStartupStatus::Starting;
        Ok(())
    }

    fn reserve_retry(
        &self,
    ) -> Result<(DesktopKernelSelection, Option<Arc<DesktopKernelOwner>>), String> {
        let mut inner = self.inner.lock().map_err(|_| runtime_unavailable())?;
        if inner.closed || inner.status != DesktopKernelStartupStatus::Failed {
            return Err(runtime_unavailable());
        }
        let selection = inner.selection.clone().ok_or_else(runtime_unavailable)?;
        let replaced_owner = inner.owner.take();
        inner.status = DesktopKernelStartupStatus::Starting;
        Ok((selection, replaced_owner))
    }

    pub(crate) fn take_owner_for_shutdown(&self) -> Option<Arc<DesktopKernelOwner>> {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        inner.closed = true;
        inner.owner.take()
    }

    fn replace_status(&self, status: DesktopKernelStartupStatus) {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !inner.closed {
            inner.status = status;
        }
    }
}

struct TauriDesktopKernelEdgeEmitter {
    app: tauri::AppHandle,
}

impl DesktopKernelEdgeEmitter for TauriDesktopKernelEdgeEmitter {
    fn emit_kernel_state_changed(&self) {
        let _emit_result = self.app.emit(NATIVE_KERNEL_BOOTSTRAP_CHANGED_EVENT, ());
    }
}

fn compose_owner(
    app: &tauri::AppHandle,
    workspace_root: PathBuf,
    renderer_origin: String,
    bootstrap: NativeKernelBootstrapOwner,
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
        KernelHostTimeouts::uniform(Duration::from_secs(30)).with_recovery(
            Duration::from_secs(1),
            Duration::from_secs(4),
            3,
        ),
        bootstrap,
        writer_gate,
        &runtime_handle,
    ));
    let owner = Arc::new(DesktopKernelOwner::new_on_handle(
        supervisor,
        endpoint_reader,
        Arc::new(TauriDesktopKernelEdgeEmitter { app: app.clone() }),
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
    use super::*;

    #[test]
    fn stable_runtime_snapshot_preserves_typed_dormant_failures() {
        for status in [
            DesktopKernelStartupStatus::Unselected,
            DesktopKernelStartupStatus::Invalid,
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
            DesktopKernelStartupStatus::Unavailable,
        );
        let selection = DesktopKernelSelection {
            workspace_root: PathBuf::from("/tmp/qingyu-retry-workspace"),
            renderer_origin: "tauri://localhost".to_owned(),
        };

        runtime.reserve_initial_start(selection.clone()).unwrap();
        runtime.replace_status(DesktopKernelStartupStatus::Failed);
        let (retried, replaced_owner) = runtime.reserve_retry().unwrap();

        assert_eq!(retried, selection);
        assert!(replaced_owner.is_none());
        assert_eq!(
            runtime.snapshot().unwrap().status,
            DesktopKernelStartupStatus::Starting
        );
        assert!(runtime.reserve_retry().is_err());
    }
}
