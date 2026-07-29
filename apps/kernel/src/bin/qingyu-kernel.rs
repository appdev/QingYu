use std::{
    io::{Read as _, Write as _},
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
};

use async_trait::async_trait;
use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::{KernelConfig, NativeLaunchCredential},
    contract::{
        ApiVersion, HostProfile, InstanceId, ReadyHealthResponse, ReadyStatus,
        RuntimeCapabilitiesDto, RuntimeStateDto, StartupState, SystemVersionResponse,
    },
    paths::KernelPaths,
    ports::KernelPorts,
    runtime::{KernelRuntime, ServiceFailure, SystemApiService},
    settings::{service::SettingsService, storage::AtomicJsonSettingsStore},
    storage::DurableFileStore,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize as _;

const MAX_STARTUP_PAYLOAD_BYTES: u64 = 64 * 1024;

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

    let mut bytes = Vec::new();
    let read_result = std::io::stdin()
        .lock()
        .take(MAX_STARTUP_PAYLOAD_BYTES + 1)
        .read_to_end(&mut bytes);
    if read_result.is_err() {
        bytes.zeroize();
        return Err(());
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_STARTUP_PAYLOAD_BYTES {
        bytes.zeroize();
        return Err(());
    }
    let startup = serde_json::from_slice::<StartupInput>(&bytes);
    bytes.zeroize();
    let startup = startup.map_err(|_| ())?;

    let (paths, origin, credential) = match startup {
        StartupInput::Desktop {
            workspace_root,
            app_data_root,
            cache_root,
            origin,
            credential,
        } => (
            StartupPaths::Desktop {
                workspace_root,
                app_data_root,
                cache_root,
            },
            origin,
            credential,
        ),
        StartupInput::Server { origin, credential } => (StartupPaths::Server, origin, credential),
        StartupInput::Mobile {
            app_data_root,
            cache_root,
            managed_name,
            origin,
            credential,
        } => (
            StartupPaths::Mobile {
                app_data_root,
                cache_root,
                managed_name,
            },
            origin,
            credential,
        ),
    };
    let credential = credential.into_native()?;
    let config =
        KernelConfig::generate_with_native_launch_credential(credential).map_err(|_| ())?;
    let paths = match paths {
        StartupPaths::Desktop {
            workspace_root,
            app_data_root,
            cache_root,
        } => KernelPaths::desktop(&workspace_root, &app_data_root, &cache_root),
        StartupPaths::Server => KernelPaths::server().activate(),
        StartupPaths::Mobile {
            app_data_root,
            cache_root,
            managed_name,
        } => KernelPaths::mobile(&app_data_root, &cache_root, &managed_name),
    }
    .map_err(|_| ())?;
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

    let readiness = serde_json::to_vec(&ReadinessRecord {
        port: address.port(),
        instance_id: runtime.instance_id(),
    })
    .map_err(|_| ())?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&readiness).map_err(|_| ())?;
    stdout.write_all(b"\n").map_err(|_| ())?;
    stdout.flush().map_err(|_| ())?;
    drop(stdout);

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _signal = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| ())
}

#[derive(Deserialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "profile"
)]
enum StartupInput {
    Desktop {
        workspace_root: PathBuf,
        app_data_root: PathBuf,
        cache_root: PathBuf,
        origin: String,
        credential: CredentialInput,
    },
    Server {
        origin: String,
        credential: CredentialInput,
    },
    Mobile {
        app_data_root: PathBuf,
        cache_root: PathBuf,
        managed_name: String,
        origin: String,
        credential: CredentialInput,
    },
}

enum StartupPaths {
    Desktop {
        workspace_root: PathBuf,
        app_data_root: PathBuf,
        cache_root: PathBuf,
    },
    Server,
    Mobile {
        app_data_root: PathBuf,
        cache_root: PathBuf,
        managed_name: String,
    },
}

#[derive(Deserialize)]
#[serde(transparent)]
struct CredentialInput(String);

impl CredentialInput {
    fn into_native(mut self) -> Result<NativeLaunchCredential, ()> {
        NativeLaunchCredential::from_secret(std::mem::take(&mut self.0)).map_err(|_| ())
    }
}

impl Drop for CredentialInput {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadinessRecord {
    port: u16,
    instance_id: InstanceId,
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
