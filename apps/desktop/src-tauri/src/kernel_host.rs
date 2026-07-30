//! Desktop Kernel child lifecycle state machine.
//!
//! The supervisor keeps endpoint publication and the same-generation parent
//! credential lease in one Ready state. Every failure, stop, and Drop path
//! revokes that lease while synchronously retaining child-process ownership.
//! Production registration remains a separate atomic legacy-writer cutover.

#![cfg_attr(not(test), allow(dead_code))]

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use qingyu_kernel::{
    config::NativeLaunchCredential,
    contract::InstanceId,
    host::native::{NativeHostReady, NativeHostStart, NativeHostWorkspaceState},
};
use tokio::{
    sync::{mpsc, oneshot, watch, Mutex as AsyncMutex},
    task::JoinHandle,
    time::{sleep, timeout},
};

type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait KernelProcessFactory: Send + Sync + 'static {
    /// Spawns one child without waiting for readiness.
    ///
    /// The returned pending process owns the parent credential, parses stdout
    /// with the shared bounded protocol, continuously drains a bounded stderr
    /// tail, and performs the authenticated health probe before returning
    /// [`ReadyEvidence`].
    fn spawn(
        &self,
        launch: NativeKernelLaunch,
        generation: u64,
        ownership: &KernelOwnership,
    ) -> Result<Box<dyn PendingKernel>, KernelHostFailure>;
}

pub(crate) struct NativeKernelLaunch {
    startup: NativeHostStart,
    credential: NativeKernelCredentialLease,
}

impl NativeKernelLaunch {
    pub(crate) fn desktop(
        workspace_root: std::path::PathBuf,
        app_data_root: std::path::PathBuf,
        cache_root: std::path::PathBuf,
        workspace_state: NativeHostWorkspaceState,
        origin: String,
    ) -> Result<Self, KernelHostFailure> {
        let credential =
            NativeLaunchCredential::generate().map_err(|_| KernelHostFailure::Spawn)?;
        let child_credential =
            NativeLaunchCredential::from_secret(credential.expose_secret().to_owned())
                .map_err(|_| KernelHostFailure::Spawn)?;
        Ok(Self {
            startup: NativeHostStart::desktop(
                workspace_root,
                app_data_root,
                cache_root,
                workspace_state,
                origin,
                child_credential,
            ),
            credential: NativeKernelCredentialLease::new(credential),
        })
    }

    pub(crate) fn into_parts(self) -> (NativeHostStart, NativeKernelCredentialLease) {
        (self.startup, self.credential)
    }
}

#[derive(Clone)]
pub(crate) struct NativeKernelCredentialLease {
    credential: Arc<Mutex<Option<NativeLaunchCredential>>>,
}

impl NativeKernelCredentialLease {
    fn new(credential: NativeLaunchCredential) -> Self {
        Self {
            credential: Arc::new(Mutex::new(Some(credential))),
        }
    }

    pub(crate) fn with_secret<T>(
        &self,
        use_secret: impl FnOnce(&str) -> T,
    ) -> Result<T, KernelHostFailure> {
        let credential = self
            .credential
            .lock()
            .map_err(|_| KernelHostFailure::Cancelled)?;
        let credential = credential.as_ref().ok_or(KernelHostFailure::Cancelled)?;
        Ok(use_secret(credential.expose_secret()))
    }

    pub(crate) fn revoke(&self) {
        match self.credential.lock() {
            Ok(mut credential) => {
                credential.take();
            }
            Err(poisoned) => {
                poisoned.into_inner().take();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn is_available(&self) -> bool {
        self.credential
            .lock()
            .is_ok_and(|credential| credential.is_some())
    }
}

impl std::fmt::Debug for NativeKernelCredentialLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeKernelCredentialLease([REDACTED])")
    }
}

/// Synchronous last-resort ownership for one spawned child.
///
/// The real process driver must kill and reap from this method without relying
/// on the Tokio runtime. This is what makes dropping the supervisor fail closed
/// even when its actor future has not observed cancellation yet. Implementors
/// must serialize this call with their async cleanup operations. If termination
/// or reaping cannot be confirmed, the implementation must abort the desktop
/// host process instead of returning with uncertain ownership.
pub(crate) trait SynchronousKernelGuard: Send + Sync + 'static {
    fn terminate_and_reap_or_abort(&self);
}

struct OwnedKernelGuard {
    generation: u64,
    guard: Arc<dyn SynchronousKernelGuard>,
}

#[derive(Default)]
struct KernelOwnershipState {
    closed: bool,
    active: Option<OwnedKernelGuard>,
}

#[derive(Default)]
pub(crate) struct KernelOwnership {
    state: Mutex<KernelOwnershipState>,
}

impl KernelOwnership {
    /// Acquires the exclusive spawn boundary.
    ///
    /// A factory must hold this permit across OS process creation and register
    /// its synchronous guard before releasing it. Therefore `close_and_terminate`
    /// either prevents the spawn or waits until it can terminate the new child.
    pub(crate) fn begin_spawn(
        &self,
        generation: u64,
    ) -> Result<KernelSpawnPermit<'_>, KernelHostFailure> {
        let state = self.state.lock().map_err(|_| KernelHostFailure::Spawn)?;
        if state.closed {
            return Err(KernelHostFailure::Cancelled);
        }
        if state.active.is_some() {
            return Err(KernelHostFailure::Busy);
        }
        Ok(KernelSpawnPermit { generation, state })
    }

    fn owns(&self, generation: u64) -> bool {
        self.state.lock().is_ok_and(|state| {
            state.active.as_ref().map(|owned| owned.generation) == Some(generation)
        })
    }

    fn clear_reaped(&self, generation: u64) {
        if let Ok(mut state) = self.state.lock() {
            if state.active.as_ref().map(|owned| owned.generation) == Some(generation) {
                state.active = None;
            }
        }
    }

    fn close_and_terminate(&self) {
        let active = match self.state.lock() {
            Ok(mut state) => {
                state.closed = true;
                state.active.take()
            }
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.closed = true;
                state.active.take()
            }
        };
        if let Some(owned) = active {
            owned.guard.terminate_and_reap_or_abort();
        }
    }

    fn terminate_active(&self) {
        let active = match self.state.lock() {
            Ok(mut state) => state.active.take(),
            Err(poisoned) => poisoned.into_inner().active.take(),
        };
        if let Some(owned) = active {
            owned.guard.terminate_and_reap_or_abort();
        }
    }
}

pub(crate) struct KernelSpawnPermit<'a> {
    generation: u64,
    state: MutexGuard<'a, KernelOwnershipState>,
}

impl KernelSpawnPermit<'_> {
    pub(crate) fn register(mut self, guard: Arc<dyn SynchronousKernelGuard>) {
        self.state.active = Some(OwnedKernelGuard {
            generation: self.generation,
            guard,
        });
    }
}

/// A spawned child which has not yet passed authenticated readiness.
///
/// Implementations must terminate the child synchronously from `Drop` while
/// armed. Async cleanup is best effort; the Drop invariant is the final guard
/// when the actor task itself is cancelled.
pub(crate) trait PendingKernel: Send {
    fn wait_ready(&mut self) -> HostFuture<'_, Result<ReadyEvidence, KernelHostFailure>>;
    fn credential_lease(&self) -> NativeKernelCredentialLease;
    fn cancel_and_reap(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>>;
    fn force_kill_and_reap(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>>;
    fn into_running(self: Box<Self>) -> Result<Box<dyn RunningKernel>, KernelHostFailure>;
}

/// An authenticated running child owned exclusively by the supervisor actor.
///
/// Implementations have the same armed-Drop fail-closed requirement as
/// [`PendingKernel`].
pub(crate) trait RunningKernel: Send {
    fn wait_exit(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>>;
    fn shutdown_and_reap(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>>;
    fn force_kill_and_reap(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>>;
}

#[derive(Clone, Debug)]
pub(crate) struct ReadyEvidence {
    pub(crate) ready: NativeHostReady,
    pub(crate) authenticated_instance: InstanceId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KernelEndpoint {
    pub(crate) generation: u64,
    pub(crate) port: u16,
    pub(crate) instance_id: InstanceId,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeKernelAccess {
    pub(crate) endpoint: KernelEndpoint,
    pub(crate) credential: NativeKernelCredentialLease,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KernelHostPhase {
    Dormant,
    Starting,
    Ready,
    Stopping,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KernelHostFailure {
    Busy,
    Spawn,
    StartupTimeout,
    Protocol,
    EarlyExit,
    IdentityMismatch,
    Cancelled,
    StopFailed,
    UnexpectedExit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KernelHostSnapshot {
    pub(crate) phase: KernelHostPhase,
    pub(crate) generation: u64,
    pub(crate) endpoint: Option<KernelEndpoint>,
    pub(crate) failure: Option<KernelHostFailure>,
}

impl KernelHostSnapshot {
    const fn dormant() -> Self {
        Self {
            phase: KernelHostPhase::Dormant,
            generation: 0,
            endpoint: None,
            failure: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct KernelHostTimeouts {
    startup: Duration,
    graceful_stop: Duration,
    force_reap: Duration,
}

impl KernelHostTimeouts {
    pub(crate) const fn uniform(duration: Duration) -> Self {
        Self {
            startup: duration,
            graceful_stop: duration,
            force_reap: duration,
        }
    }
}

pub(crate) struct KernelHostSupervisor {
    starts: mpsc::Sender<StartCommand>,
    stops: mpsc::UnboundedSender<oneshot::Sender<Result<(), KernelHostFailure>>>,
    enqueue_gate: Arc<AsyncMutex<()>>,
    snapshots: watch::Receiver<KernelHostSnapshot>,
    ownership: Arc<KernelOwnership>,
    actor: JoinHandle<()>,
}

impl KernelHostSupervisor {
    pub(crate) fn new(
        factory: Arc<dyn KernelProcessFactory>,
        timeouts: KernelHostTimeouts,
    ) -> Self {
        let (starts, start_receiver) = mpsc::channel(8);
        let (stops, stop_receiver) = mpsc::unbounded_channel();
        let (snapshot_sender, snapshots) = watch::channel(KernelHostSnapshot::dormant());
        let ownership = Arc::new(KernelOwnership::default());
        let enqueue_gate = Arc::new(AsyncMutex::new(()));
        let actor = tokio::spawn(run_actor(
            start_receiver,
            stop_receiver,
            snapshot_sender,
            factory,
            Arc::clone(&ownership),
            Arc::clone(&enqueue_gate),
            timeouts,
        ));
        Self {
            starts,
            stops,
            enqueue_gate,
            snapshots,
            ownership,
            actor,
        }
    }

    pub(crate) async fn start(
        &self,
        launch: NativeKernelLaunch,
    ) -> Result<NativeKernelAccess, KernelHostFailure> {
        let (response, receiver) = oneshot::channel();
        {
            let _enqueue_guard = self.enqueue_gate.lock().await;
            self.starts
                .try_send(StartCommand { launch, response })
                .map_err(|error| match error {
                    mpsc::error::TrySendError::Full(_) => KernelHostFailure::Busy,
                    mpsc::error::TrySendError::Closed(_) => KernelHostFailure::Spawn,
                })?;
        }
        receiver.await.map_err(|_| KernelHostFailure::Spawn)?
    }

    pub(crate) async fn stop(&self) -> Result<(), KernelHostFailure> {
        let (response, receiver) = oneshot::channel();
        {
            let _enqueue_guard = self.enqueue_gate.lock().await;
            self.stops
                .send(response)
                .map_err(|_| KernelHostFailure::StopFailed)?;
        }
        receiver.await.map_err(|_| KernelHostFailure::StopFailed)?
    }

    pub(crate) fn snapshot(&self) -> KernelHostSnapshot {
        *self.snapshots.borrow()
    }
}

impl Drop for KernelHostSupervisor {
    fn drop(&mut self) {
        self.ownership.close_and_terminate();
        self.actor.abort();
    }
}

struct StartCommand {
    launch: NativeKernelLaunch,
    response: oneshot::Sender<Result<NativeKernelAccess, KernelHostFailure>>,
}

struct ReadyKernel {
    access: NativeKernelAccess,
    process: Box<dyn RunningKernel>,
}

enum ActorMode {
    Idle,
    Ready(ReadyKernel),
}

async fn run_actor(
    mut starts: mpsc::Receiver<StartCommand>,
    mut stops: mpsc::UnboundedReceiver<oneshot::Sender<Result<(), KernelHostFailure>>>,
    snapshots: watch::Sender<KernelHostSnapshot>,
    factory: Arc<dyn KernelProcessFactory>,
    ownership: Arc<KernelOwnership>,
    enqueue_gate: Arc<AsyncMutex<()>>,
    timeouts: KernelHostTimeouts,
) {
    let mut generation = 0_u64;
    let mut mode = ActorMode::Idle;
    loop {
        mode = match mode {
            ActorMode::Idle => {
                enum IdleCommand {
                    Start(Box<StartCommand>),
                    Stop(oneshot::Sender<Result<(), KernelHostFailure>>),
                }
                let command = tokio::select! {
                    biased;
                    response = stops.recv() => match response {
                        Some(response) => IdleCommand::Stop(response),
                        None => return,
                    },
                    command = starts.recv() => match command {
                        Some(command) => IdleCommand::Start(Box::new(command)),
                        None => return,
                    },
                };
                match command {
                    IdleCommand::Start(command) => {
                        let StartCommand { launch, response } = *command;
                        generation = generation.saturating_add(1);
                        match start_transition(
                            &mut starts,
                            &mut stops,
                            &snapshots,
                            factory.as_ref(),
                            ownership.as_ref(),
                            enqueue_gate.as_ref(),
                            timeouts,
                            generation,
                            launch,
                            response,
                        )
                        .await
                        {
                            Some(ready) => ActorMode::Ready(ready),
                            None => ActorMode::Idle,
                        }
                    }
                    IdleCommand::Stop(response) => {
                        ownership.terminate_active();
                        snapshots.send_replace(KernelHostSnapshot {
                            phase: KernelHostPhase::Dormant,
                            generation,
                            endpoint: None,
                            failure: None,
                        });
                        finish_stop_barrier(&mut starts, enqueue_gate.as_ref(), response, Ok(()))
                            .await;
                        ActorMode::Idle
                    }
                }
            }
            ActorMode::Ready(ready) => {
                match ready_transition(
                    &mut starts,
                    &mut stops,
                    &snapshots,
                    ownership.as_ref(),
                    enqueue_gate.as_ref(),
                    timeouts,
                    ready,
                )
                .await
                {
                    Some(ready) => ActorMode::Ready(ready),
                    None => ActorMode::Idle,
                }
            }
        };
    }
}

#[allow(clippy::too_many_arguments)]
async fn start_transition(
    starts: &mut mpsc::Receiver<StartCommand>,
    stops: &mut mpsc::UnboundedReceiver<oneshot::Sender<Result<(), KernelHostFailure>>>,
    snapshots: &watch::Sender<KernelHostSnapshot>,
    factory: &dyn KernelProcessFactory,
    ownership: &KernelOwnership,
    enqueue_gate: &AsyncMutex<()>,
    timeouts: KernelHostTimeouts,
    generation: u64,
    launch: NativeKernelLaunch,
    response: oneshot::Sender<Result<NativeKernelAccess, KernelHostFailure>>,
) -> Option<ReadyKernel> {
    snapshots.send_replace(KernelHostSnapshot {
        phase: KernelHostPhase::Starting,
        generation,
        endpoint: None,
        failure: None,
    });
    let mut pending = match factory.spawn(launch, generation, ownership) {
        Ok(pending) => pending,
        Err(error) => {
            publish_failure(snapshots, generation, error);
            let _send_result = response.send(Err(error));
            return None;
        }
    };
    if !ownership.owns(generation) {
        publish_failure(snapshots, generation, KernelHostFailure::Spawn);
        let _send_result = response.send(Err(KernelHostFailure::Spawn));
        return None;
    }

    enum StartWait {
        Ready(Result<ReadyEvidence, KernelHostFailure>),
        Timeout,
        Stop(oneshot::Sender<Result<(), KernelHostFailure>>),
        Closed,
    }

    let wait = {
        let ready = pending.wait_ready();
        tokio::pin!(ready);
        let deadline = sleep(timeouts.startup);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                biased;
                response = stops.recv() => match response {
                    Some(response) => break StartWait::Stop(response),
                    None => break StartWait::Closed,
                },
                result = &mut ready => break StartWait::Ready(result),
                _expired = &mut deadline => break StartWait::Timeout,
                command = starts.recv() => match command {
                    Some(StartCommand { response, .. }) => {
                        let _send_result = response.send(Err(KernelHostFailure::Busy));
                    }
                    None => break StartWait::Closed,
                },
            }
        }
    };

    let evidence = match wait {
        StartWait::Ready(Ok(evidence)) => evidence,
        StartWait::Ready(Err(error)) => {
            let reported = cleanup_pending(&mut *pending, ownership, generation, timeouts)
                .await
                .err()
                .unwrap_or(error);
            publish_failure(snapshots, generation, reported);
            let _send_result = response.send(Err(reported));
            return None;
        }
        StartWait::Timeout => {
            let reported = cleanup_pending(&mut *pending, ownership, generation, timeouts)
                .await
                .err()
                .unwrap_or(KernelHostFailure::StartupTimeout);
            publish_failure(snapshots, generation, reported);
            let _send_result = response.send(Err(reported));
            return None;
        }
        StartWait::Stop(stop_response) => {
            let stopped = cleanup_pending(&mut *pending, ownership, generation, timeouts).await;
            match stopped {
                Ok(()) => {
                    snapshots.send_replace(KernelHostSnapshot {
                        phase: KernelHostPhase::Dormant,
                        generation,
                        endpoint: None,
                        failure: None,
                    });
                }
                Err(error) => publish_failure(snapshots, generation, error),
            }
            let _send_result = response.send(Err(KernelHostFailure::Cancelled));
            finish_stop_barrier(starts, enqueue_gate, stop_response, stopped).await;
            return None;
        }
        StartWait::Closed => {
            let _cleanup_result =
                cleanup_pending(&mut *pending, ownership, generation, timeouts).await;
            return None;
        }
    };

    if evidence.ready.instance_id() != evidence.authenticated_instance {
        let reported = cleanup_pending(&mut *pending, ownership, generation, timeouts)
            .await
            .err()
            .unwrap_or(KernelHostFailure::IdentityMismatch);
        publish_failure(snapshots, generation, reported);
        let _send_result = response.send(Err(reported));
        return None;
    }
    let endpoint = KernelEndpoint {
        generation,
        port: evidence.ready.port(),
        instance_id: evidence.authenticated_instance,
    };
    let access = NativeKernelAccess {
        endpoint,
        credential: pending.credential_lease(),
    };
    let process = match pending.into_running() {
        Ok(process) => process,
        Err(error) => {
            publish_failure(snapshots, generation, error);
            let _send_result = response.send(Err(error));
            return None;
        }
    };
    snapshots.send_replace(KernelHostSnapshot {
        phase: KernelHostPhase::Ready,
        generation,
        endpoint: Some(endpoint),
        failure: None,
    });
    let _send_result = response.send(Ok(access.clone()));
    Some(ReadyKernel { access, process })
}

async fn ready_transition(
    starts: &mut mpsc::Receiver<StartCommand>,
    stops: &mut mpsc::UnboundedReceiver<oneshot::Sender<Result<(), KernelHostFailure>>>,
    snapshots: &watch::Sender<KernelHostSnapshot>,
    ownership: &KernelOwnership,
    enqueue_gate: &AsyncMutex<()>,
    timeouts: KernelHostTimeouts,
    mut ready: ReadyKernel,
) -> Option<ReadyKernel> {
    enum ReadyWait {
        Exit(Result<(), KernelHostFailure>),
        Stop(oneshot::Sender<Result<(), KernelHostFailure>>),
        Closed,
    }

    let wait = {
        let exited = ready.process.wait_exit();
        tokio::pin!(exited);
        loop {
            tokio::select! {
                biased;
                response = stops.recv() => match response {
                    Some(response) => break ReadyWait::Stop(response),
                    None => break ReadyWait::Closed,
                },
                result = &mut exited => break ReadyWait::Exit(result),
                command = starts.recv() => match command {
                    Some(StartCommand { response, .. }) => {
                        let _send_result = response.send(Err(KernelHostFailure::Busy));
                    }
                    None => break ReadyWait::Closed,
                },
            }
        }
    };

    match wait {
        ReadyWait::Exit(Ok(())) => {
            ready.access.credential.revoke();
            ownership.clear_reaped(ready.access.endpoint.generation);
            publish_failure(
                snapshots,
                ready.access.endpoint.generation,
                KernelHostFailure::UnexpectedExit,
            );
        }
        ReadyWait::Exit(Err(error)) => {
            let reported = force_reap_running(
                &mut *ready.process,
                ownership,
                ready.access.endpoint.generation,
                timeouts,
            )
            .await
            .err()
            .unwrap_or(error);
            publish_failure(snapshots, ready.access.endpoint.generation, reported);
        }
        ReadyWait::Stop(response) => {
            snapshots.send_replace(KernelHostSnapshot {
                phase: KernelHostPhase::Stopping,
                generation: ready.access.endpoint.generation,
                endpoint: None,
                failure: None,
            });
            let result = stop_running(
                &mut *ready.process,
                ownership,
                ready.access.endpoint.generation,
                timeouts,
            )
            .await;
            match result {
                Ok(()) => {
                    snapshots.send_replace(KernelHostSnapshot {
                        phase: KernelHostPhase::Dormant,
                        generation: ready.access.endpoint.generation,
                        endpoint: None,
                        failure: None,
                    });
                }
                Err(error) => publish_failure(snapshots, ready.access.endpoint.generation, error),
            };
            finish_stop_barrier(starts, enqueue_gate, response, result).await;
        }
        ReadyWait::Closed => {
            let _cleanup_result = stop_running(
                &mut *ready.process,
                ownership,
                ready.access.endpoint.generation,
                timeouts,
            )
            .await;
        }
    }
    None
}

async fn finish_stop_barrier(
    starts: &mut mpsc::Receiver<StartCommand>,
    enqueue_gate: &AsyncMutex<()>,
    response: oneshot::Sender<Result<(), KernelHostFailure>>,
    result: Result<(), KernelHostFailure>,
) {
    let _enqueue_guard = enqueue_gate.lock().await;
    while let Ok(StartCommand { response, .. }) = starts.try_recv() {
        let _send_result = response.send(Err(KernelHostFailure::Cancelled));
    }
    let _send_result = response.send(result);
}

async fn cleanup_pending(
    pending: &mut dyn PendingKernel,
    ownership: &KernelOwnership,
    generation: u64,
    timeouts: KernelHostTimeouts,
) -> Result<(), KernelHostFailure> {
    let result = match timeout(timeouts.graceful_stop, pending.cancel_and_reap()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => timeout(timeouts.force_reap, pending.force_kill_and_reap())
            .await
            .map_err(|_| KernelHostFailure::StopFailed)?,
    };
    if result.is_ok() {
        ownership.clear_reaped(generation);
    }
    result
}

async fn stop_running(
    running: &mut dyn RunningKernel,
    ownership: &KernelOwnership,
    generation: u64,
    timeouts: KernelHostTimeouts,
) -> Result<(), KernelHostFailure> {
    let result = match timeout(timeouts.graceful_stop, running.shutdown_and_reap()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => timeout(timeouts.force_reap, running.force_kill_and_reap())
            .await
            .map_err(|_| KernelHostFailure::StopFailed)?,
    };
    if result.is_ok() {
        ownership.clear_reaped(generation);
    }
    result
}

async fn force_reap_running(
    running: &mut dyn RunningKernel,
    ownership: &KernelOwnership,
    generation: u64,
    timeouts: KernelHostTimeouts,
) -> Result<(), KernelHostFailure> {
    let result = timeout(timeouts.force_reap, running.force_kill_and_reap())
        .await
        .map_err(|_| KernelHostFailure::StopFailed)?;
    if result.is_ok() {
        ownership.clear_reaped(generation);
    }
    result
}

fn publish_failure(
    snapshots: &watch::Sender<KernelHostSnapshot>,
    generation: u64,
    failure: KernelHostFailure,
) {
    snapshots.send_replace(KernelHostSnapshot {
        phase: KernelHostPhase::Failed,
        generation,
        endpoint: None,
        failure: Some(failure),
    });
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Mutex,
        },
        time::Duration,
    };

    use qingyu_kernel::{contract::InstanceId, host::native::NativeHostReady};
    use tokio::sync::Notify;
    use uuid::Uuid;

    use super::*;

    #[derive(Clone, Copy)]
    enum AsyncAction {
        Complete,
        Hang,
        Fail,
    }

    #[derive(Clone, Copy)]
    struct RunningBehavior {
        exit: AsyncAction,
        graceful_stop: AsyncAction,
        force_reap: AsyncAction,
    }

    impl RunningBehavior {
        const fn graceful() -> Self {
            Self {
                exit: AsyncAction::Hang,
                graceful_stop: AsyncAction::Complete,
                force_reap: AsyncAction::Complete,
            }
        }

        const fn requires_force() -> Self {
            Self {
                exit: AsyncAction::Hang,
                graceful_stop: AsyncAction::Hang,
                force_reap: AsyncAction::Complete,
            }
        }
    }

    enum PendingScript {
        Ready {
            evidence: Option<ReadyEvidence>,
            running: RunningBehavior,
        },
        Gated {
            gate: Arc<Notify>,
            evidence: Option<ReadyEvidence>,
            running: RunningBehavior,
        },
        Fail(KernelHostFailure),
        Never,
        NeverWithCleanup {
            cancel: AsyncAction,
            force_reap: AsyncAction,
        },
    }

    impl PendingScript {
        fn ready(evidence: ReadyEvidence, running: RunningBehavior) -> Self {
            Self::Ready {
                evidence: Some(evidence),
                running,
            }
        }

        fn gated(gate: Arc<Notify>, evidence: ReadyEvidence, running: RunningBehavior) -> Self {
            Self::Gated {
                gate,
                evidence: Some(evidence),
                running,
            }
        }

        fn running_behavior(&self) -> Option<RunningBehavior> {
            match self {
                Self::Ready { running, .. } | Self::Gated { running, .. } => Some(*running),
                Self::Fail(_) | Self::Never | Self::NeverWithCleanup { .. } => None,
            }
        }
    }

    struct ScriptedFactory {
        scripts: Mutex<VecDeque<PendingScript>>,
        events: Arc<Mutex<Vec<&'static str>>>,
        spawn_count: AtomicUsize,
    }

    impl ScriptedFactory {
        fn new(scripts: impl IntoIterator<Item = PendingScript>) -> Self {
            Self {
                scripts: Mutex::new(scripts.into_iter().collect()),
                events: Arc::new(Mutex::new(Vec::new())),
                spawn_count: AtomicUsize::new(0),
            }
        }

        fn events(&self) -> Vec<&'static str> {
            self.events.lock().unwrap().clone()
        }
    }

    impl KernelProcessFactory for ScriptedFactory {
        fn spawn(
            &self,
            launch: NativeKernelLaunch,
            generation: u64,
            ownership: &KernelOwnership,
        ) -> Result<Box<dyn PendingKernel>, KernelHostFailure> {
            self.spawn_count.fetch_add(1, Ordering::SeqCst);
            let script = self
                .scripts
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(KernelHostFailure::Spawn)?;
            let permit = ownership.begin_spawn(generation)?;
            let active = Arc::new(AtomicBool::new(true));
            permit.register(Arc::new(ScriptedSynchronousGuard {
                events: Arc::clone(&self.events),
                active: Arc::clone(&active),
            }));
            let (_startup, credential) = launch.into_parts();
            Ok(Box::new(ScriptedPending {
                script,
                events: Arc::clone(&self.events),
                active,
                armed: true,
                credential,
            }))
        }
    }

    struct ScriptedSynchronousGuard {
        events: Arc<Mutex<Vec<&'static str>>>,
        active: Arc<AtomicBool>,
    }

    impl SynchronousKernelGuard for ScriptedSynchronousGuard {
        fn terminate_and_reap_or_abort(&self) {
            if self.active.swap(false, Ordering::SeqCst) {
                self.events.lock().unwrap().push("sync-terminate");
            }
        }
    }

    struct ScriptedPending {
        script: PendingScript,
        events: Arc<Mutex<Vec<&'static str>>>,
        active: Arc<AtomicBool>,
        armed: bool,
        credential: NativeKernelCredentialLease,
    }

    impl PendingKernel for ScriptedPending {
        fn wait_ready(&mut self) -> HostFuture<'_, Result<ReadyEvidence, KernelHostFailure>> {
            Box::pin(async move {
                match &mut self.script {
                    PendingScript::Ready { evidence, .. } => {
                        evidence.take().ok_or(KernelHostFailure::Protocol)
                    }
                    PendingScript::Gated { gate, evidence, .. } => {
                        let gate = Arc::clone(gate);
                        gate.notified().await;
                        evidence.take().ok_or(KernelHostFailure::Protocol)
                    }
                    PendingScript::Fail(error) => Err(*error),
                    PendingScript::Never | PendingScript::NeverWithCleanup { .. } => {
                        std::future::pending().await
                    }
                }
            })
        }

        fn credential_lease(&self) -> NativeKernelCredentialLease {
            self.credential.clone()
        }

        fn cancel_and_reap(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>> {
            Box::pin(async move {
                self.events.lock().unwrap().push("pending-cancel");
                let action = match self.script {
                    PendingScript::NeverWithCleanup { cancel, .. } => cancel,
                    _ => AsyncAction::Complete,
                };
                match action {
                    AsyncAction::Complete => {
                        self.active.store(false, Ordering::SeqCst);
                        self.credential.revoke();
                        self.armed = false;
                        Ok(())
                    }
                    AsyncAction::Hang => std::future::pending().await,
                    AsyncAction::Fail => Err(KernelHostFailure::StopFailed),
                }
            })
        }

        fn force_kill_and_reap(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>> {
            Box::pin(async move {
                self.events.lock().unwrap().push("pending-force");
                let action = match self.script {
                    PendingScript::NeverWithCleanup { force_reap, .. } => force_reap,
                    _ => AsyncAction::Complete,
                };
                match action {
                    AsyncAction::Complete => {
                        self.active.store(false, Ordering::SeqCst);
                        self.credential.revoke();
                        self.armed = false;
                        Ok(())
                    }
                    AsyncAction::Hang => std::future::pending().await,
                    AsyncAction::Fail => Err(KernelHostFailure::StopFailed),
                }
            })
        }

        fn into_running(mut self: Box<Self>) -> Result<Box<dyn RunningKernel>, KernelHostFailure> {
            let behavior = self
                .script
                .running_behavior()
                .ok_or(KernelHostFailure::Protocol)?;
            self.armed = false;
            Ok(Box::new(ScriptedRunning {
                behavior,
                events: Arc::clone(&self.events),
                active: Arc::clone(&self.active),
                armed: true,
                credential: self.credential.clone(),
            }))
        }
    }

    impl Drop for ScriptedPending {
        fn drop(&mut self) {
            if self.armed {
                self.credential.revoke();
                if self.active.swap(false, Ordering::SeqCst) {
                    self.events.lock().unwrap().push("pending-drop-terminate");
                }
            }
        }
    }

    struct ScriptedRunning {
        behavior: RunningBehavior,
        events: Arc<Mutex<Vec<&'static str>>>,
        active: Arc<AtomicBool>,
        armed: bool,
        credential: NativeKernelCredentialLease,
    }

    impl RunningKernel for ScriptedRunning {
        fn wait_exit(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>> {
            Box::pin(async move {
                match self.behavior.exit {
                    AsyncAction::Complete => {
                        self.active.store(false, Ordering::SeqCst);
                        self.credential.revoke();
                        self.armed = false;
                        Ok(())
                    }
                    AsyncAction::Hang => std::future::pending().await,
                    AsyncAction::Fail => Err(KernelHostFailure::UnexpectedExit),
                }
            })
        }

        fn shutdown_and_reap(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>> {
            Box::pin(async move {
                self.events.lock().unwrap().push("running-shutdown");
                match self.behavior.graceful_stop {
                    AsyncAction::Complete => {
                        self.active.store(false, Ordering::SeqCst);
                        self.credential.revoke();
                        self.armed = false;
                        Ok(())
                    }
                    AsyncAction::Hang => std::future::pending().await,
                    AsyncAction::Fail => Err(KernelHostFailure::StopFailed),
                }
            })
        }

        fn force_kill_and_reap(&mut self) -> HostFuture<'_, Result<(), KernelHostFailure>> {
            Box::pin(async move {
                self.events.lock().unwrap().push("running-force");
                match self.behavior.force_reap {
                    AsyncAction::Complete => {
                        self.active.store(false, Ordering::SeqCst);
                        self.credential.revoke();
                        self.armed = false;
                        Ok(())
                    }
                    AsyncAction::Hang => std::future::pending().await,
                    AsyncAction::Fail => Err(KernelHostFailure::StopFailed),
                }
            })
        }
    }

    impl Drop for ScriptedRunning {
        fn drop(&mut self) {
            if self.armed {
                self.credential.revoke();
                if self.active.swap(false, Ordering::SeqCst) {
                    self.events.lock().unwrap().push("running-drop-terminate");
                }
            }
        }
    }

    #[tokio::test]
    async fn ready_requires_matching_authenticated_instance_identity() {
        let stdout_instance = InstanceId::new(Uuid::new_v4());
        let authenticated_instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, stdout_instance),
                authenticated_instance,
            },
            RunningBehavior::graceful(),
        )]));
        let supervisor = KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
        );

        let error = supervisor.start(startup()).await.unwrap_err();

        assert_eq!(error, KernelHostFailure::IdentityMismatch);
        assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Failed);
        assert_eq!(factory.events(), vec!["pending-cancel"]);
    }

    #[tokio::test]
    async fn matching_readiness_publishes_one_endpoint_and_stops_gracefully() {
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]));
        let supervisor = KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
        );

        let access = supervisor.start(startup()).await.unwrap();
        let endpoint = access.endpoint;

        assert_eq!(endpoint.port, 43123);
        assert_eq!(endpoint.instance_id, instance);
        assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Ready);
        assert_eq!(supervisor.snapshot().endpoint, Some(endpoint));
        assert!(access.credential.is_available());
        supervisor.stop().await.unwrap();
        assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Dormant);
        assert!(!access.credential.is_available());
        assert_eq!(factory.events(), vec!["running-shutdown"]);
    }

    #[tokio::test]
    async fn concurrent_start_is_rejected_without_spawning_a_second_child() {
        let gate = Arc::new(Notify::new());
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::gated(
            gate.clone(),
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]));
        let supervisor = Arc::new(KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_secs(1)),
        ));
        let first_supervisor = supervisor.clone();
        let first = tokio::spawn(async move { first_supervisor.start(startup()).await });
        wait_for_spawn(&factory).await;

        let error = supervisor.start(startup()).await.unwrap_err();

        assert_eq!(error, KernelHostFailure::Busy);
        assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 1);
        gate.notify_one();
        first.await.unwrap().unwrap();
        supervisor.stop().await.unwrap();
    }

    #[tokio::test]
    async fn startup_timeout_cancels_and_reaps_the_pending_child() {
        let factory = Arc::new(ScriptedFactory::new([PendingScript::Never]));
        let supervisor = KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_millis(10)),
        );

        let error = supervisor.start(startup()).await.unwrap_err();

        assert_eq!(error, KernelHostFailure::StartupTimeout);
        assert_eq!(supervisor.snapshot().failure, Some(error));
        assert_eq!(factory.events(), vec!["pending-cancel"]);
    }

    #[tokio::test]
    async fn malformed_or_early_exit_readiness_is_not_retried() {
        for failure in [KernelHostFailure::Protocol, KernelHostFailure::EarlyExit] {
            let factory = Arc::new(ScriptedFactory::new([PendingScript::Fail(failure)]));
            let supervisor = KernelHostSupervisor::new(
                factory.clone(),
                KernelHostTimeouts::uniform(Duration::from_millis(100)),
            );

            assert_eq!(supervisor.start(startup()).await.unwrap_err(), failure);
            assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 1);
            assert_eq!(factory.events(), vec!["pending-cancel"]);
        }
    }

    #[tokio::test]
    async fn graceful_stop_timeout_forces_kill_and_reap() {
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::requires_force(),
        )]));
        let supervisor = KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_millis(10)),
        );
        supervisor.start(startup()).await.unwrap();

        supervisor.stop().await.unwrap();

        assert_eq!(factory.events(), vec!["running-shutdown", "running-force"]);
        assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Dormant);
    }

    #[tokio::test]
    async fn unexpected_running_exit_fails_once_without_automatic_restart() {
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior {
                exit: AsyncAction::Complete,
                graceful_stop: AsyncAction::Complete,
                force_reap: AsyncAction::Complete,
            },
        )]));
        let supervisor = KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
        );
        supervisor.start(startup()).await.unwrap();

        wait_for_phase(&supervisor, KernelHostPhase::Failed).await;

        assert_eq!(
            supervisor.snapshot().failure,
            Some(KernelHostFailure::UnexpectedExit)
        );
        assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 1);
        assert!(factory.events().is_empty());
    }

    #[tokio::test]
    async fn stop_during_start_cancels_before_any_ready_publication() {
        let gate = Arc::new(Notify::new());
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::gated(
            gate,
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]));
        let supervisor = Arc::new(KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_secs(1)),
        ));
        let start_supervisor = supervisor.clone();
        let start = tokio::spawn(async move { start_supervisor.start(startup()).await });
        wait_for_spawn(&factory).await;

        supervisor.stop().await.unwrap();

        assert_eq!(
            start.await.unwrap().unwrap_err(),
            KernelHostFailure::Cancelled
        );
        assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Dormant);
        assert_eq!(supervisor.snapshot().endpoint, None);
        assert_eq!(factory.events(), vec!["pending-cancel"]);
    }

    #[tokio::test]
    async fn queued_stop_wins_when_readiness_is_already_available() {
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]);
        let (starts, mut start_receiver) = mpsc::channel(1);
        let (stops, mut stop_receiver) = mpsc::unbounded_channel();
        let (busy_response, busy_result) = oneshot::channel();
        starts
            .send(StartCommand {
                launch: startup(),
                response: busy_response,
            })
            .await
            .unwrap();
        let (stop_response, stop_result) = oneshot::channel();
        stops.send(stop_response).unwrap();
        let (snapshots, snapshot) = watch::channel(KernelHostSnapshot::dormant());
        let (start_response, start_result) = oneshot::channel();
        let ownership = KernelOwnership::default();
        let enqueue_gate = AsyncMutex::new(());

        let ready = start_transition(
            &mut start_receiver,
            &mut stop_receiver,
            &snapshots,
            &factory,
            &ownership,
            &enqueue_gate,
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
            1,
            startup(),
            start_response,
        )
        .await;

        assert!(ready.is_none());
        assert_eq!(
            start_result.await.unwrap().unwrap_err(),
            KernelHostFailure::Cancelled
        );
        assert_eq!(
            busy_result.await.unwrap().unwrap_err(),
            KernelHostFailure::Cancelled
        );
        stop_result.await.unwrap().unwrap();
        assert_eq!(snapshot.borrow().phase, KernelHostPhase::Dormant);
        assert_eq!(snapshot.borrow().endpoint, None);
        assert_eq!(factory.events(), vec!["pending-cancel"]);
    }

    #[tokio::test]
    async fn queued_busy_start_cannot_preempt_available_readiness() {
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]);
        let (starts, mut start_receiver) = mpsc::channel(1);
        let (_stops, mut stop_receiver) = mpsc::unbounded_channel();
        let (busy_response, busy_result) = oneshot::channel();
        starts
            .send(StartCommand {
                launch: startup(),
                response: busy_response,
            })
            .await
            .unwrap();
        let (snapshots, snapshot) = watch::channel(KernelHostSnapshot::dormant());
        let (start_response, start_result) = oneshot::channel();
        let ownership = KernelOwnership::default();
        let enqueue_gate = AsyncMutex::new(());

        let ready = start_transition(
            &mut start_receiver,
            &mut stop_receiver,
            &snapshots,
            &factory,
            &ownership,
            &enqueue_gate,
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
            1,
            startup(),
            start_response,
        )
        .await;

        let access = start_result.await.unwrap().unwrap();
        assert_eq!(snapshot.borrow().phase, KernelHostPhase::Ready);
        assert_eq!(snapshot.borrow().endpoint, Some(access.endpoint));
        assert!(ready.is_some());
        drop(start_receiver.try_recv().unwrap());
        assert!(busy_result.await.is_err());
    }

    #[tokio::test]
    async fn ready_stop_barrier_cancels_every_previously_queued_start() {
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]);
        let ownership = KernelOwnership::default();
        let mut pending = factory.spawn(startup(), 1, &ownership).unwrap();
        let evidence = pending.wait_ready().await.unwrap();
        let credential = pending.credential_lease();
        let ready = ReadyKernel {
            access: NativeKernelAccess {
                endpoint: KernelEndpoint {
                    generation: 1,
                    port: evidence.ready.port(),
                    instance_id: evidence.authenticated_instance,
                },
                credential,
            },
            process: pending.into_running().unwrap(),
        };
        let (starts, mut start_receiver) = mpsc::channel(1);
        let (stops, mut stop_receiver) = mpsc::unbounded_channel();
        let (busy_response, busy_result) = oneshot::channel();
        starts
            .send(StartCommand {
                launch: startup(),
                response: busy_response,
            })
            .await
            .unwrap();
        let (stop_response, stop_result) = oneshot::channel();
        stops.send(stop_response).unwrap();
        let (snapshots, snapshot) = watch::channel(KernelHostSnapshot {
            phase: KernelHostPhase::Ready,
            generation: 1,
            endpoint: Some(ready.access.endpoint),
            failure: None,
        });
        let enqueue_gate = AsyncMutex::new(());

        let next = ready_transition(
            &mut start_receiver,
            &mut stop_receiver,
            &snapshots,
            &ownership,
            &enqueue_gate,
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
            ready,
        )
        .await;

        assert!(next.is_none());
        assert_eq!(
            busy_result.await.unwrap().unwrap_err(),
            KernelHostFailure::Cancelled
        );
        stop_result.await.unwrap().unwrap();
        assert_eq!(snapshot.borrow().phase, KernelHostPhase::Dormant);
        assert_eq!(factory.events(), vec!["running-shutdown"]);
    }

    #[tokio::test]
    async fn failed_or_timed_out_start_cleanup_never_publishes_dormant() {
        for cancel in [AsyncAction::Fail, AsyncAction::Hang] {
            let instance = InstanceId::new(Uuid::new_v4());
            let factory = Arc::new(ScriptedFactory::new([
                PendingScript::NeverWithCleanup {
                    cancel,
                    force_reap: AsyncAction::Fail,
                },
                PendingScript::ready(
                    ReadyEvidence {
                        ready: NativeHostReady::new(43123, instance),
                        authenticated_instance: instance,
                    },
                    RunningBehavior::graceful(),
                ),
            ]));
            let supervisor = Arc::new(KernelHostSupervisor::new(
                factory.clone(),
                KernelHostTimeouts::uniform(Duration::from_millis(10)),
            ));
            let start_supervisor = supervisor.clone();
            let start = tokio::spawn(async move { start_supervisor.start(startup()).await });
            wait_for_spawn(&factory).await;

            assert_eq!(
                supervisor.stop().await.unwrap_err(),
                KernelHostFailure::StopFailed
            );

            assert_eq!(
                start.await.unwrap().unwrap_err(),
                KernelHostFailure::Cancelled
            );
            assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Failed);
            assert_eq!(
                supervisor.snapshot().failure,
                Some(KernelHostFailure::StopFailed)
            );
            assert_eq!(
                factory.events(),
                vec!["pending-cancel", "pending-force", "pending-drop-terminate"]
            );

            supervisor.stop().await.unwrap();
            assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Dormant);
            supervisor.start(startup()).await.unwrap();
            supervisor.stop().await.unwrap();
            assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 2);
            assert_eq!(
                factory.events(),
                vec![
                    "pending-cancel",
                    "pending-force",
                    "pending-drop-terminate",
                    "running-shutdown"
                ]
            );
        }
    }

    #[tokio::test]
    async fn wait_exit_failure_forces_reap_before_publishing_failure() {
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior {
                exit: AsyncAction::Fail,
                graceful_stop: AsyncAction::Complete,
                force_reap: AsyncAction::Complete,
            },
        )]));
        let supervisor = KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
        );
        supervisor.start(startup()).await.unwrap();

        wait_for_phase(&supervisor, KernelHostPhase::Failed).await;

        assert_eq!(factory.events(), vec!["running-force"]);
        assert_eq!(
            supervisor.snapshot().failure,
            Some(KernelHostFailure::UnexpectedExit)
        );
    }

    #[tokio::test]
    async fn dropping_supervisor_while_starting_terminates_the_armed_child() {
        let gate = Arc::new(Notify::new());
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::gated(
            gate,
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]));
        let supervisor = Arc::new(KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_secs(1)),
        ));
        let start_supervisor = supervisor.clone();
        let start = tokio::spawn(async move { start_supervisor.start(startup()).await });
        wait_for_spawn(&factory).await;
        start.abort();
        let _join_result = start.await;

        drop(supervisor);
        assert_eq!(factory.events(), vec!["sync-terminate"]);
    }

    #[tokio::test]
    async fn dropping_ready_supervisor_terminates_the_armed_running_child() {
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]));
        let supervisor = KernelHostSupervisor::new(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
        );
        supervisor.start(startup()).await.unwrap();

        drop(supervisor);
        assert_eq!(factory.events(), vec!["sync-terminate"]);
    }

    async fn wait_for_spawn(factory: &ScriptedFactory) {
        for _attempt in 0..100 {
            if factory.spawn_count.load(Ordering::SeqCst) > 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("supervisor did not spawn the scripted child");
    }

    async fn wait_for_phase(supervisor: &KernelHostSupervisor, phase: KernelHostPhase) {
        for _attempt in 0..100 {
            if supervisor.snapshot().phase == phase {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("supervisor did not reach phase {phase:?}");
    }

    #[test]
    fn supervisor_is_not_registered_with_the_production_desktop_runtime() {
        let production_runtime = include_str!("desktop_runtime.rs");

        assert!(!production_runtime.contains("KernelHostSupervisor"));
        assert!(!production_runtime.contains("kernel_host"));
    }

    fn startup() -> NativeKernelLaunch {
        let workspace = std::env::temp_dir();
        NativeKernelLaunch::desktop(
            workspace.clone(),
            PathBuf::from("app-data"),
            PathBuf::from("cache"),
            qingyu_kernel::host::native::NativeHostWorkspaceState::for_workspace(
                &workspace,
                "Workspace",
            )
            .unwrap(),
            "tauri://localhost".to_owned(),
        )
        .unwrap()
    }
}
