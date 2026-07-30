use std::{
    ffi::OsString,
    future::IntoFuture,
    io::BufReader,
    net::SocketAddr,
    process::ExitCode,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use qingyu_kernel::{
    api::{build_router, build_server_web_router, TransportPolicy},
    composition::compose_fixed_native_kernel,
    config::KernelConfig,
    host::native::{NativeHostControl, NativeHostReady, NativeHostStart},
    paths::KernelPaths,
    server::{compose_fixed_server_kernel, ServerLaunchEnvironment},
};

const SERVER_LISTEN_ADDRESS: &str = "0.0.0.0:3210";
const SERVER_WEB_ROOT: &str = "/opt/qingyu/web";
const SERVER_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(30);

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
    let router = build_server_web_router(activation, policy, SERVER_WEB_ROOT).map_err(|_| ())?;
    let listener = tokio::net::TcpListener::bind(SERVER_LISTEN_ADDRESS)
        .await
        .map_err(|_| ())?;
    let (shutdown_started_sender, shutdown_started_receiver) = tokio::sync::oneshot::channel();
    let serve = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        server_shutdown_signal().await;
        let _send_result = shutdown_started_sender.send(());
    });

    await_bounded_server_shutdown(serve, shutdown_started_receiver, SERVER_SHUTDOWN_DEADLINE)
        .await
        .map(|_outcome| ())
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
            let parsed = reqwest::Url::parse(&public_origin).map_err(|_| ())?;
            if parsed.scheme() != "https"
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.path() != "/"
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(());
            }
            let canonical_origin = parsed.origin().ascii_serialization();
            if public_origin != canonical_origin {
                return Err(());
            }
            let exact_host = canonical_origin
                .strip_prefix("https://")
                .filter(|authority| !authority.is_empty())
                .ok_or(())?
                .to_owned();
            TransportPolicy::same_origin(&exact_host, &public_origin).map_err(|_| ())?;
            Ok(KernelCommand::Server {
                public_origin,
                exact_host,
            })
        }
        _ => Err(()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerShutdownOutcome {
    Drained,
    DeadlineElapsed,
}

async fn await_bounded_server_shutdown<Serve>(
    serve: Serve,
    mut shutdown_started: tokio::sync::oneshot::Receiver<()>,
    deadline: Duration,
) -> Result<ServerShutdownOutcome, ()>
where
    Serve: IntoFuture<Output = std::io::Result<()>>,
{
    let serve = serve.into_future();
    tokio::pin!(serve);
    tokio::select! {
        result = &mut serve => result.map(|()| ServerShutdownOutcome::Drained).map_err(|_| ()),
        started = &mut shutdown_started => {
            started.map_err(|_| ())?;
            match tokio::time::timeout(deadline, &mut serve).await {
                Ok(result) => result
                    .map(|()| ServerShutdownOutcome::Drained)
                    .map_err(|_| ()),
                // Returning drops the Axum serve future. The process-level Tokio runtime then
                // aborts any connection tasks that did not drain before the fixed deadline.
                Err(_elapsed) => Ok(ServerShutdownOutcome::DeadlineElapsed),
            }
        }
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

    fn server_command(public_origin: &str) -> Result<KernelCommand, ()> {
        parse_command(["qingyu-kernel", "server", "--public-origin", public_origin])
    }

    #[test]
    fn server_command_requires_one_exact_https_public_origin() {
        for (public_origin, exact_host) in [
            ("https://notes.example.com", "notes.example.com"),
            ("https://notes.example.com:8443", "notes.example.com:8443"),
            ("https://192.0.2.1", "192.0.2.1"),
            ("https://[2001:db8::1]", "[2001:db8::1]"),
            ("https://xn--bcher-kva.example", "xn--bcher-kva.example"),
        ] {
            assert_eq!(
                server_command(public_origin).unwrap(),
                KernelCommand::Server {
                    public_origin: public_origin.to_owned(),
                    exact_host: exact_host.to_owned(),
                }
            );
        }

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
                "HTTPS://notes.example.com",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://user@notes.example.com",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://user:password@notes.example.com",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://notes.example.com:08443",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://notes.example.com:65536",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://127.1",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://127.000.000.001",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://[2001:0db8:0:0:0:0:0:1]",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://[0:0:0:0:0:0:0:1]",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://bücher.example",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://notes.example.com?mode=server",
            ],
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://notes.example.com#server",
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
            vec![
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://notes.example.com",
                "--web-root",
                "/tmp/web",
            ],
        ] {
            assert!(
                parse_command(invalid.clone()).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[tokio::test]
    async fn bounded_shutdown_stops_waiting_for_a_slow_axum_request_after_the_deadline() {
        let (request_started, shutdown_started, release_request, server) =
            slow_test_server(std::time::Duration::from_millis(40)).await;
        let client = tokio::spawn(open_slow_request(server.address));
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            request_started.notified(),
        )
        .await
        .expect("slow request should reach its handler");

        let started_at = tokio::time::Instant::now();
        server.shutdown.send(()).unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            shutdown_started.notified(),
        )
        .await
        .expect("graceful shutdown should start");
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(2), server.task)
                .await
                .expect("bounded shutdown should return")
                .unwrap()
                .unwrap(),
            ServerShutdownOutcome::DeadlineElapsed
        );
        assert!(started_at.elapsed() < std::time::Duration::from_secs(1));

        release_request.notify_waiters();
        client.abort();
    }

    #[tokio::test]
    async fn bounded_shutdown_returns_as_soon_as_active_requests_drain() {
        let deadline = std::time::Duration::from_secs(5);
        let (request_started, shutdown_started, release_request, server) =
            slow_test_server(deadline).await;
        let client = tokio::spawn(open_slow_request(server.address));
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            request_started.notified(),
        )
        .await
        .expect("slow request should reach its handler");

        let started_at = tokio::time::Instant::now();
        server.shutdown.send(()).unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            shutdown_started.notified(),
        )
        .await
        .expect("graceful shutdown should start");
        release_request.notify_waiters();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(2), server.task)
                .await
                .expect("drained shutdown should return")
                .unwrap()
                .unwrap(),
            ServerShutdownOutcome::Drained
        );
        assert!(started_at.elapsed() < deadline);

        client.abort();
    }

    struct SlowTestServer {
        address: SocketAddr,
        shutdown: tokio::sync::oneshot::Sender<()>,
        task: tokio::task::JoinHandle<Result<ServerShutdownOutcome, ()>>,
    }

    async fn slow_test_server(
        deadline: std::time::Duration,
    ) -> (
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
        SlowTestServer,
    ) {
        use axum::{routing::get, Router};

        let request_started = Arc::new(tokio::sync::Notify::new());
        let release_request = Arc::new(tokio::sync::Notify::new());
        let request_started_in_handler = Arc::clone(&request_started);
        let release_request_in_handler = Arc::clone(&release_request);
        let router = Router::new().route(
            "/slow",
            get(move || {
                let request_started = Arc::clone(&request_started_in_handler);
                let release_request = Arc::clone(&release_request_in_handler);
                async move {
                    request_started.notify_one();
                    release_request.notified().await;
                    "done"
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (shutdown, shutdown_receiver) = tokio::sync::oneshot::channel();
        let (shutdown_started_sender, shutdown_started_receiver) = tokio::sync::oneshot::channel();
        let shutdown_started = Arc::new(tokio::sync::Notify::new());
        let shutdown_started_in_server = Arc::clone(&shutdown_started);
        let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
            shutdown_receiver.await.unwrap();
            shutdown_started_in_server.notify_one();
            shutdown_started_sender.send(()).unwrap();
        });
        let task = tokio::spawn(await_bounded_server_shutdown(
            serve,
            shutdown_started_receiver,
            deadline,
        ));
        (
            request_started,
            shutdown_started,
            release_request,
            SlowTestServer {
                address,
                shutdown,
                task,
            },
        )
    }

    async fn open_slow_request(address: SocketAddr) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
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
