mod contract {
    pub use qingyu_kernel::contract::*;
}

mod events {
    pub use qingyu_kernel::events::*;
}

mod ports {
    pub use qingyu_kernel::ports::*;
}

#[path = "../src/ports/system.rs"]
mod system;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use qingyu_kernel::{
    contract::{
        DomainEvent, ResourceRefDto, Revision, WorkspaceDto, WorkspaceGeneration, WorkspaceId,
        WorkspaceReadiness,
    },
    events::{EventPublication, EventSink as _},
    ports::{
        Clock as _, CredentialSecret, CredentialSlot, CredentialStore as _, DiagnosticRecord,
        DiagnosticsSink as _, NetworkReachability as _, PortErrorKind, Sleeper as _,
        TaskSpawner as _,
    },
};
use static_assertions::assert_not_impl_any;
use system::{
    system_kernel_ports, AlwaysReachableNetwork, MemoryCredentialStore, NoopDiagnosticsSink,
    NoopEventSink, TokioSleeper, TokioTaskSpawner, UtcSystemClock,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

assert_not_impl_any!(MemoryCredentialStore: Clone, Copy);

#[test]
fn utc_clock_returns_a_current_canonical_utc_timestamp() {
    let before = OffsetDateTime::now_utc() - time::Duration::seconds(1);

    let observed = UtcSystemClock.now().unwrap();

    let after = OffsetDateTime::now_utc() + time::Duration::seconds(1);
    let parsed = OffsetDateTime::parse(observed.as_str(), &Rfc3339).unwrap();
    assert_eq!(parsed.offset(), UtcOffset::UTC);
    assert!(
        parsed >= before,
        "clock was behind the observed lower bound"
    );
    assert!(
        parsed <= after,
        "clock was ahead of the observed upper bound"
    );
}

#[test]
fn tokio_sleeper_reports_unavailable_instead_of_panicking_without_a_runtime() {
    let result = futures::executor::block_on(TokioSleeper.sleep(Duration::ZERO));

    assert_eq!(result.unwrap_err().kind(), PortErrorKind::Unavailable);
}

#[tokio::test(start_paused = true)]
async fn tokio_sleeper_completes_only_after_the_requested_duration() {
    let completed = Arc::new(AtomicBool::new(false));
    let observed = completed.clone();
    let task = tokio::spawn(async move {
        TokioSleeper.sleep(Duration::from_secs(5)).await.unwrap();
        observed.store(true, Ordering::SeqCst);
    });
    tokio::task::yield_now().await;

    tokio::time::advance(Duration::from_secs(4)).await;
    tokio::task::yield_now().await;
    assert!(!completed.load(Ordering::SeqCst));

    tokio::time::advance(Duration::from_secs(1)).await;
    task.await.unwrap();
    assert!(completed.load(Ordering::SeqCst));
}

#[test]
fn tokio_task_spawner_reports_unavailable_without_a_runtime() {
    let error = TokioTaskSpawner.spawn(Box::pin(async {})).unwrap_err();

    assert_eq!(error.kind(), PortErrorKind::Unavailable);
}

#[tokio::test]
async fn tokio_task_spawner_schedules_the_owned_task_on_the_current_runtime() {
    let (sender, receiver) = tokio::sync::oneshot::channel();

    TokioTaskSpawner
        .spawn(Box::pin(async move {
            let _send_result = sender.send("spawned");
        }))
        .unwrap();

    assert_eq!(receiver.await.unwrap(), "spawned");
}

#[test]
fn no_op_host_ports_accept_publication_and_diagnostics_and_report_reachable() {
    NoopEventSink.publish(&workspace_publication()).unwrap();
    NoopDiagnosticsSink
        .emit(DiagnosticRecord {
            code: "system-port-test",
        })
        .unwrap();
    assert!(AlwaysReachableNetwork.is_reachable().unwrap());
}

#[test]
fn memory_credentials_track_slots_and_clear_without_exposing_secret_debug() {
    let store = MemoryCredentialStore::default();
    let first_secret = CredentialSecret::new("webdav-password-must-not-leak");
    let replacement_secret = CredentialSecret::new("replacement-password-must-not-leak");
    let s3_secret = CredentialSecret::new("s3-secret-must-not-leak");

    assert!(!store.is_present(CredentialSlot::WebDavPassword).unwrap());
    store
        .replace(CredentialSlot::WebDavPassword, &first_secret)
        .unwrap();
    store
        .replace(CredentialSlot::WebDavPassword, &replacement_secret)
        .unwrap();
    store
        .replace(CredentialSlot::S3SecretAccessKey, &s3_secret)
        .unwrap();

    assert!(store.is_present(CredentialSlot::WebDavPassword).unwrap());
    assert!(!store.is_present(CredentialSlot::S3AccessKeyId).unwrap());
    assert!(store.is_present(CredentialSlot::S3SecretAccessKey).unwrap());
    let debug = format!("{store:?}");
    for secret in [
        first_secret.expose_secret(),
        replacement_secret.expose_secret(),
        s3_secret.expose_secret(),
    ] {
        assert!(!debug.contains(secret));
    }

    store.clear(CredentialSlot::WebDavPassword).unwrap();
    assert!(!store.is_present(CredentialSlot::WebDavPassword).unwrap());
    assert!(store.is_present(CredentialSlot::S3SecretAccessKey).unwrap());
}

#[tokio::test]
async fn system_kernel_ports_bundle_exposes_available_native_and_server_defaults() {
    let ports = system_kernel_ports();
    let secret = CredentialSecret::new("bundle-secret-must-not-leak");

    assert!(ports.clock().now().is_ok());
    ports.sleeper().sleep(Duration::ZERO).await.unwrap();
    ports
        .credential_store()
        .replace(CredentialSlot::S3SecretAccessKey, &secret)
        .unwrap();
    assert!(ports
        .credential_store()
        .is_present(CredentialSlot::S3SecretAccessKey)
        .unwrap());
    ports
        .diagnostics()
        .emit(DiagnosticRecord {
            code: "bundle-test",
        })
        .unwrap();
    assert!(ports.network_reachability().is_reachable().unwrap());
    ports
        .event_sink()
        .publish(&workspace_publication())
        .unwrap();

    let debug = format!("{ports:?}");
    assert!(!debug.contains(secret.expose_secret()));
}

fn workspace_publication() -> EventPublication {
    let workspace_id = WorkspaceId::new(uuid::Uuid::from_u128(41));
    let revision = Revision::parse("system-port-revision").unwrap();
    EventPublication {
        resource: ResourceRefDto::Workspace { id: workspace_id },
        revision: revision.clone(),
        event: DomainEvent::WorkspaceChanged {
            workspace: WorkspaceDto {
                id: workspace_id,
                generation: WorkspaceGeneration::parse("system-port-generation").unwrap(),
                display_name: "System ports fixture".to_owned(),
                readiness: WorkspaceReadiness::Ready,
                revision,
            },
        },
    }
}
