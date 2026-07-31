use std::{
    fmt,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::Duration,
};

use qingyu_kernel::host::mobile::{
    MobileKernelEndpoint, MobileKernelHostError, MobileKernelHostOwner, MobileKernelLaunch,
};
use serde::Serializer;
use tauri::{Emitter as _, Manager as _};

const MOBILE_KERNEL_BOOTSTRAP_VERSION: u16 = 1;
const TERMINAL_EXIT_IDLE: u8 = 0;
const TERMINAL_EXIT_STOPPING: u8 = 1;
const TERMINAL_EXIT_READY: u8 = 2;
pub(crate) const MOBILE_KERNEL_BOOTSTRAP_CHANGED_EVENT: &str = "qingyu://kernel-bootstrap-changed";

pub(crate) struct MobileKernelRuntimeState {
    operation: tokio::sync::Mutex<()>,
    origin: String,
    owner: Arc<MobileKernelHostOwner>,
    phase: Mutex<MobileKernelRuntimePhase>,
    terminal_exit: AtomicU8,
}

enum MobileKernelRuntimePhase {
    Starting { generation: u64 },
    Ready { endpoint: Arc<MobileKernelEndpoint> },
    Stopping { generation: u64 },
    Dormant { generation: u64 },
    Failed { generation: u64 },
}

#[derive(serde::Serialize)]
#[serde(untagged)]
pub(crate) enum MobileKernelBootstrap {
    Lifecycle(MobileKernelLifecycleBootstrap),
    Ready(MobileKernelReadyBootstrap),
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MobileKernelLifecycleBootstrap {
    bootstrap_version: u16,
    generation: String,
    status: MobileKernelBootstrapStatus,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MobileKernelReadyBootstrap {
    bootstrap_version: u16,
    credential: MobileKernelBootstrapCredential,
    generation: String,
    instance_id: qingyu_kernel::contract::InstanceId,
    port: u16,
    status: MobileKernelBootstrapStatus,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum MobileKernelBootstrapStatus {
    Starting,
    Ready,
    Dormant,
    Failed,
}

struct MobileKernelBootstrapCredential(Arc<MobileKernelEndpoint>);

impl serde::Serialize for MobileKernelBootstrapCredential {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(
            self.0
                .bearer()
                .map_err(|_| serde::ser::Error::custom("mobile Kernel credential unavailable"))?,
        )
    }
}

impl fmt::Debug for MobileKernelBootstrapCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl MobileKernelRuntimeState {
    pub(crate) fn new(
        drain_timeout: Duration,
        origin: impl Into<String>,
    ) -> Result<Arc<Self>, MobileKernelRuntimeError> {
        let origin = origin.into();
        if origin.is_empty() {
            return Err(MobileKernelRuntimeError);
        }
        let owner = MobileKernelHostOwner::new(drain_timeout).map_err(safe_host_error)?;
        Ok(Arc::new(Self {
            operation: tokio::sync::Mutex::new(()),
            origin,
            owner: Arc::new(owner),
            phase: Mutex::new(MobileKernelRuntimePhase::Starting { generation: 1 }),
            terminal_exit: AtomicU8::new(TERMINAL_EXIT_IDLE),
        }))
    }

    pub(crate) async fn start(
        &self,
        launch: MobileKernelLaunch,
        origin: &str,
    ) -> Result<(), MobileKernelRuntimeError> {
        self.verify_origin(origin)?;
        let _operation = self.operation.lock().await;
        let generation = match &*self.lock_phase()? {
            MobileKernelRuntimePhase::Starting { generation } => *generation,
            _ => return Err(MobileKernelRuntimeError),
        };
        let endpoint = match self.owner.start(launch, &self.origin).await {
            Ok(endpoint) if endpoint.generation() == generation => endpoint,
            Ok(_) => {
                *self.lock_phase()? = MobileKernelRuntimePhase::Failed { generation };
                let _stopped = self.owner.stop().await;
                return Err(MobileKernelRuntimeError);
            }
            Err(error) => {
                *self.lock_phase()? = MobileKernelRuntimePhase::Failed { generation };
                return Err(safe_host_error(error));
            }
        };
        *self.lock_phase()? = MobileKernelRuntimePhase::Ready {
            endpoint: Arc::new(endpoint),
        };
        Ok(())
    }

    pub(crate) fn read_bootstrap(
        &self,
        origin: &str,
    ) -> Result<MobileKernelBootstrap, MobileKernelRuntimeError> {
        self.verify_origin(origin)?;
        let phase = self.lock_phase()?;
        Ok(match &*phase {
            MobileKernelRuntimePhase::Starting { generation } => {
                lifecycle_bootstrap(MobileKernelBootstrapStatus::Starting, *generation)
            }
            MobileKernelRuntimePhase::Ready { endpoint } => {
                MobileKernelBootstrap::Ready(MobileKernelReadyBootstrap {
                    bootstrap_version: MOBILE_KERNEL_BOOTSTRAP_VERSION,
                    credential: MobileKernelBootstrapCredential(endpoint.clone()),
                    generation: endpoint.generation().to_string(),
                    instance_id: endpoint.instance_id(),
                    port: endpoint.address().port(),
                    status: MobileKernelBootstrapStatus::Ready,
                })
            }
            MobileKernelRuntimePhase::Stopping { generation }
            | MobileKernelRuntimePhase::Dormant { generation } => {
                lifecycle_bootstrap(MobileKernelBootstrapStatus::Dormant, *generation)
            }
            MobileKernelRuntimePhase::Failed { generation } => {
                lifecycle_bootstrap(MobileKernelBootstrapStatus::Failed, *generation)
            }
        })
    }

    pub(crate) async fn stop(&self) -> Result<(), MobileKernelRuntimeError> {
        let _operation = self.operation.lock().await;
        let generation = {
            let mut phase = self.lock_phase()?;
            let generation = match &*phase {
                MobileKernelRuntimePhase::Starting { generation }
                | MobileKernelRuntimePhase::Stopping { generation }
                | MobileKernelRuntimePhase::Dormant { generation }
                | MobileKernelRuntimePhase::Failed { generation } => *generation,
                MobileKernelRuntimePhase::Ready { endpoint } => endpoint.generation(),
            };
            if matches!(&*phase, MobileKernelRuntimePhase::Dormant { .. }) {
                return Ok(());
            }
            *phase = MobileKernelRuntimePhase::Stopping { generation };
            generation
        };
        match self.owner.stop().await {
            Ok(_) => {
                *self.lock_phase()? = MobileKernelRuntimePhase::Dormant { generation };
                Ok(())
            }
            Err(error) => {
                *self.lock_phase()? = MobileKernelRuntimePhase::Failed { generation };
                Err(safe_host_error(error))
            }
        }
    }

    fn fail_start(&self) -> Result<(), MobileKernelRuntimeError> {
        let mut phase = self.lock_phase()?;
        let generation = match &*phase {
            MobileKernelRuntimePhase::Starting { generation } => *generation,
            _ => return Err(MobileKernelRuntimeError),
        };
        *phase = MobileKernelRuntimePhase::Failed { generation };
        Ok(())
    }

    fn configured_origin(&self) -> &str {
        &self.origin
    }

    pub(crate) fn begin_terminal_exit(&self) -> bool {
        self.terminal_exit
            .compare_exchange(
                TERMINAL_EXIT_IDLE,
                TERMINAL_EXIT_STOPPING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    pub(crate) fn mark_terminal_exit_ready(&self) {
        self.terminal_exit
            .store(TERMINAL_EXIT_READY, Ordering::SeqCst);
    }

    pub(crate) fn terminal_exit_is_ready(&self) -> bool {
        self.terminal_exit.load(Ordering::SeqCst) == TERMINAL_EXIT_READY
    }

    fn verify_origin(&self, origin: &str) -> Result<(), MobileKernelRuntimeError> {
        if origin == self.origin {
            Ok(())
        } else {
            Err(MobileKernelRuntimeError)
        }
    }

    fn lock_phase(
        &self,
    ) -> Result<MutexGuard<'_, MobileKernelRuntimePhase>, MobileKernelRuntimeError> {
        self.phase.lock().map_err(|_| MobileKernelRuntimeError)
    }
}

fn lifecycle_bootstrap(
    status: MobileKernelBootstrapStatus,
    generation: u64,
) -> MobileKernelBootstrap {
    MobileKernelBootstrap::Lifecycle(MobileKernelLifecycleBootstrap {
        bootstrap_version: MOBILE_KERNEL_BOOTSTRAP_VERSION,
        generation: generation.to_string(),
        status,
    })
}

fn safe_host_error(_error: MobileKernelHostError) -> MobileKernelRuntimeError {
    MobileKernelRuntimeError
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MobileKernelRuntimeError;

impl fmt::Display for MobileKernelRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("mobile Kernel runtime is unavailable")
    }
}

impl std::error::Error for MobileKernelRuntimeError {}

pub(crate) fn configured_mobile_renderer_origin(
    development: bool,
    dev_url: Option<&tauri::Url>,
    android: bool,
) -> Result<String, MobileKernelRuntimeError> {
    if development {
        return dev_url
            .ok_or(MobileKernelRuntimeError)
            .and_then(mobile_renderer_origin);
    }
    if android {
        Ok("http://tauri.localhost".to_owned())
    } else {
        Ok("tauri://localhost".to_owned())
    }
}

fn mobile_renderer_origin(url: &tauri::Url) -> Result<String, MobileKernelRuntimeError> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(MobileKernelRuntimeError);
    }
    match url.scheme() {
        "http" | "https" => {
            let origin = url.origin().ascii_serialization();
            (origin != "null")
                .then_some(origin)
                .ok_or(MobileKernelRuntimeError)
        }
        "tauri" if !url.authority().is_empty() => Ok(format!("tauri://{}", url.authority())),
        _ => Err(MobileKernelRuntimeError),
    }
}

pub(crate) fn validated_mobile_renderer_origin(
    caller_label: &str,
    configured_origin: &str,
    caller_url: &tauri::Url,
) -> Result<String, MobileKernelRuntimeError> {
    if caller_label != "main" || mobile_renderer_origin(caller_url)? != configured_origin {
        return Err(MobileKernelRuntimeError);
    }
    Ok(configured_origin.to_owned())
}

fn configured_main_mobile_renderer_origin(
    app: &tauri::AppHandle,
) -> Result<String, MobileKernelRuntimeError> {
    configured_mobile_renderer_origin(
        cfg!(dev),
        app.config().build.dev_url.as_ref(),
        cfg!(target_os = "android"),
    )
}

#[tauri::command]
pub(crate) fn read_mobile_kernel_bootstrap(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, Arc<MobileKernelRuntimeState>>,
) -> Result<MobileKernelBootstrap, String> {
    let url = window
        .url()
        .map_err(|_| MobileKernelRuntimeError.to_string())?;
    let origin =
        validated_mobile_renderer_origin(window.label(), runtime.configured_origin(), &url)
            .map_err(|error| error.to_string())?;
    runtime
        .read_bootstrap(&origin)
        .map_err(|error| error.to_string())
}

pub(crate) fn install_mobile_kernel_runtime(
    app: &mut tauri::App,
) -> Result<(), Box<dyn std::error::Error>> {
    let origin = configured_main_mobile_renderer_origin(&app.handle())?;
    let runtime = MobileKernelRuntimeState::new(Duration::from_secs(30), origin.clone())?;
    app.manage(runtime.clone());

    let app_handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let launch = compose_mobile_launch(&app_handle).await;
        let result = match launch {
            Ok(launch) => runtime.start(launch, &origin).await,
            Err(error) => {
                let _failed = runtime.fail_start();
                Err(error)
            }
        };
        if result.is_err() {
            let _failed = runtime.fail_start();
        }
        let _emitted = app_handle.emit(MOBILE_KERNEL_BOOTSTRAP_CHANGED_EVENT, ());
    });
    Ok(())
}

async fn compose_mobile_launch(
    app: &tauri::AppHandle,
) -> Result<MobileKernelLaunch, MobileKernelRuntimeError> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|_| MobileKernelRuntimeError)?;
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|_| MobileKernelRuntimeError)?;
    std::fs::create_dir_all(&app_data).map_err(|_| MobileKernelRuntimeError)?;
    std::fs::create_dir_all(&cache).map_err(|_| MobileKernelRuntimeError)?;
    let paths = qingyu_kernel::paths::KernelPaths::mobile(&app_data, &cache, "primary")
        .map_err(|_| MobileKernelRuntimeError)?;
    let config =
        qingyu_kernel::config::KernelConfig::generate().map_err(|_| MobileKernelRuntimeError)?;
    qingyu_kernel::composition::compose_fixed_mobile_kernel(config, paths, "QingYu")
        .await
        .map_err(|_| MobileKernelRuntimeError)
}
