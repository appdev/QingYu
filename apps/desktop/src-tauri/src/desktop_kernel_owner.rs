use std::{future::Future, pin::Pin, sync::Arc};

use tokio::task::JoinHandle;

use crate::kernel_host::{
    kernel_endpoint_record::KernelEndpointRecordReader, KernelHostFailure,
    KernelHostPublicationSubscription, KernelHostSupervisor, NativeKernelAccess,
    NativeKernelLaunch,
};

type DesktopKernelFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait DesktopKernelDriver: Send + Sync + 'static {
    fn subscribe(&self) -> KernelHostPublicationSubscription;
    fn start(
        &self,
        launch: NativeKernelLaunch,
    ) -> DesktopKernelFuture<'_, Result<NativeKernelAccess, KernelHostFailure>>;
    fn stop(&self) -> DesktopKernelFuture<'_, Result<(), KernelHostFailure>>;
    fn close_fail_safe(&self);
}

/// Emits a payload-free invalidation edge. The renderer must re-read the
/// bootstrap/endpoint snapshot instead of receiving credentials in an event.
pub(crate) trait DesktopKernelEdgeEmitter: Send + Sync + 'static {
    fn emit_kernel_state_changed(&self);
}

pub(crate) struct DesktopKernelOwner {
    driver: Arc<dyn DesktopKernelDriver>,
    endpoints: KernelEndpointRecordReader,
    subscription: JoinHandle<()>,
}

impl DesktopKernelOwner {
    pub(crate) fn new<Driver, Emitter>(
        driver: Arc<Driver>,
        endpoints: KernelEndpointRecordReader,
        emitter: Arc<Emitter>,
    ) -> Self
    where
        Driver: DesktopKernelDriver,
        Emitter: DesktopKernelEdgeEmitter,
    {
        let mut publications = driver.subscribe();
        let subscription = tokio::spawn(async move {
            let mut last_sequence = 0;
            while let Some(publication) = publications.recv().await {
                if publication.sequence == 0 || publication.sequence <= last_sequence {
                    continue;
                }
                last_sequence = publication.sequence;
                emitter.emit_kernel_state_changed();
            }
        });
        Self {
            driver,
            endpoints,
            subscription,
        }
    }

    pub(crate) async fn start(
        &self,
        launch: NativeKernelLaunch,
    ) -> Result<NativeKernelAccess, KernelHostFailure> {
        self.driver.start(launch).await
    }

    /// Explicit shutdown preserves the supervisor's graceful drain contract.
    pub(crate) async fn stop(&self) -> Result<(), KernelHostFailure> {
        self.driver.stop().await
    }

    pub(crate) fn endpoint_reader(&self) -> KernelEndpointRecordReader {
        self.endpoints.clone()
    }
}

impl Drop for DesktopKernelOwner {
    fn drop(&mut self) {
        self.driver.close_fail_safe();
        self.subscription.abort();
    }
}

impl DesktopKernelDriver for KernelHostSupervisor {
    fn subscribe(&self) -> KernelHostPublicationSubscription {
        self.subscribe_publications()
    }

    fn start(
        &self,
        launch: NativeKernelLaunch,
    ) -> DesktopKernelFuture<'_, Result<NativeKernelAccess, KernelHostFailure>> {
        Box::pin(KernelHostSupervisor::start(self, launch))
    }

    fn stop(&self) -> DesktopKernelFuture<'_, Result<(), KernelHostFailure>> {
        Box::pin(KernelHostSupervisor::stop(self))
    }

    fn close_fail_safe(&self) {
        KernelHostSupervisor::close_fail_safe(self);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use qingyu_kernel::{config::NativeLaunchCredential, contract::InstanceId};
    use tokio::sync::Notify;
    use uuid::Uuid;

    use crate::kernel_bootstrap::NativeKernelBootstrapOwner;
    use crate::kernel_host::{
        KernelEndpoint, KernelHostFailure, KernelHostPhase, KernelHostPublicationSender,
        KernelHostSnapshot, NativeKernelAccess, NativeKernelCredentialLease, NativeKernelLaunch,
    };

    use super::{DesktopKernelDriver, DesktopKernelEdgeEmitter, DesktopKernelOwner};

    struct TestDriver {
        publications: KernelHostPublicationSender,
        access: Mutex<Option<NativeKernelAccess>>,
        starts: AtomicUsize,
        stops: AtomicUsize,
        fail_safe_closes: AtomicUsize,
        stop_gate: Notify,
    }

    impl DesktopKernelDriver for TestDriver {
        fn subscribe(&self) -> crate::kernel_host::KernelHostPublicationSubscription {
            self.publications.subscribe()
        }

        fn start(
            &self,
            _launch: NativeKernelLaunch,
        ) -> Pin<Box<dyn Future<Output = Result<NativeKernelAccess, KernelHostFailure>> + Send + '_>>
        {
            Box::pin(async move {
                self.starts.fetch_add(1, Ordering::SeqCst);
                self.access
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or(KernelHostFailure::Spawn)
            })
        }

        fn stop(&self) -> Pin<Box<dyn Future<Output = Result<(), KernelHostFailure>> + Send + '_>> {
            Box::pin(async move {
                self.stops.fetch_add(1, Ordering::SeqCst);
                self.stop_gate.notified().await;
                Ok(())
            })
        }

        fn close_fail_safe(&self) {
            self.fail_safe_closes.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct CountingEmitter(AtomicUsize);

    impl DesktopKernelEdgeEmitter for CountingEmitter {
        fn emit_kernel_state_changed(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn committed_edges_emit_once_without_payload_and_start_delegates() {
        let bootstrap = NativeKernelBootstrapOwner::new();
        let endpoint_reader = bootstrap.endpoint_reader();
        let (publications, _snapshots, _reader) = KernelHostPublicationSender::new();
        let access = test_access(1);
        let driver = Arc::new(TestDriver {
            publications: publications.clone(),
            access: Mutex::new(Some(access.clone())),
            starts: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            fail_safe_closes: AtomicUsize::new(0),
            stop_gate: Notify::new(),
        });
        let emitter = Arc::new(CountingEmitter(AtomicUsize::new(0)));
        let owner = DesktopKernelOwner::new(driver.clone(), endpoint_reader, emitter.clone());
        assert!(owner.endpoint_reader().read().unwrap().is_none());

        let started = owner.start(startup()).await.unwrap();
        assert_eq!(started.endpoint, access.endpoint);
        assert_eq!(driver.starts.load(Ordering::SeqCst), 1);

        publications.send_replace(KernelHostSnapshot {
            phase: KernelHostPhase::Starting,
            generation: 1,
            endpoint: None,
            failure: None,
        });
        publications.send_ready(
            KernelHostSnapshot {
                phase: KernelHostPhase::Ready,
                generation: 1,
                endpoint: Some(access.endpoint),
                failure: None,
            },
            access,
        );
        publications.send_replace(KernelHostSnapshot {
            phase: KernelHostPhase::Failed,
            generation: 1,
            endpoint: None,
            failure: Some(KernelHostFailure::UnexpectedExit),
        });

        wait_for_count(&emitter.0, 3).await;
        assert_eq!(emitter.0.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn explicit_stop_awaits_driver_drain_and_drop_is_only_the_fail_safe() {
        let bootstrap = NativeKernelBootstrapOwner::new();
        let (publications, _snapshots, _reader) = KernelHostPublicationSender::new();
        let driver = Arc::new(TestDriver {
            publications,
            access: Mutex::new(Some(test_access(1))),
            starts: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            fail_safe_closes: AtomicUsize::new(0),
            stop_gate: Notify::new(),
        });
        let owner = Arc::new(DesktopKernelOwner::new(
            driver.clone(),
            bootstrap.endpoint_reader(),
            Arc::new(CountingEmitter(AtomicUsize::new(0))),
        ));
        let stopping_owner = owner.clone();
        let stopping = tokio::spawn(async move { stopping_owner.stop().await });

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(driver.stops.load(Ordering::SeqCst), 1);
        assert!(!stopping.is_finished());
        assert_eq!(driver.fail_safe_closes.load(Ordering::SeqCst), 0);

        driver.stop_gate.notify_one();
        stopping.await.unwrap().unwrap();
        drop(owner);
        assert_eq!(driver.fail_safe_closes.load(Ordering::SeqCst), 1);
    }

    async fn wait_for_count(count: &AtomicUsize, expected: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while count.load(Ordering::SeqCst) != expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("all committed edges should be emitted once");
    }

    fn startup() -> NativeKernelLaunch {
        let workspace = std::env::temp_dir();
        NativeKernelLaunch::desktop(
            workspace.clone(),
            "/tmp/app-data".into(),
            "/tmp/cache".into(),
            qingyu_kernel::host::native::NativeHostWorkspaceState::for_workspace(
                &workspace,
                "Workspace",
            )
            .unwrap(),
            "http://127.0.0.1:4173".to_owned(),
        )
        .unwrap()
    }

    fn test_access(generation: u64) -> NativeKernelAccess {
        NativeKernelAccess {
            endpoint: KernelEndpoint {
                generation,
                port: 49_152,
                instance_id: InstanceId::new(Uuid::new_v4()),
            },
            credential: NativeKernelCredentialLease::new(
                NativeLaunchCredential::from_secret(
                    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                )
                .unwrap(),
            ),
        }
    }
}
