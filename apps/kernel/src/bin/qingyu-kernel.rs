use std::{
    ffi::OsString, fmt, future::IntoFuture, io::BufReader, net::SocketAddr, process::ExitCode,
    time::Duration,
};

use qingyu_kernel::{
    api::{build_router, build_server_web_router, TransportPolicy},
    composition::compose_fixed_native_kernel_runtime,
    config::KernelConfig,
    host::native::{NativeHostControl, NativeHostReady, NativeHostStart},
    paths::KernelPaths,
    server::{compose_fixed_server_kernel, ServerLaunchEnvironment, ServerRuntimeCompositionError},
};

const SERVER_LISTEN_ADDRESS: &str = "0.0.0.0:3210";
const SERVER_WEB_ROOT: &str = "/opt/qingyu/web";
const NATIVE_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(30);
const SERVER_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Debug, Eq, PartialEq)]
enum KernelCommand {
    NativeServe,
    Server {
        public_origin: String,
        exact_host: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KernelCommandError {
    Command,
    TransportPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KernelStartupError {
    Native,
    Command,
    Server(ServerStartupStage),
}

impl fmt::Display for KernelStartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native => formatter.write_str("QingYu Kernel startup failed."),
            Self::Command => formatter.write_str("QingYu Kernel startup failed [QK-CMD]."),
            Self::Server(stage) => write!(
                formatter,
                "QingYu Kernel startup failed [{}].",
                stage.diagnostic_code()
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerStartupStage {
    TransportPolicy,
    Environment,
    Paths,
    RuntimeConfig,
    Composition(ServerRuntimeCompositionError),
    AuthenticationApi,
    StaticRouter,
    Listener,
    Serve,
    KernelDrain,
    ShutdownDeadline,
}

impl ServerStartupStage {
    const fn diagnostic_code(self) -> &'static str {
        match self {
            Self::TransportPolicy => "QK-SRV-TRANSPORT",
            Self::Environment => "QK-SRV-ENV",
            Self::Paths => "QK-SRV-PATHS",
            Self::RuntimeConfig => "QK-SRV-CONFIG",
            Self::Composition(error) => error.diagnostic_code(),
            Self::AuthenticationApi => "QK-SRV-AUTH-API",
            Self::StaticRouter => "QK-SRV-STATIC-ROUTER",
            Self::Listener => "QK-SRV-LISTENER",
            Self::Serve => "QK-SRV-SERVE",
            Self::KernelDrain => "QK-SRV-KERNEL-DRAIN",
            Self::ShutdownDeadline => "QK-SRV-SHUTDOWN-DEADLINE",
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), KernelStartupError> {
    let command = parse_command(std::env::args_os()).map_err(|error| match error {
        KernelCommandError::Command => KernelStartupError::Command,
        KernelCommandError::TransportPolicy => {
            KernelStartupError::Server(ServerStartupStage::TransportPolicy)
        }
    })?;
    match command {
        KernelCommand::NativeServe => run_native_server()
            .await
            .map_err(|()| KernelStartupError::Native),
        KernelCommand::Server {
            public_origin,
            exact_host,
        } => run_fixed_server(public_origin, exact_host)
            .await
            .map_err(KernelStartupError::Server),
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
    let composition = compose_fixed_native_kernel_runtime(config, paths, workspace_state)
        .await
        .map_err(|_| ())?;
    let runtime = composition.runtime().clone();

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

    let (http_shutdown_sender, http_shutdown_receiver) = tokio::sync::oneshot::channel();
    let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
        let _shutdown = http_shutdown_receiver.await;
    });
    await_native_shutdown(
        serve,
        native_shutdown_signal(control_receiver),
        http_shutdown_sender,
        async move {
            composition
                .shutdown()
                .await
                .map_err(|_error| NativeShutdownFailure::KernelDrain)
        },
        NATIVE_SHUTDOWN_DEADLINE,
    )
    .await
    .map_err(|_error| ())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeShutdownFailure {
    Serve,
    Signal,
    KernelDrain,
    Deadline,
}

async fn native_shutdown_signal(
    control: tokio::sync::oneshot::Receiver<
        Result<NativeHostControl, qingyu_kernel::host::native::NativeHostProtocolError>,
    >,
) -> Result<(), NativeShutdownFailure> {
    tokio::select! {
        control = control => match control {
            Ok(Ok(NativeHostControl::Shutdown | NativeHostControl::EndOfStream)) => Ok(()),
            Ok(Err(_)) | Err(_) => Err(NativeShutdownFailure::Signal),
        },
        () = server_shutdown_signal() => Ok(()),
    }
}

async fn await_native_shutdown<Serve, ShutdownSignal, KernelShutdown>(
    serve: Serve,
    shutdown_signal: ShutdownSignal,
    http_shutdown: tokio::sync::oneshot::Sender<()>,
    kernel_shutdown: KernelShutdown,
    deadline: Duration,
) -> Result<(), NativeShutdownFailure>
where
    Serve: IntoFuture<Output = std::io::Result<()>>,
    ShutdownSignal: std::future::Future<Output = Result<(), NativeShutdownFailure>>,
    KernelShutdown: std::future::Future<Output = Result<(), NativeShutdownFailure>>,
{
    let serve = serve.into_future();
    tokio::pin!(serve);
    tokio::pin!(shutdown_signal);
    tokio::select! {
        serve_result = &mut serve => {
            let kernel_result = tokio::time::timeout(deadline, kernel_shutdown)
                .await
                .map_err(|_elapsed| NativeShutdownFailure::Deadline)?;
            serve_result.map_err(|_error| NativeShutdownFailure::Serve)?;
            kernel_result
        }
        signal_result = &mut shutdown_signal => {
            let _http_shutdown_result = http_shutdown.send(());
            let (serve_result, kernel_result) = tokio::time::timeout(deadline, async {
                tokio::join!(&mut serve, kernel_shutdown)
            })
            .await
            .map_err(|_elapsed| NativeShutdownFailure::Deadline)?;
            signal_result?;
            serve_result.map_err(|_error| NativeShutdownFailure::Serve)?;
            kernel_result
        }
    }
}

async fn run_fixed_server(
    public_origin: String,
    exact_host: String,
) -> Result<(), ServerStartupStage> {
    let policy = TransportPolicy::same_origin(&exact_host, &public_origin)
        .map_err(|_| ServerStartupStage::TransportPolicy)?;
    let environment =
        ServerLaunchEnvironment::load().map_err(|_| ServerStartupStage::Environment)?;
    let paths = environment
        .layout()
        .activate()
        .map_err(|_| ServerStartupStage::Paths)?;
    let config = KernelConfig::generate().map_err(|_| ServerStartupStage::RuntimeConfig)?;
    let composition = compose_fixed_server_kernel(config, paths)
        .await
        .map_err(ServerStartupStage::Composition)?;
    let activation = composition
        .activate_api(environment)
        .map_err(|_| ServerStartupStage::AuthenticationApi)?;
    let kernel_lifecycle = activation
        .shutdown_handle()
        .ok_or(ServerStartupStage::KernelDrain)?;
    let router = build_server_web_router(activation, policy, SERVER_WEB_ROOT)
        .map_err(|_| ServerStartupStage::StaticRouter)?;
    let listener = tokio::net::TcpListener::bind(SERVER_LISTEN_ADDRESS)
        .await
        .map_err(|_| ServerStartupStage::Listener)?;
    let (http_shutdown_sender, http_shutdown_receiver) = tokio::sync::oneshot::channel();
    let serve = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _shutdown = http_shutdown_receiver.await;
    });

    let outcome = await_bounded_server_shutdown(
        serve,
        server_shutdown_signal(),
        http_shutdown_sender,
        async move { kernel_lifecycle.shutdown().await.map_err(|_error| ()) },
        SERVER_SHUTDOWN_DEADLINE,
    )
    .await
    .map_err(|error| match error {
        ServerShutdownFailure::Serve => ServerStartupStage::Serve,
        ServerShutdownFailure::KernelDrain => ServerStartupStage::KernelDrain,
    })?;
    match outcome {
        ServerShutdownOutcome::Drained => Ok(()),
        ServerShutdownOutcome::DeadlineElapsed => Err(ServerStartupStage::ShutdownDeadline),
    }
}

fn parse_command<Arguments, Argument>(args: Arguments) -> Result<KernelCommand, KernelCommandError>
where
    Arguments: IntoIterator<Item = Argument>,
    Argument: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let _executable = args.next().ok_or(KernelCommandError::Command)?;
    let command = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or(KernelCommandError::Command)?;
    match command.as_str() {
        "serve" if args.next().is_none() => Ok(KernelCommand::NativeServe),
        "server" => {
            if args.next().as_deref() != Some(std::ffi::OsStr::new("--public-origin")) {
                return Err(KernelCommandError::Command);
            }
            let public_origin = args
                .next()
                .and_then(|value| value.into_string().ok())
                .ok_or(KernelCommandError::Command)?;
            if args.next().is_some() {
                return Err(KernelCommandError::Command);
            }
            let parsed = reqwest::Url::parse(&public_origin)
                .map_err(|_| KernelCommandError::TransportPolicy)?;
            if parsed.scheme() != "https"
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.path() != "/"
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(KernelCommandError::TransportPolicy);
            }
            let canonical_origin = parsed.origin().ascii_serialization();
            if public_origin != canonical_origin {
                return Err(KernelCommandError::TransportPolicy);
            }
            let exact_host = canonical_origin
                .strip_prefix("https://")
                .filter(|authority| !authority.is_empty())
                .ok_or(KernelCommandError::TransportPolicy)?
                .to_owned();
            TransportPolicy::same_origin(&exact_host, &public_origin)
                .map_err(|_| KernelCommandError::TransportPolicy)?;
            Ok(KernelCommand::Server {
                public_origin,
                exact_host,
            })
        }
        _ => Err(KernelCommandError::Command),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerShutdownOutcome {
    Drained,
    DeadlineElapsed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerShutdownFailure {
    Serve,
    KernelDrain,
}

async fn await_bounded_server_shutdown<Serve, ShutdownSignal, KernelShutdown>(
    serve: Serve,
    shutdown_signal: ShutdownSignal,
    http_shutdown: tokio::sync::oneshot::Sender<()>,
    kernel_shutdown: KernelShutdown,
    deadline: Duration,
) -> Result<ServerShutdownOutcome, ServerShutdownFailure>
where
    Serve: IntoFuture<Output = std::io::Result<()>>,
    ShutdownSignal: std::future::Future<Output = ()>,
    KernelShutdown: std::future::Future<Output = Result<(), ()>>,
{
    let serve = serve.into_future();
    tokio::pin!(serve);
    tokio::pin!(shutdown_signal);
    tokio::select! {
        result = &mut serve => result
            .map(|()| ServerShutdownOutcome::Drained)
            .map_err(|_| ServerShutdownFailure::Serve),
        () = &mut shutdown_signal => {
            http_shutdown.send(()).map_err(|()| ServerShutdownFailure::Serve)?;
            let drain = async {
                let (serve_result, kernel_result) = tokio::join!(&mut serve, kernel_shutdown);
                serve_result.map_err(|_| ServerShutdownFailure::Serve)?;
                kernel_result.map_err(|()| ServerShutdownFailure::KernelDrain)?;
                Ok(())
            };
            match tokio::time::timeout(deadline, drain).await {
                Ok(result) => result.map(|()| ServerShutdownOutcome::Drained),
                // Returning drops the Axum serve future. The process-level Tokio runtime then
                // aborts HTTP and Kernel tasks that did not drain before the shared deadline.
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
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    use super::*;

    #[tokio::test]
    async fn native_control_and_eof_shutdown_drain_http_and_kernel_lifecycle() {
        for control in [NativeHostControl::Shutdown, NativeHostControl::EndOfStream] {
            let (control_sender, control_receiver) = tokio::sync::oneshot::channel();
            control_sender.send(Ok(control)).unwrap();
            let (http_shutdown_sender, http_shutdown_receiver) = tokio::sync::oneshot::channel();
            let http_drained = Arc::new(AtomicBool::new(false));
            let kernel_drained = Arc::new(AtomicBool::new(false));
            let http_drained_by_server = Arc::clone(&http_drained);
            let kernel_drained_by_lifecycle = Arc::clone(&kernel_drained);

            let result = await_native_shutdown(
                async move {
                    http_shutdown_receiver.await.map_err(|_closed| {
                        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "shutdown closed")
                    })?;
                    http_drained_by_server.store(true, Ordering::Release);
                    Ok(())
                },
                native_shutdown_signal(control_receiver),
                http_shutdown_sender,
                async move {
                    kernel_drained_by_lifecycle.store(true, Ordering::Release);
                    Ok(())
                },
                Duration::from_secs(1),
            )
            .await;

            assert_eq!(result, Ok(()));
            assert!(http_drained.load(Ordering::Acquire));
            assert!(kernel_drained.load(Ordering::Acquire));
        }
    }

    #[tokio::test]
    async fn native_process_signal_drains_http_and_kernel_lifecycle() {
        let (http_shutdown_sender, http_shutdown_receiver) = tokio::sync::oneshot::channel();
        let http_drained = Arc::new(AtomicBool::new(false));
        let kernel_drained = Arc::new(AtomicBool::new(false));
        let http_drained_by_server = Arc::clone(&http_drained);
        let kernel_drained_by_lifecycle = Arc::clone(&kernel_drained);

        let result = await_native_shutdown(
            async move {
                http_shutdown_receiver.await.map_err(|_closed| {
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "shutdown closed")
                })?;
                http_drained_by_server.store(true, Ordering::Release);
                Ok(())
            },
            async { Ok(()) },
            http_shutdown_sender,
            async move {
                kernel_drained_by_lifecycle.store(true, Ordering::Release);
                Ok(())
            },
            Duration::from_secs(1),
        )
        .await;

        assert_eq!(result, Ok(()));
        assert!(http_drained.load(Ordering::Acquire));
        assert!(kernel_drained.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn native_shutdown_deadline_bounds_pending_http_and_kernel_drain() {
        let (http_shutdown_sender, http_shutdown_receiver) = tokio::sync::oneshot::channel();
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            await_native_shutdown(
                async move {
                    http_shutdown_receiver.await.map_err(|_closed| {
                        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "shutdown closed")
                    })?;
                    std::future::pending::<std::io::Result<()>>().await
                },
                async { Ok(()) },
                http_shutdown_sender,
                std::future::pending::<Result<(), NativeShutdownFailure>>(),
                Duration::from_millis(10),
            ),
        )
        .await
        .expect("the native drain must observe its own deadline");

        assert_eq!(result, Err(NativeShutdownFailure::Deadline));
    }

    #[tokio::test]
    async fn native_serve_exit_still_bounds_pending_kernel_drain() {
        let (http_shutdown_sender, _http_shutdown_receiver) = tokio::sync::oneshot::channel();
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            await_native_shutdown(
                async { Ok(()) },
                std::future::pending::<Result<(), NativeShutdownFailure>>(),
                http_shutdown_sender,
                std::future::pending::<Result<(), NativeShutdownFailure>>(),
                Duration::from_millis(10),
            ),
        )
        .await
        .expect("serve-first native drain must observe its own deadline");

        assert_eq!(result, Err(NativeShutdownFailure::Deadline));
    }

    #[tokio::test]
    async fn native_signal_tolerates_an_already_closed_http_shutdown_receiver() {
        let (http_shutdown_sender, http_shutdown_receiver) = tokio::sync::oneshot::channel();
        drop(http_shutdown_receiver);

        let result = await_native_shutdown(
            async {
                tokio::task::yield_now().await;
                Ok(())
            },
            async { Ok(()) },
            http_shutdown_sender,
            async { Ok(()) },
            Duration::from_secs(1),
        )
        .await;

        assert_eq!(result, Ok(()));
    }

    fn server_command(public_origin: &str) -> Result<KernelCommand, KernelCommandError> {
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
        let (request_started, shutdown_started, release_request, release_kernel, server) =
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
        release_kernel.notify_waiters();
        client.abort();
    }

    #[tokio::test]
    async fn bounded_shutdown_returns_as_soon_as_active_requests_drain() {
        let deadline = std::time::Duration::from_secs(5);
        let (request_started, shutdown_started, release_request, release_kernel, server) =
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
        let mut task = server.task;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut task)
                .await
                .is_err(),
            "HTTP drain alone must not finish before the Kernel lifecycle drains"
        );
        release_kernel.notify_waiters();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(2), task)
                .await
                .expect("drained shutdown should return")
                .unwrap()
                .unwrap(),
            ServerShutdownOutcome::Drained
        );
        assert!(started_at.elapsed() < deadline);

        client.abort();
    }

    #[tokio::test]
    async fn bounded_shutdown_applies_the_same_deadline_to_kernel_drain() {
        let (signal_sender, signal_receiver) = tokio::sync::oneshot::channel();
        let (http_shutdown_sender, http_shutdown_receiver) = tokio::sync::oneshot::channel();
        let serve = async move {
            http_shutdown_receiver.await.map_err(|_closed| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "HTTP shutdown closed")
            })?;
            Ok(())
        };
        let task = tokio::spawn(await_bounded_server_shutdown(
            serve,
            async move {
                signal_receiver.await.unwrap();
            },
            http_shutdown_sender,
            async move {
                std::future::pending::<()>().await;
                Ok(())
            },
            Duration::from_millis(40),
        ));

        signal_sender.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .expect("Kernel drain must be bounded")
                .unwrap()
                .unwrap(),
            ServerShutdownOutcome::DeadlineElapsed
        );
    }

    struct SlowTestServer {
        address: SocketAddr,
        shutdown: tokio::sync::oneshot::Sender<()>,
        task: tokio::task::JoinHandle<Result<ServerShutdownOutcome, ServerShutdownFailure>>,
    }

    async fn slow_test_server(
        deadline: std::time::Duration,
    ) -> (
        Arc<tokio::sync::Notify>,
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
        let (http_shutdown_sender, http_shutdown_receiver) = tokio::sync::oneshot::channel();
        let shutdown_started = Arc::new(tokio::sync::Notify::new());
        let shutdown_started_in_kernel = Arc::clone(&shutdown_started);
        let release_kernel = Arc::new(tokio::sync::Notify::new());
        let release_kernel_in_shutdown = Arc::clone(&release_kernel);
        let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
            http_shutdown_receiver.await.unwrap();
        });
        let task = tokio::spawn(await_bounded_server_shutdown(
            serve,
            async move {
                shutdown_receiver.await.unwrap();
            },
            http_shutdown_sender,
            async move {
                shutdown_started_in_kernel.notify_one();
                release_kernel_in_shutdown.notified().await;
                Ok(())
            },
            deadline,
        ));
        (
            request_started,
            shutdown_started,
            release_request,
            release_kernel,
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

    #[test]
    fn command_parsing_separates_cli_shape_from_transport_policy() {
        assert_eq!(
            parse_command(["qingyu-kernel", "unknown"]).unwrap_err(),
            KernelCommandError::Command
        );
        assert_eq!(
            parse_command([
                "qingyu-kernel",
                "server",
                "--public-origin",
                "https://private-host-marker.example/path",
            ])
            .unwrap_err(),
            KernelCommandError::TransportPolicy
        );
    }

    #[test]
    fn command_and_server_startup_failures_emit_only_stable_stage_codes() {
        for (error, expected) in [
            (
                KernelStartupError::Command,
                "QingYu Kernel startup failed [QK-CMD].",
            ),
            (
                KernelStartupError::Server(ServerStartupStage::TransportPolicy),
                "QingYu Kernel startup failed [QK-SRV-TRANSPORT].",
            ),
            (
                KernelStartupError::Server(ServerStartupStage::Environment),
                "QingYu Kernel startup failed [QK-SRV-ENV].",
            ),
            (
                KernelStartupError::Server(ServerStartupStage::Paths),
                "QingYu Kernel startup failed [QK-SRV-PATHS].",
            ),
            (
                KernelStartupError::Server(ServerStartupStage::RuntimeConfig),
                "QingYu Kernel startup failed [QK-SRV-CONFIG].",
            ),
            (
                KernelStartupError::Server(ServerStartupStage::Composition(
                    ServerRuntimeCompositionError::FixedWorkspaceService,
                )),
                "QingYu Kernel startup failed [QK-SRV-COMPOSE-FIXED-WORKSPACE].",
            ),
            (
                KernelStartupError::Server(ServerStartupStage::AuthenticationApi),
                "QingYu Kernel startup failed [QK-SRV-AUTH-API].",
            ),
            (
                KernelStartupError::Server(ServerStartupStage::StaticRouter),
                "QingYu Kernel startup failed [QK-SRV-STATIC-ROUTER].",
            ),
            (
                KernelStartupError::Server(ServerStartupStage::Listener),
                "QingYu Kernel startup failed [QK-SRV-LISTENER].",
            ),
            (
                KernelStartupError::Server(ServerStartupStage::Serve),
                "QingYu Kernel startup failed [QK-SRV-SERVE].",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
            assert!(!error.to_string().contains("private-marker"));
        }

        assert_eq!(
            KernelStartupError::Native.to_string(),
            "QingYu Kernel startup failed."
        );
    }
}
