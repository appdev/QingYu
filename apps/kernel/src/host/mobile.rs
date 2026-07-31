//! In-process mobile Kernel host ownership.
//!
//! The platform shell supplies an already composed mobile runtime and its
//! lifecycle. This owner keeps transport and ownership policy inside the
//! Kernel crate: one active launch, an OS-assigned IPv4 loopback port, an
//! exact WebView origin, and launch-scoped bearer revocation.

use std::{
    collections::HashSet,
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard, Weak,
    },
    time::Duration,
};

use async_trait::async_trait;
use tokio::{net::TcpListener, sync::oneshot};
use zeroize::Zeroizing;

use crate::{
    api::{build_router, TransportPolicy},
    config::KernelLaunchEpoch,
    contract::HostProfile,
    runtime::KernelRuntime,
};

/// Flushes and closes the process-owned services for one mobile launch.
///
/// Platform code must adapt the real Kernel composition to this seam. A
/// credential store remains a [`crate::ports::KernelPorts`] concern; this
/// lifecycle must not be used as an in-memory credential-store substitute.
#[async_trait]
pub trait MobileKernelLifecycle: Send + Sync {
    async fn drain(&self) -> Result<(), MobileKernelDrainError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MobileKernelDrainError;

impl fmt::Display for MobileKernelDrainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("mobile Kernel lifecycle could not drain")
    }
}

impl std::error::Error for MobileKernelDrainError {}

/// One fully composed, fixed-workspace mobile Kernel launch.
pub struct MobileKernelLaunch {
    runtime: Arc<KernelRuntime>,
    lifecycle: Arc<dyn MobileKernelLifecycle>,
}

impl MobileKernelLaunch {
    pub fn new(runtime: Arc<KernelRuntime>, lifecycle: Arc<dyn MobileKernelLifecycle>) -> Self {
        Self { runtime, lifecycle }
    }
}

impl fmt::Debug for MobileKernelLaunch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MobileKernelLaunch([REDACTED])")
    }
}

/// Owns the only active in-process mobile Kernel transport.
pub struct MobileKernelHostOwner {
    inner: Arc<MobileKernelHostInner>,
}

static MOBILE_KERNEL_HOST_PROCESS_CLAIMED: AtomicBool = AtomicBool::new(false);

impl MobileKernelHostOwner {
    pub fn new(drain_timeout: Duration) -> Result<Self, MobileKernelHostError> {
        if drain_timeout.is_zero() {
            return Err(MobileKernelHostError::new(
                MobileKernelHostErrorKind::InvalidConfiguration,
            ));
        }
        let process_claim = Arc::new(MobileKernelHostProcessClaim::acquire()?);
        Ok(Self {
            inner: Arc::new(MobileKernelHostInner {
                drain_timeout,
                next_generation: AtomicU64::new(1),
                process_claim,
                slot: StdMutex::new(MobileKernelHostSlot {
                    phase: MobileKernelHostState::Idle,
                    retired_epochs: HashSet::new(),
                }),
            }),
        })
    }

    /// Starts one mobile runtime on `127.0.0.1:0`.
    ///
    /// `webview_origin` is treated as untrusted input and must be one exact
    /// header value. Wildcard origins are rejected by the shared transport
    /// policy. The returned endpoint owns the only intentional bearer exposure.
    pub async fn start(
        &self,
        launch: MobileKernelLaunch,
        webview_origin: &str,
    ) -> Result<MobileKernelEndpoint, MobileKernelHostError> {
        if launch.runtime.host_profile() != HostProfile::Mobile {
            return Err(MobileKernelHostError::new(
                MobileKernelHostErrorKind::UnsupportedProfile,
            ));
        }
        let epoch = *launch.runtime.launch_epoch();

        let generation = self
            .inner
            .next_generation
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(1)
            })
            .map_err(|_| {
                MobileKernelHostError::new(MobileKernelHostErrorKind::GenerationExhausted)
            })?;
        {
            let mut slot = lock_unpoisoned(&self.inner.slot);
            settle_completed_for_restart(&mut slot);
            if slot.retired_epochs.contains(&epoch) {
                return Err(MobileKernelHostError::new(
                    MobileKernelHostErrorKind::RetiredLaunch,
                ));
            }
            match &slot.phase {
                MobileKernelHostState::Idle => {
                    slot.phase = MobileKernelHostState::Starting { generation };
                }
                MobileKernelHostState::Starting { .. } | MobileKernelHostState::Running { .. } => {
                    return Err(MobileKernelHostError::new(
                        MobileKernelHostErrorKind::AlreadyActive,
                    ));
                }
                MobileKernelHostState::Stopping { .. } => {
                    return Err(MobileKernelHostError::new(
                        MobileKernelHostErrorKind::Stopping,
                    ));
                }
            }
        }
        let mut reservation = MobileKernelStartReservation {
            inner: Arc::downgrade(&self.inner),
            generation,
            committed: false,
        };

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| MobileKernelHostError::new(MobileKernelHostErrorKind::BindUnavailable))?;
        let address = listener
            .local_addr()
            .map_err(|_| MobileKernelHostError::new(MobileKernelHostErrorKind::BindUnavailable))?;
        if address.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) || address.port() == 0 {
            return Err(MobileKernelHostError::new(
                MobileKernelHostErrorKind::UnsafeListener,
            ));
        }
        let authority = address.to_string();
        let policy = TransportPolicy::loopback(&authority, webview_origin)
            .map_err(|_| MobileKernelHostError::new(MobileKernelHostErrorKind::InvalidOrigin))?;

        let bearer = Zeroizing::new(launch.runtime.expose_native_launch_credential().to_owned());
        let completion = Arc::new(MobileKernelCompletion::new());
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let router = build_router(launch.runtime, policy);
        let lifecycle = launch.lifecycle;
        let task_completion = completion.clone();
        let task_process_claim = self.inner.process_claim.clone();
        tokio::spawn(async move {
            let serve_result = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _shutdown = shutdown_receiver.await;
                })
                .await
                .map_err(|_| MobileKernelHostErrorKind::ServerFailed);
            let drain_result = lifecycle
                .drain()
                .await
                .map_err(|_| MobileKernelHostErrorKind::DrainFailed);
            task_completion.finish(serve_result.and(drain_result));
            drop(task_process_claim);
        });

        {
            let mut slot = lock_unpoisoned(&self.inner.slot);
            if !matches!(
                &slot.phase,
                MobileKernelHostState::Starting {
                    generation: current,
                } if *current == generation
            ) {
                let _sent = shutdown_sender.send(());
                return Err(MobileKernelHostError::new(
                    MobileKernelHostErrorKind::LaunchSuperseded,
                ));
            }
            slot.phase = MobileKernelHostState::Running {
                completion: completion.clone(),
                epoch,
                generation,
                shutdown: Some(shutdown_sender),
            };
        }
        reservation.committed = true;

        Ok(MobileKernelEndpoint {
            address,
            bearer,
            completion,
            epoch,
            generation,
            owner: Arc::downgrade(&self.inner),
        })
    }

    /// Revokes the endpoint, stops new requests, and waits up to the configured
    /// bound for the shared lifecycle drain. Cancellation of one waiter does
    /// not cancel the server or lifecycle settlement.
    pub async fn stop(&self) -> Result<MobileKernelStopDisposition, MobileKernelHostError> {
        let (completion, generation) = {
            let mut slot = lock_unpoisoned(&self.inner.slot);
            let phase = std::mem::replace(&mut slot.phase, MobileKernelHostState::Idle);
            match phase {
                MobileKernelHostState::Idle => {
                    return Ok(MobileKernelStopDisposition::AlreadyStopped);
                }
                MobileKernelHostState::Starting { generation } => {
                    slot.phase = MobileKernelHostState::Starting { generation };
                    return Err(MobileKernelHostError::new(
                        MobileKernelHostErrorKind::AlreadyActive,
                    ));
                }
                MobileKernelHostState::Running {
                    completion,
                    epoch,
                    generation,
                    mut shutdown,
                } => {
                    let shutdown = shutdown.take();
                    slot.retired_epochs.insert(epoch);
                    slot.phase = MobileKernelHostState::Stopping {
                        completion: completion.clone(),
                        generation,
                    };
                    if let Some(shutdown) = shutdown {
                        let _sent = shutdown.send(());
                    }
                    (completion, generation)
                }
                MobileKernelHostState::Stopping {
                    completion,
                    generation,
                } => {
                    slot.phase = MobileKernelHostState::Stopping {
                        completion: completion.clone(),
                        generation,
                    };
                    (completion, generation)
                }
            }
        };

        let result = tokio::time::timeout(self.inner.drain_timeout, completion.wait())
            .await
            .map_err(|_| MobileKernelHostError::new(MobileKernelHostErrorKind::DrainTimedOut))?;
        {
            let mut slot = lock_unpoisoned(&self.inner.slot);
            if matches!(
                &slot.phase,
                MobileKernelHostState::Stopping {
                    generation: current,
                    ..
                } if *current == generation
            ) {
                slot.phase = MobileKernelHostState::Idle;
            }
        }
        result
            .map(|()| MobileKernelStopDisposition::Stopped)
            .map_err(MobileKernelHostError::new)
    }
}

impl Drop for MobileKernelHostOwner {
    fn drop(&mut self) {
        let mut slot = lock_unpoisoned(&self.inner.slot);
        let phase = std::mem::replace(&mut slot.phase, MobileKernelHostState::Idle);
        match phase {
            MobileKernelHostState::Running {
                completion,
                epoch,
                generation,
                mut shutdown,
            } => {
                slot.retired_epochs.insert(epoch);
                slot.phase = MobileKernelHostState::Stopping {
                    completion,
                    generation,
                };
                if let Some(shutdown) = shutdown.take() {
                    let _sent = shutdown.send(());
                }
            }
            phase => slot.phase = phase,
        }
    }
}

impl fmt::Debug for MobileKernelHostOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MobileKernelHostOwner(..)")
    }
}

/// Launch-scoped address and bearer lease for the shared WebView KernelClient.
pub struct MobileKernelEndpoint {
    address: SocketAddr,
    bearer: Zeroizing<String>,
    completion: Arc<MobileKernelCompletion>,
    epoch: KernelLaunchEpoch,
    generation: u64,
    owner: Weak<MobileKernelHostInner>,
}

impl MobileKernelEndpoint {
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn events_url(&self) -> String {
        format!("ws://{}/api/v1/events", self.address)
    }

    /// Exposes the bearer only while this exact owner incarnation is current.
    pub fn bearer(&self) -> Result<&str, MobileKernelCredentialRevoked> {
        if self.is_current() {
            Ok(self.bearer.as_str())
        } else {
            Err(MobileKernelCredentialRevoked)
        }
    }

    pub fn is_current(&self) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        if self.completion.result().is_some() {
            return false;
        }
        let current = matches!(
            &lock_unpoisoned(&owner.slot).phase,
            MobileKernelHostState::Running {
                completion,
                epoch,
                generation,
                ..
            } if *generation == self.generation
                && *epoch == self.epoch
                && Arc::ptr_eq(completion, &self.completion)
        );
        current
    }
}

impl fmt::Debug for MobileKernelEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MobileKernelEndpoint([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MobileKernelCredentialRevoked;

impl fmt::Display for MobileKernelCredentialRevoked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("mobile Kernel credential lease is no longer current")
    }
}

impl std::error::Error for MobileKernelCredentialRevoked {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MobileKernelStopDisposition {
    Stopped,
    AlreadyStopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MobileKernelHostErrorKind {
    InvalidConfiguration,
    ProcessOwnerClaimed,
    UnsupportedProfile,
    RetiredLaunch,
    AlreadyActive,
    Stopping,
    GenerationExhausted,
    BindUnavailable,
    UnsafeListener,
    InvalidOrigin,
    LaunchSuperseded,
    ServerFailed,
    DrainFailed,
    DrainTimedOut,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct MobileKernelHostError {
    kind: MobileKernelHostErrorKind,
}

impl MobileKernelHostError {
    const fn new(kind: MobileKernelHostErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(self) -> MobileKernelHostErrorKind {
        self.kind
    }
}

impl fmt::Debug for MobileKernelHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileKernelHostError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for MobileKernelHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            MobileKernelHostErrorKind::InvalidConfiguration => {
                "mobile Kernel host configuration is invalid"
            }
            MobileKernelHostErrorKind::ProcessOwnerClaimed => {
                "mobile Kernel host process owner is already claimed"
            }
            MobileKernelHostErrorKind::UnsupportedProfile => {
                "mobile Kernel host requires a mobile runtime"
            }
            MobileKernelHostErrorKind::RetiredLaunch => "mobile Kernel launch identity is retired",
            MobileKernelHostErrorKind::AlreadyActive => "mobile Kernel host is already active",
            MobileKernelHostErrorKind::Stopping => "mobile Kernel host is stopping",
            MobileKernelHostErrorKind::GenerationExhausted => {
                "mobile Kernel host generation is unavailable"
            }
            MobileKernelHostErrorKind::BindUnavailable
            | MobileKernelHostErrorKind::UnsafeListener => {
                "mobile Kernel loopback listener is unavailable"
            }
            MobileKernelHostErrorKind::InvalidOrigin => "mobile Kernel WebView origin is invalid",
            MobileKernelHostErrorKind::LaunchSuperseded => "mobile Kernel launch was superseded",
            MobileKernelHostErrorKind::ServerFailed => "mobile Kernel transport failed",
            MobileKernelHostErrorKind::DrainFailed => "mobile Kernel lifecycle drain failed",
            MobileKernelHostErrorKind::DrainTimedOut => "mobile Kernel lifecycle drain timed out",
        })
    }
}

impl std::error::Error for MobileKernelHostError {}

struct MobileKernelHostInner {
    drain_timeout: Duration,
    next_generation: AtomicU64,
    process_claim: Arc<MobileKernelHostProcessClaim>,
    slot: StdMutex<MobileKernelHostSlot>,
}

struct MobileKernelHostSlot {
    phase: MobileKernelHostState,
    retired_epochs: HashSet<KernelLaunchEpoch>,
}

struct MobileKernelHostProcessClaim;

impl MobileKernelHostProcessClaim {
    fn acquire() -> Result<Self, MobileKernelHostError> {
        MOBILE_KERNEL_HOST_PROCESS_CLAIMED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                MobileKernelHostError::new(MobileKernelHostErrorKind::ProcessOwnerClaimed)
            })?;
        Ok(Self)
    }
}

impl Drop for MobileKernelHostProcessClaim {
    fn drop(&mut self) {
        let was_claimed = MOBILE_KERNEL_HOST_PROCESS_CLAIMED.swap(false, Ordering::SeqCst);
        debug_assert!(was_claimed);
    }
}

enum MobileKernelHostState {
    Idle,
    Starting {
        generation: u64,
    },
    Running {
        completion: Arc<MobileKernelCompletion>,
        epoch: KernelLaunchEpoch,
        generation: u64,
        shutdown: Option<oneshot::Sender<()>>,
    },
    Stopping {
        completion: Arc<MobileKernelCompletion>,
        generation: u64,
    },
}

struct MobileKernelStartReservation {
    committed: bool,
    generation: u64,
    inner: Weak<MobileKernelHostInner>,
}

impl Drop for MobileKernelStartReservation {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let mut slot = lock_unpoisoned(&inner.slot);
        if matches!(
            &slot.phase,
            MobileKernelHostState::Starting { generation } if *generation == self.generation
        ) {
            slot.phase = MobileKernelHostState::Idle;
        }
    }
}

struct MobileKernelCompletion {
    notify: tokio::sync::Notify,
    result: StdMutex<Option<Result<(), MobileKernelHostErrorKind>>>,
}

impl MobileKernelCompletion {
    fn new() -> Self {
        Self {
            notify: tokio::sync::Notify::new(),
            result: StdMutex::new(None),
        }
    }

    fn finish(&self, result: Result<(), MobileKernelHostErrorKind>) {
        let mut current = lock_unpoisoned(&self.result);
        if current.is_none() {
            *current = Some(result);
            drop(current);
            self.notify.notify_waiters();
        }
    }

    fn result(&self) -> Option<Result<(), MobileKernelHostErrorKind>> {
        *lock_unpoisoned(&self.result)
    }

    async fn wait(&self) -> Result<(), MobileKernelHostErrorKind> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(result) = self.result() {
                return result;
            }
            notified.await;
        }
    }
}

fn settle_completed_for_restart(slot: &mut MobileKernelHostSlot) {
    let completed_epoch = match &slot.phase {
        MobileKernelHostState::Running {
            completion, epoch, ..
        } if completion.result().is_some() => Some(*epoch),
        MobileKernelHostState::Stopping { completion, .. } if completion.result().is_some() => None,
        MobileKernelHostState::Idle
        | MobileKernelHostState::Starting { .. }
        | MobileKernelHostState::Running { .. }
        | MobileKernelHostState::Stopping { .. } => return,
    };
    if let Some(epoch) = completed_epoch {
        slot.retired_epochs.insert(epoch);
    }
    slot.phase = MobileKernelHostState::Idle;
}

fn lock_unpoisoned<T>(lock: &StdMutex<T>) -> StdMutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
