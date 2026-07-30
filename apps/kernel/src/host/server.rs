//! Server-host lifecycle and direct-peer trust primitives.
//!
//! This module is intentionally transport-neutral apart from accepting the
//! request header map as an explicitly untrusted input. HTTP routing and the
//! authentication coordinator are composed by a later server-host slice.

#![allow(dead_code)] // Staged until the server HTTP host composes these primitives.

use std::{
    fmt,
    net::{IpAddr, SocketAddr},
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use axum::http::HeaderMap;
use hmac::{Hmac, Mac as _};
use sha2::Sha256;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;
use zeroize::Zeroize as _;

use crate::config::KernelLaunchEpoch;

#[derive(Clone, Copy, Eq, PartialEq)]
struct ServerLaunchEpochId(Uuid);

impl ServerLaunchEpochId {
    const fn from_launch_epoch(epoch: &KernelLaunchEpoch) -> Self {
        Self(epoch.value())
    }
}

impl fmt::Debug for ServerLaunchEpochId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerLaunchEpochId(..)")
    }
}

struct ServerHostBundle<T> {
    epoch: ServerLaunchEpochId,
    state: T,
}

struct ServerHostEpochSlotInner<T> {
    current: RwLock<Option<Arc<ServerHostBundle<T>>>>,
    blocking_gate: Arc<Semaphore>,
}

/// Owns the replaceable server-host bundle and the process-wide blocking cap.
///
/// The blocking semaphore intentionally lives outside each launch bundle, so
/// replacing an epoch cannot bypass work already occupying the bounded pool.
pub(crate) struct ServerHostEpochSlot<T> {
    inner: Arc<ServerHostEpochSlotInner<T>>,
}

impl<T> ServerHostEpochSlot<T> {
    pub(crate) fn new(blocking_capacity: usize) -> Result<Self, ServerHostEpochSlotError> {
        if blocking_capacity == 0 {
            return Err(ServerHostEpochSlotError::ZeroBlockingCapacity);
        }
        Ok(Self {
            inner: Arc::new(ServerHostEpochSlotInner {
                current: RwLock::new(None),
                blocking_gate: Arc::new(Semaphore::new(blocking_capacity)),
            }),
        })
    }

    /// Atomically replaces the current launch incarnation.
    pub(crate) fn replace(&self, epoch: &KernelLaunchEpoch, state: T) -> ServerHostLease<T> {
        let bundle = Arc::new(ServerHostBundle {
            epoch: ServerLaunchEpochId::from_launch_epoch(epoch),
            state,
        });
        *write_unpoisoned(&self.inner.current) = Some(Arc::clone(&bundle));
        ServerHostLease {
            slot: Arc::clone(&self.inner),
            bundle,
        }
    }

    /// Clears a launch only when the supplied lease is still the exact current
    /// incarnation. A stale lease cannot clear a replacement with the same
    /// launch epoch (the ABA case).
    pub(crate) fn invalidate(&self, lease: &ServerHostLease<T>) -> bool {
        if !Arc::ptr_eq(&self.inner, &lease.slot) {
            return false;
        }
        let mut current = write_unpoisoned(&self.inner.current);
        let matches = current
            .as_ref()
            .is_some_and(|bundle| same_incarnation(bundle, &lease.bundle));
        if matches {
            *current = None;
        }
        matches
    }
}

impl<T> fmt::Debug for ServerHostEpochSlot<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerHostEpochSlot(..)")
    }
}

pub(crate) struct ServerHostLease<T> {
    slot: Arc<ServerHostEpochSlotInner<T>>,
    bundle: Arc<ServerHostBundle<T>>,
}

impl<T> Clone for ServerHostLease<T> {
    fn clone(&self) -> Self {
        Self {
            slot: Arc::clone(&self.slot),
            bundle: Arc::clone(&self.bundle),
        }
    }
}

impl<T> ServerHostLease<T> {
    pub(crate) fn is_current(&self) -> bool {
        read_unpoisoned(&self.slot.current)
            .as_ref()
            .is_some_and(|bundle| same_incarnation(bundle, &self.bundle))
    }
}

impl<T> ServerHostLease<T>
where
    T: Send + Sync + 'static,
{
    /// Runs one synchronous operation without queuing beyond the host's fixed
    /// blocking capacity.
    ///
    /// An operation is never retried. The launch incarnation is checked before
    /// dispatch, immediately inside the blocking worker, after the operation,
    /// and again after joining. If the launch changes, any produced value is
    /// dropped rather than published. Once dispatched, the worker owns its
    /// permit, so cancelling the async waiter cannot cancel or leak settlement.
    pub(crate) async fn run_blocking<R, F>(&self, operation: F) -> Result<R, ServerBlockingError>
    where
        R: Send + 'static,
        F: FnOnce(&T) -> R + Send + 'static,
    {
        if !self.is_current() {
            return Err(ServerBlockingError::StaleLaunch);
        }
        let permit = Arc::clone(&self.slot.blocking_gate)
            .try_acquire_owned()
            .map_err(|_| ServerBlockingError::Saturated)?;
        if !self.is_current() {
            return Err(ServerBlockingError::StaleLaunch);
        }

        let lease = self.clone();
        let worker = tokio::task::spawn_blocking(move || {
            run_blocking_once_with_settlement(lease, permit, operation)
        });
        match worker
            .await
            .map_err(|_| ServerBlockingError::WorkerFailed)?
        {
            BlockingWorkerOutcome::StaleLaunch => Err(ServerBlockingError::StaleLaunch),
            BlockingWorkerOutcome::Completed { lease, result } => {
                if lease.is_current() {
                    Ok(result)
                } else {
                    Err(ServerBlockingError::StaleLaunch)
                }
            }
        }
    }
}

fn run_blocking_once_with_settlement<T, R, F>(
    lease: ServerHostLease<T>,
    permit: OwnedSemaphorePermit,
    operation: F,
) -> BlockingWorkerOutcome<T, R>
where
    T: Send + Sync + 'static,
    R: Send + 'static,
    F: FnOnce(&T) -> R,
{
    let _permit = permit;
    if !lease.is_current() {
        return BlockingWorkerOutcome::StaleLaunch;
    }
    let result = operation(&lease.bundle.state);
    if !lease.is_current() {
        return BlockingWorkerOutcome::StaleLaunch;
    }
    BlockingWorkerOutcome::Completed { lease, result }
}

enum BlockingWorkerOutcome<T, R> {
    StaleLaunch,
    Completed {
        lease: ServerHostLease<T>,
        result: R,
    },
}

fn same_incarnation<T>(
    current: &Arc<ServerHostBundle<T>>,
    candidate: &Arc<ServerHostBundle<T>>,
) -> bool {
    current.epoch == candidate.epoch && Arc::ptr_eq(current, candidate)
}

fn read_unpoisoned<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write_unpoisoned<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServerHostEpochSlotError {
    ZeroBlockingCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServerBlockingError {
    StaleLaunch,
    Saturated,
    WorkerFailed,
}

/// Per-process keyed identity derivation for authentication rate limiting.
///
/// Only the accepted transport peer is trusted. Ports are discarded, IPv4
/// mapped IPv6 addresses are canonicalized, and forwarding headers are never
/// read. The key keeps client identifiers unlinkable across host processes.
pub(crate) struct DirectPeerClientIdKey([u8; 32]);

impl DirectPeerClientIdKey {
    pub(crate) fn generate() -> Result<Self, DirectPeerClientIdKeyGenerationError> {
        let mut key = [0_u8; 32];
        getrandom::fill(&mut key).map_err(|_| DirectPeerClientIdKeyGenerationError)?;
        Ok(Self(key))
    }

    #[cfg(test)]
    const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// `untrusted_headers` is accepted only to make the trust boundary
    /// executable: forwarded and real-IP headers cannot influence identity.
    pub(crate) fn identify_request(
        &self,
        direct_peer: Option<SocketAddr>,
        _untrusted_headers: &HeaderMap,
    ) -> Result<u64, MissingDirectPeer> {
        let peer = direct_peer.ok_or(MissingDirectPeer)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0)
            .expect("a fixed-size key is valid for HMAC-SHA256");
        mac.update(b"qingyu/server/direct-peer-client-id/v1\0");
        match normalize_direct_peer(peer) {
            IpAddr::V4(address) => {
                mac.update(&[4]);
                mac.update(&address.octets());
            }
            IpAddr::V6(address) => {
                mac.update(&[6]);
                mac.update(&address.octets());
            }
        }
        let digest = mac.finalize().into_bytes();
        Ok(u64::from_le_bytes(
            digest[..8]
                .try_into()
                .expect("an HMAC-SHA256 digest contains eight bytes"),
        ))
    }
}

impl Drop for DirectPeerClientIdKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for DirectPeerClientIdKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DirectPeerClientIdKey([REDACTED])")
    }
}

fn normalize_direct_peer(peer: SocketAddr) -> IpAddr {
    match peer.ip() {
        IpAddr::V4(address) => IpAddr::V4(address),
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map_or(IpAddr::V6(address), IpAddr::V4),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MissingDirectPeer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DirectPeerClientIdKeyGenerationError;

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, SocketAddr},
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc, Arc,
        },
        time::Duration,
    };

    use axum::http::{HeaderMap, HeaderValue};

    use super::{
        DirectPeerClientIdKey, ServerBlockingError, ServerHostEpochSlot, ServerHostEpochSlotError,
    };
    use crate::config::KernelConfig;

    #[test]
    fn replacing_same_epoch_invalidates_the_old_arc_incarnation() {
        let config = KernelConfig::generate().unwrap();
        let slot = ServerHostEpochSlot::new(1).unwrap();
        let first = slot.replace(config.launch_epoch(), "first");
        let second = slot.replace(config.launch_epoch(), "second");

        assert!(!first.is_current());
        assert!(second.is_current());
        assert!(!slot.invalidate(&first));
        assert!(second.is_current());
        assert!(slot.invalidate(&second));
        assert!(!second.is_current());
    }

    #[test]
    fn replacing_launch_epoch_invalidates_the_old_lease() {
        let first_config = KernelConfig::generate().unwrap();
        let second_config = KernelConfig::generate().unwrap();
        let slot = ServerHostEpochSlot::new(1).unwrap();
        let first = slot.replace(first_config.launch_epoch(), "first");
        let second = slot.replace(second_config.launch_epoch(), "second");

        assert!(!first.is_current());
        assert!(second.is_current());
    }

    #[test]
    fn zero_blocking_capacity_is_rejected() {
        assert_eq!(
            ServerHostEpochSlot::<()>::new(0).unwrap_err(),
            ServerHostEpochSlotError::ZeroBlockingCapacity
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_capacity_is_shared_across_epoch_replacement() {
        let first_config = KernelConfig::generate().unwrap();
        let second_config = KernelConfig::generate().unwrap();
        let slot = ServerHostEpochSlot::new(1).unwrap();
        let first = slot.replace(first_config.launch_epoch(), "first");
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let first_task = tokio::spawn(async move {
            first
                .run_blocking(move |state| {
                    started_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    *state
                })
                .await
        });
        tokio::task::spawn_blocking(move || {
            started_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
        })
        .await
        .unwrap();

        let second = slot.replace(second_config.launch_epoch(), "second");
        let saturated = second.run_blocking(|state| *state).await;
        assert_eq!(saturated, Err(ServerBlockingError::Saturated));

        release_sender.send(()).unwrap();
        assert_eq!(
            first_task.await.unwrap(),
            Err(ServerBlockingError::StaleLaunch)
        );
        assert_eq!(second.run_blocking(|state| *state).await.unwrap(), "second");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_lease_never_starts_a_blocking_operation() {
        let first_config = KernelConfig::generate().unwrap();
        let second_config = KernelConfig::generate().unwrap();
        let slot = ServerHostEpochSlot::new(1).unwrap();
        let first = slot.replace(first_config.launch_epoch(), ());
        let second = slot.replace(second_config.launch_epoch(), ());
        let calls = Arc::new(AtomicUsize::new(0));
        let operation_calls = Arc::clone(&calls);

        assert_eq!(
            first
                .run_blocking(move |_| {
                    operation_calls.fetch_add(1, Ordering::SeqCst);
                })
                .await,
            Err(ServerBlockingError::StaleLaunch)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(second.is_current());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn result_from_a_replaced_epoch_is_dropped_after_one_execution() {
        struct DropProbe(Arc<AtomicUsize>);

        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let first_config = KernelConfig::generate().unwrap();
        let second_config = KernelConfig::generate().unwrap();
        let slot = ServerHostEpochSlot::new(1).unwrap();
        let first = slot.replace(first_config.launch_epoch(), ());
        let calls = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let operation_calls = Arc::clone(&calls);
        let result_drops = Arc::clone(&drops);
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let first_task = tokio::spawn(async move {
            first
                .run_blocking(move |_| {
                    operation_calls.fetch_add(1, Ordering::SeqCst);
                    started_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    DropProbe(result_drops)
                })
                .await
        });
        tokio::task::spawn_blocking(move || {
            started_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
        })
        .await
        .unwrap();

        let second = slot.replace(second_config.launch_epoch(), ());
        release_sender.send(()).unwrap();
        assert!(matches!(
            first_task.await.unwrap(),
            Err(ServerBlockingError::StaleLaunch)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(second.is_current());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelling_the_waiter_does_not_cancel_or_leak_the_blocking_settlement() {
        let config = KernelConfig::generate().unwrap();
        let slot = ServerHostEpochSlot::new(1).unwrap();
        let lease = slot.replace(config.launch_epoch(), ());
        let cancelled_lease = lease.clone();
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let (settled_sender, settled_receiver) = mpsc::sync_channel(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let operation_calls = Arc::clone(&calls);
        let waiter = tokio::spawn(async move {
            cancelled_lease
                .run_blocking(move |_| {
                    operation_calls.fetch_add(1, Ordering::SeqCst);
                    started_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    settled_sender.send(()).unwrap();
                })
                .await
        });
        tokio::task::spawn_blocking(move || {
            started_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
        })
        .await
        .unwrap();

        waiter.abort();
        assert_eq!(
            lease.run_blocking(|_| ()).await,
            Err(ServerBlockingError::Saturated)
        );
        release_sender.send(()).unwrap();
        tokio::task::spawn_blocking(move || {
            settled_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
        })
        .await
        .unwrap();

        let next_result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match lease.run_blocking(|_| 7).await {
                    Ok(result) => break result,
                    Err(ServerBlockingError::Saturated) => tokio::task::yield_now().await,
                    Err(error) => panic!("unexpected blocking result after settlement: {error:?}"),
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(next_result, 7);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn direct_peer_identity_ignores_ports_and_forwarding_headers() {
        let key = DirectPeerClientIdKey::from_bytes([7; 32]);
        let mut first_headers = HeaderMap::new();
        first_headers.insert(
            "forwarded",
            HeaderValue::from_static("for=198.51.100.10:9000"),
        );
        first_headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.11"));
        let mut second_headers = HeaderMap::new();
        second_headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.12"));
        let first_peer = SocketAddr::from((Ipv4Addr::new(192, 0, 2, 44), 1000));
        let second_peer = SocketAddr::from((Ipv4Addr::new(192, 0, 2, 44), 65000));

        assert_eq!(
            key.identify_request(Some(first_peer), &first_headers)
                .unwrap(),
            key.identify_request(Some(second_peer), &second_headers)
                .unwrap()
        );
    }

    #[test]
    fn ipv4_mapped_peer_has_the_same_identity_as_ipv4() {
        let key = DirectPeerClientIdKey::from_bytes([9; 32]);
        let headers = HeaderMap::new();
        let ipv4 = Ipv4Addr::new(203, 0, 113, 7);
        let mapped = ipv4.to_ipv6_mapped();

        assert_eq!(
            key.identify_request(Some(SocketAddr::from((ipv4, 443))), &headers)
                .unwrap(),
            key.identify_request(Some(SocketAddr::from((mapped, 8443))), &headers)
                .unwrap()
        );
    }

    #[test]
    fn client_identity_is_bound_to_the_process_key() {
        let first_key = DirectPeerClientIdKey::from_bytes([13; 32]);
        let second_key = DirectPeerClientIdKey::from_bytes([14; 32]);
        let headers = HeaderMap::new();
        let peer = SocketAddr::from((Ipv4Addr::new(198, 51, 100, 20), 443));

        assert_ne!(
            first_key.identify_request(Some(peer), &headers).unwrap(),
            second_key.identify_request(Some(peer), &headers).unwrap()
        );
    }

    #[test]
    fn missing_direct_peer_fails_closed_even_with_forwarding_headers() {
        let key = DirectPeerClientIdKey::from_bytes([11; 32]);
        let mut headers = HeaderMap::new();
        headers.insert("forwarded", HeaderValue::from_static("for=203.0.113.99"));
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.98"));

        assert_eq!(
            key.identify_request(None, &headers),
            Err(super::MissingDirectPeer)
        );
    }
}
