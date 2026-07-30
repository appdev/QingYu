use std::{
    ffi::OsString,
    io::BufReader,
    net::SocketAddr,
    process::ExitCode,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use qingyu_kernel::{
    api::{build_router, build_server_router, TransportPolicy},
    composition::compose_fixed_native_kernel,
    config::KernelConfig,
    host::native::{NativeHostControl, NativeHostReady, NativeHostStart},
    paths::KernelPaths,
    server::{compose_fixed_server_kernel, ServerLaunchEnvironment},
};

const SERVER_LISTEN_ADDRESS: &str = "0.0.0.0:3210";

#[derive(Debug, Eq, PartialEq)]
enum KernelCommand {
    NativeServe,
    Server {
        public_origin: String,
        exact_host: String,
    },
}

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
    match parse_command(std::env::args_os())? {
        KernelCommand::NativeServe => run_native_server().await,
        KernelCommand::Server {
            public_origin,
            exact_host,
        } => run_fixed_server(public_origin, exact_host).await,
    }
}

async fn run_native_server() -> Result<(), ()> {
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

async fn run_fixed_server(public_origin: String, exact_host: String) -> Result<(), ()> {
    let policy = TransportPolicy::same_origin(&exact_host, &public_origin).map_err(|_| ())?;
    let environment = ServerLaunchEnvironment::load().map_err(|_| ())?;
    let paths = environment.layout().activate().map_err(|_| ())?;
    let composition = compose_fixed_server_kernel(KernelConfig::generate().map_err(|_| ())?, paths)
        .await
        .map_err(|_| ())?;
    let activation = composition.activate_api(environment).map_err(|_| ())?;
    let router = build_server_router(activation, policy);
    let listener = tokio::net::TcpListener::bind(SERVER_LISTEN_ADDRESS)
        .await
        .map_err(|_| ())?;

    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(server_shutdown_signal())
    .await
    .map_err(|_| ())
}

fn parse_command<Arguments, Argument>(args: Arguments) -> Result<KernelCommand, ()>
where
    Arguments: IntoIterator<Item = Argument>,
    Argument: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let _executable = args.next().ok_or(())?;
    let command = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or(())?;
    match command.as_str() {
        "serve" if args.next().is_none() => Ok(KernelCommand::NativeServe),
        "server" => {
            if args.next().as_deref() != Some(std::ffi::OsStr::new("--public-origin")) {
                return Err(());
            }
            let public_origin = args
                .next()
                .and_then(|value| value.into_string().ok())
                .ok_or(())?;
            if args.next().is_some() {
                return Err(());
            }
            let uri = public_origin.parse::<axum::http::Uri>().map_err(|_| ())?;
            let authority = uri.authority().ok_or(())?;
            let exact_host = authority.to_string();
            if public_origin != format!("https://{exact_host}")
                || exact_host != exact_host.to_ascii_lowercase()
                || authority.port_u16() == Some(443)
            {
                return Err(());
            }
            TransportPolicy::same_origin(&exact_host, &public_origin).map_err(|_| ())?;
            Ok(KernelCommand::Server {
                public_origin,
                exact_host,
            })
        }
        _ => Err(()),
    }
}

async fn server_shutdown_signal() {
    #[cfg(unix)]
    {
        let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        if let Ok(mut terminate) = terminate {
            tokio::select! {
                _interrupt = tokio::signal::ctrl_c() => {}
                _terminate = terminate.recv() => {}
            }
            return;
        }
    }
    let _interrupt = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_command_requires_one_exact_https_public_origin() {
        let parsed = parse_command([
            "qingyu-kernel",
            "server",
            "--public-origin",
            "https://notes.example.com:8443",
        ])
        .unwrap();
        assert_eq!(
            parsed,
            KernelCommand::Server {
                public_origin: "https://notes.example.com:8443".to_owned(),
                exact_host: "notes.example.com:8443".to_owned(),
            }
        );

        for invalid in [
            vec!["qingyu-kernel", "server"],
            vec!["qingyu-kernel", "server", "--public-origin"],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "http://notes.example.com",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://notes.example.com/path",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://notes.example.com/",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://notes.example.com:443",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://NOTES.example.com",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://notes.example.com",
                "--public-origin",
                "https://other.example.com",
            ],
            vec!["qingyu-kernel", "server", "--bind", "127.0.0.1:0"],
        ] {
            assert!(
                parse_command(invalid.clone()).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn native_serve_command_remains_distinct_from_server_options() {
        assert_eq!(
            parse_command(["qingyu-kernel", "serve"]).unwrap(),
            KernelCommand::NativeServe
        );
        assert!(parse_command([
            "qingyu-kernel",
            "serve",
            "--public-origin",
            "https://notes.example.com",
        ])
        .is_err());
    }
}
