use std::{
    fs,
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Weak,
    },
    time::Duration,
};

use async_trait::async_trait;
use reqwest::{header, StatusCode};
use serde_json::{json, Value};
use tempfile::{tempdir, TempDir};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
    sync::Notify,
};
use zeroize::Zeroizing;

use super::mobile::{
    MobileKernelDrainError, MobileKernelEndpoint, MobileKernelHostErrorKind, MobileKernelHostOwner,
    MobileKernelLaunch, MobileKernelLifecycle, MobileKernelStopDisposition,
};
use crate::api::ApiConnectionLifecycle;
use crate::contract::ServerFrame;
use crate::{
    composition::compose_fixed_mobile_kernel, config::KernelConfig, contract::HostProfile,
    paths::KernelPaths, ports::KernelPorts, runtime::KernelRuntime,
};

const WEBVIEW_ORIGIN: &str = "qingyu://localhost";
const DRAIN_FAILURE_CHILD_ENV: &str = "QINGYU_MOBILE_DRAIN_FAILURE_CHILD";
static MOBILE_OWNER_TEST_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct MobileRuntimeFixture {
    _root: TempDir,
    runtime: Arc<KernelRuntime>,
}

impl MobileRuntimeFixture {
    fn new() -> Self {
        let root = tempdir().unwrap();
        let app_data = root.path().join("app-data");
        let cache = root.path().join("cache");
        fs::create_dir(&app_data).unwrap();
        fs::create_dir(&cache).unwrap();
        let paths = KernelPaths::mobile(&app_data, &cache, "primary").unwrap();
        let runtime = KernelRuntime::activate(
            KernelConfig::generate().unwrap(),
            paths,
            KernelPorts::unavailable(),
        )
        .unwrap();
        Self {
            _root: root,
            runtime,
        }
    }

    fn launch(&self, lifecycle: Arc<dyn MobileKernelLifecycle>) -> MobileKernelLaunch {
        MobileKernelLaunch::from_composition_parts(self.runtime.clone(), lifecycle)
    }
}

#[derive(Default)]
struct ImmediateLifecycle {
    calls: AtomicUsize,
}

#[async_trait]
impl MobileKernelLifecycle for ImmediateLifecycle {
    async fn drain(&self) -> Result<(), MobileKernelDrainError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Default)]
struct BlockingLifecycle {
    calls: AtomicUsize,
    release: Notify,
    started: Notify,
}

impl BlockingLifecycle {
    async fn wait_started(&self) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let started = self.started.notified();
                tokio::pin!(started);
                started.as_mut().enable();
                if self.calls.load(Ordering::SeqCst) > 0 {
                    return;
                }
                started.await;
            }
        })
        .await
        .expect("mobile drain did not start");
    }

    fn release(&self) {
        self.release.notify_waiters();
    }
}

#[async_trait]
impl MobileKernelLifecycle for BlockingLifecycle {
    async fn drain(&self) -> Result<(), MobileKernelDrainError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started.notify_waiters();
        self.release.notified().await;
        Ok(())
    }
}

struct FailingLifecycle;

#[async_trait]
impl MobileKernelLifecycle for FailingLifecycle {
    async fn drain(&self) -> Result<(), MobileKernelDrainError> {
        Err(MobileKernelDrainError)
    }
}

struct RuntimeReleaseLifecycle {
    runtime: Weak<KernelRuntime>,
}

#[async_trait]
impl MobileKernelLifecycle for RuntimeReleaseLifecycle {
    async fn drain(&self) -> Result<(), MobileKernelDrainError> {
        if self.runtime.upgrade().is_some() {
            Err(MobileKernelDrainError)
        } else {
            Ok(())
        }
    }
}

fn owner(timeout: Duration) -> MobileKernelHostOwner {
    MobileKernelHostOwner::new(timeout).unwrap()
}

async fn authenticated_json(endpoint: &MobileKernelEndpoint, path: &str) -> serde_json::Value {
    let response = reqwest::Client::new()
        .get(format!("{}{}", endpoint.base_url(), path))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", endpoint.bearer().unwrap()),
        )
        .header(header::ORIGIN, WEBVIEW_ORIGIN)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    serde_json::from_slice(&response.bytes().await.unwrap()).unwrap()
}

async fn authenticated_status(
    endpoint: &MobileKernelEndpoint,
    bearer: &str,
    origin: &str,
) -> StatusCode {
    reqwest::Client::new()
        .get(format!("{}/api/v1/health/ready", endpoint.base_url()))
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::ORIGIN, origin)
        .send()
        .await
        .unwrap()
        .status()
}

#[test]
fn mobile_paths_activate_only_the_managed_mobile_profile() {
    let fixture = MobileRuntimeFixture::new();

    assert_eq!(fixture.runtime.host_profile(), HostProfile::Mobile);
    assert!(fixture
        .runtime
        .active_workspace_authority()
        .unwrap()
        .root()
        .verify_held_directory()
        .is_ok());
}

#[tokio::test]
async fn production_mobile_composition_reuses_one_managed_workspace_identity_across_launches() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let root = tempdir().unwrap();
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    fs::create_dir(&app_data).unwrap();
    fs::create_dir(&cache).unwrap();

    let first_launch = compose_fixed_mobile_kernel(
        KernelConfig::generate().unwrap(),
        KernelPaths::mobile(&app_data, &cache, "primary").unwrap(),
        "QingYu",
    )
    .await
    .unwrap();
    let first_owner = owner(Duration::from_secs(2));
    let first_endpoint = first_owner
        .start(first_launch, WEBVIEW_ORIGIN)
        .await
        .unwrap();
    let first_runtime = authenticated_json(&first_endpoint, "/api/v1/runtime").await;
    let first_workspace = authenticated_json(&first_endpoint, "/api/v1/workspace").await;
    assert_eq!(first_runtime["profile"], "mobile");
    assert_eq!(first_workspace["readiness"], "ready");
    first_owner.stop().await.unwrap();
    drop(first_endpoint);
    drop(first_owner);

    let second_launch = compose_fixed_mobile_kernel(
        KernelConfig::generate().unwrap(),
        KernelPaths::mobile(&app_data, &cache, "primary").unwrap(),
        "QingYu",
    )
    .await
    .unwrap();
    let second_owner = owner(Duration::from_secs(2));
    let second_endpoint = second_owner
        .start(second_launch, WEBVIEW_ORIGIN)
        .await
        .unwrap();
    let second_workspace = authenticated_json(&second_endpoint, "/api/v1/workspace").await;

    assert_eq!(second_workspace["id"], first_workspace["id"]);
    assert_eq!(second_workspace["displayName"], "QingYu");
    second_owner.stop().await.unwrap();
}

#[test]
fn mobile_host_source_cannot_spawn_a_child_or_use_the_desktop_native_protocol() {
    let source = include_str!("mobile.rs");
    for forbidden in [
        "std::process",
        "tokio::process",
        "Command::new",
        "NativeHostStart",
        "NativeHostControl",
    ] {
        assert!(
            !source.contains(forbidden),
            "mobile in-process host crossed the process boundary with {forbidden}"
        );
    }
}

#[tokio::test]
async fn mobile_host_binds_one_random_ipv4_loopback_http_and_websocket_endpoint() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let fixture = MobileRuntimeFixture::new();
    let lifecycle = Arc::new(ImmediateLifecycle::default());
    let owner = owner(Duration::from_secs(2));

    let endpoint = owner
        .start(fixture.launch(lifecycle.clone()), WEBVIEW_ORIGIN)
        .await
        .unwrap();

    assert_eq!(endpoint.address().ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_ne!(endpoint.address().port(), 0);
    assert_eq!(
        endpoint.base_url(),
        format!("http://{}", endpoint.address())
    );
    assert_eq!(
        endpoint.events_url(),
        format!("ws://{}/api/v1/events", endpoint.address())
    );

    let live = reqwest::Client::new()
        .get(format!("{}/api/v1/health/live", endpoint.base_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(live.status(), StatusCode::OK);

    let _socket = RawWebSocket::connect(endpoint.address(), WEBVIEW_ORIGIN)
        .await
        .expect("mobile websocket upgrade failed");

    assert_eq!(
        owner.stop().await.unwrap(),
        MobileKernelStopDisposition::Stopped
    );
    assert_eq!(lifecycle.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stop_closes_authenticated_and_pending_websockets_before_lifecycle_drain() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let fixture = MobileRuntimeFixture::new();
    let weak_runtime = Arc::downgrade(&fixture.runtime);
    let owner = owner(Duration::from_secs(2));
    let endpoint = owner
        .start(
            fixture.launch(Arc::new(RuntimeReleaseLifecycle {
                runtime: weak_runtime.clone(),
            })),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap();
    let bearer = Zeroizing::new(endpoint.bearer().unwrap().to_owned());
    let mut authenticated = RawWebSocket::connect(endpoint.address(), WEBVIEW_ORIGIN)
        .await
        .unwrap();
    authenticated
        .send_json(&json!({
            "type": "authenticate",
            "protocolVersion": 1,
            "credential": bearer.as_str(),
        }))
        .await;
    assert!(matches!(
        authenticated.read_server_frame().await,
        ServerFrame::Ready { .. }
    ));
    let mut pending = RawWebSocket::connect(endpoint.address(), WEBVIEW_ORIGIN)
        .await
        .unwrap();
    drop(fixture);

    assert_eq!(
        owner.stop().await.unwrap(),
        MobileKernelStopDisposition::Stopped
    );
    assert!(weak_runtime.upgrade().is_none());
    authenticated.expect_host_shutdown_close().await;
    pending.expect_host_shutdown_close().await;
}

#[tokio::test]
async fn connection_shutdown_rejects_late_registration_and_waits_for_existing_connections() {
    let lifecycle = ApiConnectionLifecycle::new();
    let connection = lifecycle.register().expect("connection should register");

    lifecycle.begin_shutdown();
    assert!(lifecycle.register().is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), lifecycle.wait_drained())
            .await
            .is_err(),
        "shutdown reported drained while an upgraded connection remained"
    );

    drop(connection);
    tokio::time::timeout(Duration::from_secs(2), lifecycle.wait_drained())
        .await
        .expect("connection shutdown lost the final release notification");
}

#[tokio::test]
async fn stop_rejects_websocket_upgrades_after_revocation_while_lifecycle_is_draining() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let fixture = MobileRuntimeFixture::new();
    let lifecycle = Arc::new(BlockingLifecycle::default());
    let owner = Arc::new(owner(Duration::from_secs(2)));
    let endpoint = owner
        .start(fixture.launch(lifecycle.clone()), WEBVIEW_ORIGIN)
        .await
        .unwrap();
    let mut active = RawWebSocket::connect(endpoint.address(), WEBVIEW_ORIGIN)
        .await
        .unwrap();

    let stopping_owner = owner.clone();
    let stop = tokio::spawn(async move { stopping_owner.stop().await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while endpoint.bearer().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("mobile endpoint was not revoked");

    let late_upgrade = tokio::time::timeout(
        Duration::from_secs(2),
        RawWebSocket::connect(endpoint.address(), WEBVIEW_ORIGIN),
    )
    .await;
    assert!(
        !matches!(late_upgrade, Ok(Ok(_))),
        "a websocket upgraded after mobile shutdown began"
    );
    active.expect_host_shutdown_close().await;
    lifecycle.wait_started().await;
    lifecycle.release();
    assert_eq!(
        stop.await.unwrap().unwrap(),
        MobileKernelStopDisposition::Stopped
    );
}

#[tokio::test]
async fn mobile_transport_rejects_wrong_host_and_origin_before_authentication() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let fixture = MobileRuntimeFixture::new();
    let owner = owner(Duration::from_secs(2));
    let endpoint = owner
        .start(
            fixture.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap();
    let bearer = endpoint.bearer().unwrap();

    let wrong_origin = authenticated_status(&endpoint, bearer, "https://attacker.invalid").await;
    assert_eq!(wrong_origin, StatusCode::FORBIDDEN);

    let wrong_host = reqwest::Client::new()
        .get(format!("{}/api/v1/health/ready", endpoint.base_url()))
        .header(header::HOST, "127.0.0.1:1")
        .header(header::ORIGIN, WEBVIEW_ORIGIN)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_host.status(), StatusCode::FORBIDDEN);

    owner.stop().await.unwrap();
}

#[tokio::test]
async fn one_owner_rejects_a_second_active_kernel() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let first = MobileRuntimeFixture::new();
    let second = MobileRuntimeFixture::new();
    let owner = owner(Duration::from_secs(2));
    let _endpoint = owner
        .start(
            first.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap();

    let error = owner
        .start(
            second.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), MobileKernelHostErrorKind::AlreadyActive);
    owner.stop().await.unwrap();
}

#[tokio::test]
async fn the_process_cannot_construct_a_second_mobile_host_owner() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let first = owner(Duration::from_secs(2));

    let second = MobileKernelHostOwner::new(Duration::from_secs(2));

    assert_eq!(
        second.unwrap_err().kind(),
        MobileKernelHostErrorKind::ProcessOwnerClaimed
    );
    drop(first);
}

#[tokio::test]
async fn process_claim_survives_owner_drop_until_the_running_lifecycle_drains() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let fixture = MobileRuntimeFixture::new();
    let lifecycle = Arc::new(BlockingLifecycle::default());
    let owner = owner(Duration::from_secs(2));
    let endpoint = owner
        .start(fixture.launch(lifecycle.clone()), WEBVIEW_ORIGIN)
        .await
        .unwrap();

    drop(owner);
    lifecycle.wait_started().await;
    assert!(endpoint.bearer().is_err());
    assert_eq!(
        MobileKernelHostOwner::new(Duration::from_secs(2))
            .unwrap_err()
            .kind(),
        MobileKernelHostErrorKind::ProcessOwnerClaimed
    );

    lifecycle.release();
    let replacement = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match MobileKernelHostOwner::new(Duration::from_secs(2)) {
                Ok(owner) => return owner,
                Err(error) => {
                    assert_eq!(error.kind(), MobileKernelHostErrorKind::ProcessOwnerClaimed);
                    tokio::task::yield_now().await;
                }
            }
        }
    })
    .await
    .expect("the process claim was not released after drain");
    drop(replacement);
}

#[tokio::test]
async fn stop_revokes_the_old_epoch_and_bearer_before_restart() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let first = MobileRuntimeFixture::new();
    let owner = owner(Duration::from_secs(2));
    let first_endpoint = owner
        .start(
            first.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap();
    let old_bearer = Zeroizing::new(first_endpoint.bearer().unwrap().to_owned());

    owner.stop().await.unwrap();
    assert!(first_endpoint.bearer().is_err());

    let second = MobileRuntimeFixture::new();
    let second_endpoint = owner
        .start(
            second.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap();
    let new_bearer = second_endpoint.bearer().unwrap();
    assert_ne!(old_bearer.as_str(), new_bearer);
    assert_eq!(
        authenticated_status(&second_endpoint, old_bearer.as_str(), WEBVIEW_ORIGIN).await,
        StatusCode::UNAUTHORIZED
    );
    assert_ne!(
        authenticated_status(&second_endpoint, new_bearer, WEBVIEW_ORIGIN).await,
        StatusCode::UNAUTHORIZED
    );

    owner.stop().await.unwrap();
}

#[tokio::test]
async fn a_retired_runtime_epoch_cannot_reactivate_its_old_bearer() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let fixture = MobileRuntimeFixture::new();
    let owner = owner(Duration::from_secs(2));
    let endpoint = owner
        .start(
            fixture.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap();
    let old_bearer = Zeroizing::new(endpoint.bearer().unwrap().to_owned());
    owner.stop().await.unwrap();

    let reused = owner
        .start(
            fixture.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await;

    assert_eq!(
        reused.unwrap_err().kind(),
        MobileKernelHostErrorKind::RetiredLaunch
    );
    assert!(endpoint.bearer().is_err());
    assert!(!old_bearer.is_empty());
}

#[tokio::test]
async fn retired_launch_identity_survives_owner_replacement_and_a_b_a() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let first = MobileRuntimeFixture::new();
    let first_owner = owner(Duration::from_secs(2));
    let first_endpoint = first_owner
        .start(
            first.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap();
    let old_bearer = Zeroizing::new(first_endpoint.bearer().unwrap().to_owned());
    first_owner.stop().await.unwrap();
    drop(first_owner);

    let second = MobileRuntimeFixture::new();
    let second_owner = owner(Duration::from_secs(2));
    let second_endpoint = second_owner
        .start(
            second.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap();
    assert_eq!(
        authenticated_status(&second_endpoint, old_bearer.as_str(), WEBVIEW_ORIGIN).await,
        StatusCode::UNAUTHORIZED
    );
    second_owner.stop().await.unwrap();

    let reused = second_owner
        .start(
            first.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await;
    assert_eq!(
        reused.unwrap_err().kind(),
        MobileKernelHostErrorKind::RetiredLaunch
    );
}

#[test]
fn lifecycle_drain_failure_latches_the_mobile_process_closed() {
    let executable = std::env::current_exe().unwrap();
    let status = Command::new(executable)
        .args([
            "--ignored",
            "--exact",
            "host::mobile_tests::drain_failure_process_helper",
        ])
        .env(DRAIN_FAILURE_CHILD_ENV, "1")
        .status()
        .unwrap();
    assert!(status.success(), "drain failure child assertions failed");
}

#[tokio::test]
#[ignore = "child-process helper"]
async fn drain_failure_process_helper() {
    assert_eq!(std::env::var(DRAIN_FAILURE_CHILD_ENV).as_deref(), Ok("1"));
    let first = MobileRuntimeFixture::new();
    let owner = owner(Duration::from_secs(2));
    let _endpoint = owner
        .start(first.launch(Arc::new(FailingLifecycle)), WEBVIEW_ORIGIN)
        .await
        .unwrap();

    let drain_error = owner.stop().await.unwrap_err();
    assert_eq!(drain_error.kind(), MobileKernelHostErrorKind::DrainFailed);

    let second = MobileRuntimeFixture::new();
    let same_owner = owner
        .start(
            second.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await;
    assert_eq!(
        same_owner.unwrap_err().kind(),
        MobileKernelHostErrorKind::ProcessPoisoned
    );
    drop(owner);
    assert_eq!(
        MobileKernelHostOwner::new(Duration::from_secs(2))
            .unwrap_err()
            .kind(),
        MobileKernelHostErrorKind::ProcessPoisoned
    );
}

#[tokio::test]
async fn cancelled_stop_caller_does_not_cancel_drain_or_unrevoke_the_endpoint() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let fixture = MobileRuntimeFixture::new();
    let lifecycle = Arc::new(BlockingLifecycle::default());
    let owner = Arc::new(owner(Duration::from_secs(2)));
    let endpoint = owner
        .start(fixture.launch(lifecycle.clone()), WEBVIEW_ORIGIN)
        .await
        .unwrap();

    let first_owner = owner.clone();
    let first_stop = tokio::spawn(async move { first_owner.stop().await });
    lifecycle.wait_started().await;
    assert!(endpoint.bearer().is_err());
    let replacement = MobileRuntimeFixture::new();
    let replacement_error = owner
        .start(
            replacement.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap_err();
    assert_eq!(
        replacement_error.kind(),
        MobileKernelHostErrorKind::Stopping
    );
    first_stop.abort();
    assert!(first_stop.await.unwrap_err().is_cancelled());

    let second_owner = owner.clone();
    let second_stop = tokio::spawn(async move { second_owner.stop().await });
    tokio::task::yield_now().await;
    assert_eq!(lifecycle.calls.load(Ordering::SeqCst), 1);
    lifecycle.release();
    assert_eq!(
        second_stop.await.unwrap().unwrap(),
        MobileKernelStopDisposition::Stopped
    );
    assert_eq!(lifecycle.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn an_old_concurrent_stop_waiter_cannot_overwrite_a_new_running_generation() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let first = MobileRuntimeFixture::new();
    let lifecycle = Arc::new(BlockingLifecycle::default());
    let owner = owner(Duration::from_secs(2));
    let first_endpoint = owner
        .start(first.launch(lifecycle.clone()), WEBVIEW_ORIGIN)
        .await
        .unwrap();

    let first_stop = owner.stop();
    tokio::pin!(first_stop);
    std::future::poll_fn(|context| {
        assert!(first_stop.as_mut().poll(context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let old_waiter = owner.stop();
    tokio::pin!(old_waiter);
    std::future::poll_fn(|context| {
        assert!(old_waiter.as_mut().poll(context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    lifecycle.wait_started().await;
    lifecycle.release();
    assert_eq!(
        first_stop.await.unwrap(),
        MobileKernelStopDisposition::Stopped
    );
    assert!(first_endpoint.bearer().is_err());

    let second = MobileRuntimeFixture::new();
    let second_endpoint = owner
        .start(
            second.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap();
    assert_eq!(
        old_waiter.await.unwrap(),
        MobileKernelStopDisposition::Stopped
    );
    assert!(second_endpoint.is_current());

    owner.stop().await.unwrap();
}

#[tokio::test]
async fn bounded_stop_times_out_safely_while_the_shared_drain_keeps_settling() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let fixture = MobileRuntimeFixture::new();
    let lifecycle = Arc::new(BlockingLifecycle::default());
    let owner = owner(Duration::from_millis(20));
    let endpoint = owner
        .start(fixture.launch(lifecycle.clone()), WEBVIEW_ORIGIN)
        .await
        .unwrap();

    let error = owner.stop().await.unwrap_err();
    assert_eq!(error.kind(), MobileKernelHostErrorKind::DrainTimedOut);
    assert!(endpoint.bearer().is_err());
    assert_eq!(lifecycle.calls.load(Ordering::SeqCst), 1);

    lifecycle.release();
    assert_eq!(
        owner.stop().await.unwrap(),
        MobileKernelStopDisposition::Stopped
    );
}

#[tokio::test]
async fn invalid_profile_and_origin_errors_never_render_paths_origins_or_credentials() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("private-workspace");
    let app_data = temporary.path().join("private-app-data");
    let cache = temporary.path().join("private-cache");
    for path in [&workspace, &app_data, &cache] {
        fs::create_dir(path).unwrap();
    }
    let desktop = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        KernelPaths::desktop(&workspace, &app_data, &cache).unwrap(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let owner = owner(Duration::from_secs(2));
    let profile_error = owner
        .start(
            MobileKernelLaunch::from_composition_parts(
                desktop.clone(),
                Arc::new(ImmediateLifecycle::default()),
            ),
            WEBVIEW_ORIGIN,
        )
        .await
        .unwrap_err();
    assert_eq!(
        profile_error.kind(),
        MobileKernelHostErrorKind::UnsupportedProfile
    );

    let mobile = MobileRuntimeFixture::new();
    let secret_origin = "secret-value\r\nX-Leak: private-value";
    let origin_error = owner
        .start(
            mobile.launch(Arc::new(ImmediateLifecycle::default())),
            secret_origin,
        )
        .await
        .unwrap_err();
    assert_eq!(
        origin_error.kind(),
        MobileKernelHostErrorKind::InvalidOrigin
    );

    let rendered =
        format!("{profile_error:?} {profile_error} {origin_error:?} {origin_error} {desktop:?}");
    for secret in [
        "private-workspace",
        "private-app-data",
        "private-cache",
        "secret-value",
        "private-value",
        desktop.expose_native_launch_credential(),
    ] {
        assert!(!rendered.contains(secret));
    }

    let recovered = MobileRuntimeFixture::new();
    let endpoint = owner
        .start(
            recovered.launch(Arc::new(ImmediateLifecycle::default())),
            WEBVIEW_ORIGIN,
        )
        .await
        .expect("an invalid origin stranded the mobile start reservation");
    assert!(endpoint.is_current());
    owner.stop().await.unwrap();
}

#[tokio::test]
async fn stopping_an_idle_owner_is_idempotent() {
    let _test_gate = MOBILE_OWNER_TEST_GATE.lock().await;
    let owner = owner(Duration::from_secs(2));

    assert_eq!(
        owner.stop().await.unwrap(),
        MobileKernelStopDisposition::AlreadyStopped
    );
}

#[derive(Debug, Eq, PartialEq)]
enum WsMessage {
    Text(String),
    Close(u16),
}

struct RawWebSocket {
    stream: TcpStream,
}

impl RawWebSocket {
    async fn connect(address: SocketAddr, origin: &str) -> Result<Self, String> {
        let mut stream = TcpStream::connect(address)
            .await
            .map_err(|error| error.to_string())?;
        let request = format!(
            "GET /api/v1/events HTTP/1.1\r\n\
             Host: {address}\r\n\
             Origin: {origin}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: AAECAwQFBgcICQoLDA0ODw==\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|error| error.to_string())?;

        let mut response = Vec::new();
        let mut byte = [0_u8; 1];
        while !response.ends_with(b"\r\n\r\n") {
            stream
                .read_exact(&mut byte)
                .await
                .map_err(|error| error.to_string())?;
            response.push(byte[0]);
            if response.len() >= 16 * 1024 {
                return Err("upgrade response exceeded its test bound".to_owned());
            }
        }
        let response = String::from_utf8(response).map_err(|error| error.to_string())?;
        if !response.starts_with("HTTP/1.1 101 ") {
            return Err(response);
        }
        Ok(Self { stream })
    }

    async fn send_json(&mut self, value: &Value) {
        let serialized = serde_json::to_string(value).unwrap();
        let payload = serialized.as_bytes();
        let mut frame = vec![0x81];
        match payload.len() {
            length if length < 126 => frame.push(0x80 | length as u8),
            length if u16::try_from(length).is_ok() => {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&(length as u16).to_be_bytes());
            }
            length => {
                frame.push(0x80 | 127);
                frame.extend_from_slice(&(length as u64).to_be_bytes());
            }
        }
        let mask = [0x19, 0x7a, 0xc3, 0x4d];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % mask.len()]),
        );
        self.stream.write_all(&frame).await.unwrap();
    }

    async fn read_server_frame(&mut self) -> ServerFrame {
        match self.read_message().await {
            WsMessage::Text(text) => serde_json::from_str(&text).unwrap(),
            WsMessage::Close(code) => panic!("expected a server frame before close {code}"),
        }
    }

    async fn expect_host_shutdown_close(&mut self) {
        let message = tokio::time::timeout(Duration::from_secs(2), self.read_message())
            .await
            .expect("mobile websocket did not close during host shutdown");
        assert_eq!(message, WsMessage::Close(1001));
    }

    async fn read_message(&mut self) -> WsMessage {
        loop {
            let mut header = [0_u8; 2];
            self.stream.read_exact(&mut header).await.unwrap();
            assert_ne!(header[0] & 0x80, 0, "test server frames must not fragment");
            assert_eq!(header[1] & 0x80, 0, "server frames must not be masked");
            let length = match header[1] & 0x7f {
                length @ 0..=125 => u64::from(length),
                126 => {
                    let mut bytes = [0_u8; 2];
                    self.stream.read_exact(&mut bytes).await.unwrap();
                    u64::from(u16::from_be_bytes(bytes))
                }
                127 => {
                    let mut bytes = [0_u8; 8];
                    self.stream.read_exact(&mut bytes).await.unwrap();
                    u64::from_be_bytes(bytes)
                }
                _ => unreachable!(),
            };
            let length = usize::try_from(length).unwrap();
            assert!(length <= 20 * 1024 * 1024);
            let mut payload = vec![0_u8; length];
            self.stream.read_exact(&mut payload).await.unwrap();
            match header[0] & 0x0f {
                0x1 => return WsMessage::Text(String::from_utf8(payload).unwrap()),
                0x8 => {
                    let code = payload
                        .get(..2)
                        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
                        .unwrap_or(1005);
                    return WsMessage::Close(code);
                }
                0x9 => self.send_control(0xA, &payload).await,
                opcode => panic!("unexpected server websocket opcode {opcode:#x}"),
            }
        }
    }

    async fn send_control(&mut self, opcode: u8, payload: &[u8]) {
        assert!(payload.len() <= 125);
        let mask = [0x52, 0x0b, 0xa6, 0xd1];
        let mut frame = vec![0x80 | opcode, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % mask.len()]),
        );
        self.stream.write_all(&frame).await.unwrap();
    }
}
