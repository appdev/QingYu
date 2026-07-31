use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use reqwest::{header, StatusCode};
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
use crate::{
    config::KernelConfig, contract::HostProfile, paths::KernelPaths, ports::KernelPorts,
    runtime::KernelRuntime,
};

const WEBVIEW_ORIGIN: &str = "qingyu://localhost";
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
        MobileKernelLaunch::new(self.runtime.clone(), lifecycle)
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
        tokio::time::timeout(Duration::from_secs(2), self.started.notified())
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

fn owner(timeout: Duration) -> MobileKernelHostOwner {
    MobileKernelHostOwner::new(timeout).unwrap()
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

    let mut stream = TcpStream::connect(endpoint.address()).await.unwrap();
    let websocket_request = format!(
        "GET /api/v1/events HTTP/1.1\r\nHost: {}\r\nOrigin: {WEBVIEW_ORIGIN}\r\nAuthorization: Bearer {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        endpoint.address(),
        endpoint.bearer().unwrap()
    );
    stream
        .write_all(websocket_request.as_bytes())
        .await
        .unwrap();
    let mut response = vec![0_u8; 1024];
    let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut response))
        .await
        .unwrap()
        .unwrap();
    let response = std::str::from_utf8(&response[..read]).unwrap();
    assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

    assert_eq!(
        owner.stop().await.unwrap(),
        MobileKernelStopDisposition::Stopped
    );
    assert_eq!(lifecycle.calls.load(Ordering::SeqCst), 1);
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
            MobileKernelLaunch::new(desktop.clone(), Arc::new(ImmediateLifecycle::default())),
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
