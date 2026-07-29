use std::{
    io::BufReader,
    process::ExitCode,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    composition::compose_fixed_native_kernel,
    config::KernelConfig,
    host::native::{NativeHostControl, NativeHostReady, NativeHostStart},
    paths::KernelPaths,
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
    let (workspace_root, app_data_root, cache_root, workspace_state, origin, credential) =
        startup.into_parts();
    let paths =
        KernelPaths::desktop(&workspace_root, &app_data_root, &cache_root).map_err(|_| ())?;
    let config =
        KernelConfig::generate_with_native_launch_credential(credential).map_err(|_| ())?;
    let runtime = compose_fixed_native_kernel(config, paths, workspace_state)
        .await
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
