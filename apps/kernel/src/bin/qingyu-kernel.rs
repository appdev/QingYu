use std::{
    io::BufReader,
    process::ExitCode,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use async_trait::async_trait;
use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        ApiVersion, HostProfile, InstanceId, ReadyHealthResponse, ReadyStatus,
        RuntimeCapabilitiesDto, RuntimeStateDto, StartupState, SystemVersionResponse,
    },
    host::native::{NativeHostControl, NativeHostReady, NativeHostStart},
    paths::KernelPaths,
    ports::KernelPorts,
    runtime::{KernelRuntime, ServiceFailure, SystemApiService},
    settings::{service::SettingsService, storage::AtomicJsonSettingsStore},
    storage::DurableFileStore,
};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => {
            eprintln!("QingYu Kernel startup failed.");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), ()> {
    if std::env::args().nth(1).as_deref() != Some("serve") {
        return Err(());
    }

    let mut control_reader = BufReader::new(std::io::stdin());
    let startup = NativeHostStart::read_json_line(&mut control_reader).map_err(|_| ())?;
    let (workspace_root, app_data_root, cache_root, origin, credential) = startup.into_parts();
    let paths =
        KernelPaths::desktop(&workspace_root, &app_data_root, &cache_root).map_err(|_| ())?;
    let config =
        KernelConfig::generate_with_native_launch_credential(credential).map_err(|_| ())?;
    let profile = paths.profile();
    let settings_store = Arc::new(
        AtomicJsonSettingsStore::new(
            DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
                .map_err(|_| ())?,
        )
        .map_err(|_| ())?,
    );
    let runtime =
        KernelRuntime::activate(config, paths, KernelPorts::unavailable()).map_err(|_| ())?;
    let settings_service = Arc::new(SettingsService::new(
        settings_store,
        runtime.event_broker().clone(),
    ));
    settings_service.migrate_schema().map_err(|_| ())?;
    runtime
        .install_settings_api_service(settings_service)
        .map_err(|_| ())?;
    runtime
        .install_system_api_service(Arc::new(BasicSystemService {
            instance_id: runtime.instance_id(),
            profile,
        }))
        .map_err(|_| ())?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| ())?;
    let address = listener.local_addr().map_err(|_| ())?;
    let policy = TransportPolicy::loopback(&address.to_string(), &origin).map_err(|_| ())?;
    let router = build_router(runtime.clone(), policy);

    let (control_sender, control_receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("qingyu-kernel-control".to_owned())
        .spawn(move || {
            let signal = NativeHostControl::read_json_line(&mut control_reader);
            let _send_result = control_sender.send(signal);
        })
        .map_err(|_| ())?;

    let readiness = NativeHostReady::new(address.port(), runtime.instance_id());
    let mut stdout = std::io::stdout().lock();
    readiness.write_json_line(&mut stdout).map_err(|_| ())?;
    drop(stdout);

    let protocol_failed = Arc::new(AtomicBool::new(false));
    let protocol_failed_on_shutdown = Arc::clone(&protocol_failed);
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            tokio::select! {
                control = control_receiver => {
                    if !matches!(
                        control,
                        Ok(Ok(NativeHostControl::Shutdown | NativeHostControl::EndOfStream))
                    ) {
                        protocol_failed_on_shutdown.store(true, Ordering::Release);
                    }
                }
                _signal = tokio::signal::ctrl_c() => {}
            }
        })
        .await
        .map_err(|_| ())?;
    if protocol_failed.load(Ordering::Acquire) {
        Err(())
    } else {
        Ok(())
    }
}

struct BasicSystemService {
    instance_id: InstanceId,
    profile: HostProfile,
}

#[async_trait]
impl SystemApiService for BasicSystemService {
    async fn ready(&self) -> Result<ReadyHealthResponse, ServiceFailure> {
        Ok(ReadyHealthResponse {
            status: ReadyStatus::Ready,
            api_version: ApiVersion::V1,
            instance_id: self.instance_id,
        })
    }

    async fn version(&self) -> Result<SystemVersionResponse, ServiceFailure> {
        Ok(SystemVersionResponse {
            api_version: ApiVersion::V1,
            kernel_version: env!("CARGO_PKG_VERSION").to_owned(),
            instance_id: self.instance_id,
        })
    }

    async fn runtime_state(&self) -> Result<RuntimeStateDto, ServiceFailure> {
        Ok(RuntimeStateDto {
            profile: self.profile,
            startup_state: StartupState::Ready,
            capabilities: RuntimeCapabilitiesDto {
                documents: false,
                history: false,
                search: false,
                settings: true,
                sync: false,
                webdav: false,
                s3: false,
                portable_settings: true,
            },
            instance_id: self.instance_id,
        })
    }
}
