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
    plan: NativeKernelLaunchPlan,
}

#[derive(Clone)]
struct NativeKernelLaunchPlan {
    workspace_root: std::path::PathBuf,
    app_data_root: std::path::PathBuf,
    cache_root: std::path::PathBuf,
    workspace_state: NativeHostWorkspaceState,
    origin: String,
}

impl NativeKernelLaunch {
    pub(crate) fn desktop(
        workspace_root: std::path::PathBuf,
        app_data_root: std::path::PathBuf,
        cache_root: std::path::PathBuf,
        workspace_state: NativeHostWorkspaceState,
        origin: String,
    ) -> Result<Self, KernelHostFailure> {
        NativeKernelLaunchPlan {
            workspace_root,
            app_data_root,
            cache_root,
            workspace_state,
            origin,
        }
        .fresh_launch()
    }

    fn recovery_plan(&self) -> NativeKernelLaunchPlan {
        self.plan.clone()
    }

    pub(crate) fn into_parts(self) -> (NativeHostStart, NativeKernelCredentialLease) {
        (self.startup, self.credential)
    }
}

impl NativeKernelLaunchPlan {
    fn fresh_launch(&self) -> Result<NativeKernelLaunch, KernelHostFailure> {
        let credential =
            NativeLaunchCredential::generate().map_err(|_| KernelHostFailure::Spawn)?;
        let child_credential =
            NativeLaunchCredential::from_secret(credential.expose_secret().to_owned())
                .map_err(|_| KernelHostFailure::Spawn)?;
        Ok(Self {
            workspace_root: self.workspace_root.clone(),
            app_data_root: self.app_data_root.clone(),
            cache_root: self.cache_root.clone(),
            workspace_state: self.workspace_state.clone(),
            origin: self.origin.clone(),
        }
        .into_launch(
            NativeHostStart::desktop(
                self.workspace_root.clone(),
                self.app_data_root.clone(),
                self.cache_root.clone(),
                self.workspace_state.clone(),
                self.origin.clone(),
                child_credential,
            ),
            credential,
        ))
    }

    fn into_launch(
        self,
        startup: NativeHostStart,
        credential: NativeLaunchCredential,
    ) -> NativeKernelLaunch {
        NativeKernelLaunch {
            startup,
            credential: NativeKernelCredentialLease::new(credential),
            plan: self,
        }
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
    Retrying,
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
    recovery_initial_backoff: Duration,
    recovery_max_backoff: Duration,
    max_recovery_attempts: u8,
}

impl KernelHostTimeouts {
    pub(crate) const fn uniform(duration: Duration) -> Self {
        Self {
            startup: duration,
            graceful_stop: duration,
            force_reap: duration,
            recovery_initial_backoff: duration,
            recovery_max_backoff: duration.saturating_mul(4),
            max_recovery_attempts: 3,
        }
    }

    pub(crate) const fn with_recovery(
        mut self,
        initial_backoff: Duration,
        max_backoff: Duration,
        max_attempts: u8,
    ) -> Self {
        self.recovery_initial_backoff = initial_backoff;
        self.recovery_max_backoff = max_backoff;
        self.max_recovery_attempts = max_attempts;
        self
    }
}

pub(crate) struct KernelHostSupervisor {
    starts: mpsc::Sender<StartCommand>,
    stops: mpsc::UnboundedSender<oneshot::Sender<Result<(), KernelHostFailure>>>,
    enqueue_gate: Arc<AsyncMutex<()>>,
    snapshots: watch::Receiver<KernelHostSnapshot>,
    bootstrap_session: crate::kernel_bootstrap::NativeKernelBootstrapSession,
    ownership: Arc<KernelOwnership>,
    actor: JoinHandle<()>,
}

impl KernelHostSupervisor {
    pub(crate) fn new(
        factory: Arc<dyn KernelProcessFactory>,
        timeouts: KernelHostTimeouts,
    ) -> Self {
        Self::new_with_bootstrap(
            factory,
            timeouts,
            crate::kernel_bootstrap::NativeKernelBootstrapOwner::new(),
        )
    }

    pub(crate) fn new_with_bootstrap(
        factory: Arc<dyn KernelProcessFactory>,
        timeouts: KernelHostTimeouts,
        bootstrap: crate::kernel_bootstrap::NativeKernelBootstrapOwner,
    ) -> Self {
        let (starts, start_receiver) = mpsc::channel(8);
        let (stops, stop_receiver) = mpsc::unbounded_channel();
        let (snapshot_sender, snapshots) = watch::channel(KernelHostSnapshot::dormant());
        let ownership = Arc::new(KernelOwnership::default());
        let enqueue_gate = Arc::new(AsyncMutex::new(()));
        let bootstrap_session = bootstrap.open_supervisor_session();
        let actor = tokio::spawn(run_actor(
            start_receiver,
            stop_receiver,
            snapshot_sender,
            factory,
            bootstrap_session.clone(),
            Arc::clone(&ownership),
            Arc::clone(&enqueue_gate),
            timeouts,
        ));
        Self {
            starts,
            stops,
            enqueue_gate,
            snapshots,
            bootstrap_session,
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
        self.bootstrap_session.close();
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
    plan: NativeKernelLaunchPlan,
    recovery_attempts: u8,
}

enum ActorMode {
    Idle,
    Ready(Box<ReadyKernel>),
}

enum StartKind {
    Manual,
    Recovery,
}

enum StartOutcome {
    Ready(Box<ReadyKernel>),
    Failed(KernelHostFailure),
    Stopped,
    Closed,
}

enum ReadyOutcome {
    Recover {
        plan: NativeKernelLaunchPlan,
        attempts: u8,
        failure: KernelHostFailure,
    },
    Idle,
    Closed,
}

#[allow(clippy::too_many_arguments)]
async fn run_actor(
    mut starts: mpsc::Receiver<StartCommand>,
    mut stops: mpsc::UnboundedReceiver<oneshot::Sender<Result<(), KernelHostFailure>>>,
    snapshots: watch::Sender<KernelHostSnapshot>,
    factory: Arc<dyn KernelProcessFactory>,
    bootstrap: crate::kernel_bootstrap::NativeKernelBootstrapSession,
    ownership: Arc<KernelOwnership>,
    enqueue_gate: Arc<AsyncMutex<()>>,
    timeouts: KernelHostTimeouts,
) {
    let Ok(mut generation) = bootstrap.last_generation() else {
        return;
    };
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
                            &bootstrap,
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
                        let _finish_result = bootstrap.finish_stop(generation);
                        finish_stop_barrier(&mut starts, enqueue_gate.as_ref(), response, Ok(()))
                            .await;
                        ActorMode::Idle
                    }
                }
            }
            ActorMode::Ready(ready) => match ready_transition(
                &mut starts,
                &mut stops,
                &snapshots,
                &bootstrap,
                ownership.as_ref(),
                enqueue_gate.as_ref(),
                timeouts,
                ready,
            )
            .await
            {
                ReadyOutcome::Recover {
                    plan,
                    attempts,
                    failure,
                } => match recover_transition(
                    &mut starts,
                    &mut stops,
                    &snapshots,
                    factory.as_ref(),
                    &bootstrap,
                    ownership.as_ref(),
                    enqueue_gate.as_ref(),
                    timeouts,
                    &mut generation,
                    plan,
                    attempts,
                    failure,
                )
                .await
                {
                    Some(ready) => ActorMode::Ready(ready),
                    None => ActorMode::Idle,
                },
                ReadyOutcome::Idle => ActorMode::Idle,
                ReadyOutcome::Closed => return,
            },
        };
    }
}

#[allow(clippy::too_many_arguments)]
async fn start_transition(
    starts: &mut mpsc::Receiver<StartCommand>,
    stops: &mut mpsc::UnboundedReceiver<oneshot::Sender<Result<(), KernelHostFailure>>>,
    snapshots: &watch::Sender<KernelHostSnapshot>,
    factory: &dyn KernelProcessFactory,
    bootstrap: &crate::kernel_bootstrap::NativeKernelBootstrapSession,
    ownership: &KernelOwnership,
    enqueue_gate: &AsyncMutex<()>,
    timeouts: KernelHostTimeouts,
    generation: u64,
    launch: NativeKernelLaunch,
    response: oneshot::Sender<Result<NativeKernelAccess, KernelHostFailure>>,
) -> Option<Box<ReadyKernel>> {
    match spawn_transition(
        starts,
        stops,
        snapshots,
        factory,
        bootstrap,
        ownership,
        enqueue_gate,
        timeouts,
        generation,
        launch,
        StartKind::Manual,
        0,
    )
    .await
    {
        StartOutcome::Ready(ready) => {
            let _send_result = response.send(Ok(ready.access.clone()));
            Some(ready)
        }
        StartOutcome::Failed(error) => {
            let _send_result = response.send(Err(error));
            None
        }
        StartOutcome::Stopped => {
            let _send_result = response.send(Err(KernelHostFailure::Cancelled));
            None
        }
        StartOutcome::Closed => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn spawn_transition(
    starts: &mut mpsc::Receiver<StartCommand>,
    stops: &mut mpsc::UnboundedReceiver<oneshot::Sender<Result<(), KernelHostFailure>>>,
    snapshots: &watch::Sender<KernelHostSnapshot>,
    factory: &dyn KernelProcessFactory,
    bootstrap: &crate::kernel_bootstrap::NativeKernelBootstrapSession,
    ownership: &KernelOwnership,
    enqueue_gate: &AsyncMutex<()>,
    timeouts: KernelHostTimeouts,
    generation: u64,
    launch: NativeKernelLaunch,
    kind: StartKind,
    recovery_attempts: u8,
) -> StartOutcome {
    let bootstrap_result = match kind {
        StartKind::Manual => bootstrap.begin_start(generation),
        StartKind::Recovery => bootstrap.continue_start(generation),
    };
    if bootstrap_result.is_err() {
        return StartOutcome::Failed(KernelHostFailure::Cancelled);
    }
    snapshots.send_replace(KernelHostSnapshot {
        phase: KernelHostPhase::Starting,
        generation,
        endpoint: None,
        failure: None,
    });
    let plan = launch.recovery_plan();
    let mut pending = match factory.spawn(launch, generation, ownership) {
        Ok(pending) => pending,
        Err(error) => {
            publish_failure(snapshots, bootstrap, generation, error);
            return StartOutcome::Failed(error);
        }
    };
    if !ownership.owns(generation) {
        publish_failure(snapshots, bootstrap, generation, KernelHostFailure::Spawn);
        return StartOutcome::Failed(KernelHostFailure::Spawn);
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
            publish_failure(snapshots, bootstrap, generation, reported);
            return StartOutcome::Failed(reported);
        }
        StartWait::Timeout => {
            let reported = cleanup_pending(&mut *pending, ownership, generation, timeouts)
                .await
                .err()
                .unwrap_or(KernelHostFailure::StartupTimeout);
            publish_failure(snapshots, bootstrap, generation, reported);
            return StartOutcome::Failed(reported);
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
                    let _finish_result = bootstrap.finish_stop(generation);
                }
                Err(error) => publish_failure(snapshots, bootstrap, generation, error),
            }
            finish_stop_barrier(starts, enqueue_gate, stop_response, stopped).await;
            return StartOutcome::Stopped;
        }
        StartWait::Closed => {
            let _cleanup_result =
                cleanup_pending(&mut *pending, ownership, generation, timeouts).await;
            return StartOutcome::Closed;
        }
    };

    if evidence.ready.instance_id() != evidence.authenticated_instance {
        let reported = cleanup_pending(&mut *pending, ownership, generation, timeouts)
            .await
            .err()
            .unwrap_or(KernelHostFailure::IdentityMismatch);
        publish_failure(snapshots, bootstrap, generation, reported);
        return StartOutcome::Failed(reported);
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
    let mut process = match pending.into_running() {
        Ok(process) => process,
        Err(error) => {
            ownership.terminate_active();
            publish_failure(snapshots, bootstrap, generation, error);
            return StartOutcome::Failed(error);
        }
    };
    if bootstrap.publish(access.clone()).is_err() {
        let reported = stop_running(&mut *process, ownership, generation, timeouts)
            .await
            .err()
            .unwrap_or(KernelHostFailure::Cancelled);
        publish_failure(snapshots, bootstrap, generation, reported);
        return StartOutcome::Failed(reported);
    }
    snapshots.send_replace(KernelHostSnapshot {
        phase: KernelHostPhase::Ready,
        generation,
        endpoint: Some(endpoint),
        failure: None,
    });
    StartOutcome::Ready(Box::new(ReadyKernel {
        access,
        process,
        plan,
        recovery_attempts,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn ready_transition(
    starts: &mut mpsc::Receiver<StartCommand>,
    stops: &mut mpsc::UnboundedReceiver<oneshot::Sender<Result<(), KernelHostFailure>>>,
    snapshots: &watch::Sender<KernelHostSnapshot>,
    bootstrap: &crate::kernel_bootstrap::NativeKernelBootstrapSession,
    ownership: &KernelOwnership,
    enqueue_gate: &AsyncMutex<()>,
    timeouts: KernelHostTimeouts,
    mut ready: Box<ReadyKernel>,
) -> ReadyOutcome {
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
            ReadyOutcome::Recover {
                plan: ready.plan,
                attempts: ready.recovery_attempts,
                failure: KernelHostFailure::UnexpectedExit,
            }
        }
        ReadyWait::Exit(Err(error)) => {
            publish_failure(
                snapshots,
                bootstrap,
                ready.access.endpoint.generation,
                error,
            );
            match force_reap_running(
                &mut *ready.process,
                ownership,
                ready.access.endpoint.generation,
                timeouts,
            )
            .await
            {
                Ok(()) => ReadyOutcome::Recover {
                    plan: ready.plan,
                    attempts: ready.recovery_attempts,
                    failure: error,
                },
                Err(reported) => {
                    publish_failure(
                        snapshots,
                        bootstrap,
                        ready.access.endpoint.generation,
                        reported,
                    );
                    ReadyOutcome::Idle
                }
            }
        }
        ReadyWait::Stop(response) => {
            let generation = ready.access.endpoint.generation;
            snapshots.send_replace(KernelHostSnapshot {
                phase: KernelHostPhase::Stopping,
                generation,
                endpoint: None,
                failure: None,
            });
            let _finish_result = bootstrap.finish_stop(generation);
            let result = stop_running(&mut *ready.process, ownership, generation, timeouts).await;
            match result {
                Ok(()) => {
                    snapshots.send_replace(KernelHostSnapshot {
                        phase: KernelHostPhase::Dormant,
                        generation,
                        endpoint: None,
                        failure: None,
                    });
                }
                Err(error) => publish_failure(snapshots, bootstrap, generation, error),
            };
            finish_stop_barrier(starts, enqueue_gate, response, result).await;
            ReadyOutcome::Idle
        }
        ReadyWait::Closed => {
            let _cleanup_result = stop_running(
                &mut *ready.process,
                ownership,
                ready.access.endpoint.generation,
                timeouts,
            )
            .await;
            let _clear_result = bootstrap.clear_generation(ready.access.endpoint.generation);
            ReadyOutcome::Closed
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn recover_transition(
    starts: &mut mpsc::Receiver<StartCommand>,
    stops: &mut mpsc::UnboundedReceiver<oneshot::Sender<Result<(), KernelHostFailure>>>,
    snapshots: &watch::Sender<KernelHostSnapshot>,
    factory: &dyn KernelProcessFactory,
    bootstrap: &crate::kernel_bootstrap::NativeKernelBootstrapSession,
    ownership: &KernelOwnership,
    enqueue_gate: &AsyncMutex<()>,
    timeouts: KernelHostTimeouts,
    generation: &mut u64,
    plan: NativeKernelLaunchPlan,
    mut attempts: u8,
    mut last_failure: KernelHostFailure,
) -> Option<Box<ReadyKernel>> {
    loop {
        if attempts >= timeouts.max_recovery_attempts {
            publish_failure(snapshots, bootstrap, *generation, last_failure);
            return None;
        }
        attempts = attempts.saturating_add(1);
        *generation = generation.saturating_add(1);
        let attempt_generation = *generation;
        if bootstrap.begin_retry(attempt_generation).is_err() {
            return None;
        }
        snapshots.send_replace(KernelHostSnapshot {
            phase: KernelHostPhase::Retrying,
            generation: attempt_generation,
            endpoint: None,
            failure: Some(last_failure),
        });

        enum RecoveryWait {
            Retry,
            Stop(oneshot::Sender<Result<(), KernelHostFailure>>),
            Closed,
        }

        let deadline = sleep(recovery_backoff(timeouts, attempts));
        tokio::pin!(deadline);
        let wait = loop {
            tokio::select! {
                biased;
                response = stops.recv() => match response {
                    Some(response) => break RecoveryWait::Stop(response),
                    None => break RecoveryWait::Closed,
                },
                _expired = &mut deadline => break RecoveryWait::Retry,
                command = starts.recv() => match command {
                    Some(StartCommand { response, .. }) => {
                        let _send_result = response.send(Err(KernelHostFailure::Busy));
                    }
                    None => break RecoveryWait::Closed,
                },
            }
        };

        match wait {
            RecoveryWait::Retry => {}
            RecoveryWait::Stop(response) => {
                snapshots.send_replace(KernelHostSnapshot {
                    phase: KernelHostPhase::Dormant,
                    generation: attempt_generation,
                    endpoint: None,
                    failure: None,
                });
                let _finish_result = bootstrap.finish_stop(attempt_generation);
                finish_stop_barrier(starts, enqueue_gate, response, Ok(())).await;
                return None;
            }
            RecoveryWait::Closed => {
                let _clear_result = bootstrap.clear_generation(attempt_generation);
                return None;
            }
        }

        let launch = match plan.fresh_launch() {
            Ok(launch) => launch,
            Err(error) => {
                last_failure = error;
                publish_failure(snapshots, bootstrap, attempt_generation, error);
                continue;
            }
        };
        match spawn_transition(
            starts,
            stops,
            snapshots,
            factory,
            bootstrap,
            ownership,
            enqueue_gate,
            timeouts,
            attempt_generation,
            launch,
            StartKind::Recovery,
            attempts,
        )
        .await
        {
            StartOutcome::Ready(ready) => return Some(ready),
            StartOutcome::Failed(error) if can_retry_recovery_failure(error) => {
                last_failure = error;
            }
            StartOutcome::Failed(_) | StartOutcome::Stopped | StartOutcome::Closed => {
                return None;
            }
        }
    }
}

fn recovery_backoff(timeouts: KernelHostTimeouts, attempt: u8) -> Duration {
    let exponent = u32::from(attempt.saturating_sub(1)).min(31);
    let factor = 1_u32.checked_shl(exponent).unwrap_or(u32::MAX);
    timeouts
        .recovery_initial_backoff
        .saturating_mul(factor)
        .min(timeouts.recovery_max_backoff)
}

fn can_retry_recovery_failure(failure: KernelHostFailure) -> bool {
    matches!(
        failure,
        KernelHostFailure::Spawn
            | KernelHostFailure::StartupTimeout
            | KernelHostFailure::Protocol
            | KernelHostFailure::EarlyExit
            | KernelHostFailure::UnexpectedExit
    )
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
    bootstrap: &crate::kernel_bootstrap::NativeKernelBootstrapSession,
    generation: u64,
    failure: KernelHostFailure,
) {
    if matches!(bootstrap.fail_generation(generation), Ok(true)) {
        snapshots.send_replace(KernelHostSnapshot {
            phase: KernelHostPhase::Failed,
            generation,
            endpoint: None,
            failure: Some(failure),
        });
    }
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
    use serde_json::json;
    use tokio::sync::Notify;
    use uuid::Uuid;

    use super::*;

    #[derive(Clone, Copy)]
    enum AsyncAction {
        Complete,
        Hang,
        Fail,
    }

    #[derive(Clone)]
    struct RunningBehavior {
        exit: AsyncAction,
        graceful_stop: AsyncAction,
        force_reap: AsyncAction,
        force_reap_gate: Option<Arc<Notify>>,
    }

    impl RunningBehavior {
        const fn graceful() -> Self {
            Self {
                exit: AsyncAction::Hang,
                graceful_stop: AsyncAction::Complete,
                force_reap: AsyncAction::Complete,
                force_reap_gate: None,
            }
        }

        const fn requires_force() -> Self {
            Self {
                exit: AsyncAction::Hang,
                graceful_stop: AsyncAction::Hang,
                force_reap: AsyncAction::Complete,
                force_reap_gate: None,
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
                Self::Ready { running, .. } | Self::Gated { running, .. } => Some(running.clone()),
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
                if let Some(gate) = self.behavior.force_reap_gate.clone() {
                    gate.notified().await;
                }
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
    async fn replacement_supervisor_continues_the_shared_bootstrap_generation_fence() {
        let instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]));
        let bootstrap = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let retired = bootstrap.open_supervisor_session();
        retired.begin_start(7).unwrap();
        retired.fail_generation(7).unwrap();
        retired.close();
        let supervisor = KernelHostSupervisor::new_with_bootstrap(
            factory,
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
            bootstrap,
        );

        let access = supervisor.start(startup()).await.unwrap();

        assert_eq!(access.endpoint.generation, 8);
        supervisor.stop().await.unwrap();
    }

    #[tokio::test]
    async fn stale_supervisor_cannot_fail_or_revoke_the_active_generation() {
        let instance = InstanceId::new(Uuid::new_v4());
        let active_factory = Arc::new(ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior::graceful(),
        )]));
        let stale_factory = Arc::new(ScriptedFactory::new(std::iter::empty()));
        let bootstrap = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let active = KernelHostSupervisor::new_with_bootstrap(
            active_factory,
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
            bootstrap.clone(),
        );
        let stale = KernelHostSupervisor::new_with_bootstrap(
            stale_factory,
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
            bootstrap.clone(),
        );
        let access = active.start(startup()).await.unwrap();

        assert_eq!(
            stale.start(startup()).await.unwrap_err(),
            KernelHostFailure::Cancelled
        );

        assert!(access.credential.is_available());
        let publication = serde_json::to_value(bootstrap.read().unwrap()).unwrap();
        assert_eq!(publication["status"], json!("ready"));
        assert_eq!(publication["generation"], json!("1"));
        active.stop().await.unwrap();
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

    #[tokio::test(start_paused = true)]
    async fn unexpected_exit_revokes_ready_and_recovers_with_a_fresh_generation() {
        let first_instance = InstanceId::new(Uuid::new_v4());
        let second_instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([
            PendingScript::ready(
                ReadyEvidence {
                    ready: NativeHostReady::new(43123, first_instance),
                    authenticated_instance: first_instance,
                },
                RunningBehavior {
                    exit: AsyncAction::Complete,
                    graceful_stop: AsyncAction::Complete,
                    force_reap: AsyncAction::Complete,
                    force_reap_gate: None,
                },
            ),
            PendingScript::ready(
                ReadyEvidence {
                    ready: NativeHostReady::new(43124, second_instance),
                    authenticated_instance: second_instance,
                },
                RunningBehavior::graceful(),
            ),
        ]));
        let bootstrap = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let supervisor = KernelHostSupervisor::new_with_bootstrap(
            factory.clone(),
            recovery_timeouts(3),
            bootstrap.clone(),
        );
        let first = supervisor.start(startup()).await.unwrap();

        wait_for_phase(&supervisor, KernelHostPhase::Retrying).await;

        assert!(!first.credential.is_available());
        assert_eq!(supervisor.snapshot().generation, 2);
        assert_eq!(supervisor.snapshot().endpoint, None);
        assert_eq!(
            serde_json::to_value(bootstrap.read().unwrap()).unwrap(),
            json!({
                "status": "retrying",
                "bootstrapVersion": 1,
                "generation": "2",
            })
        );

        tokio::time::advance(Duration::from_secs(1)).await;
        wait_for_phase(&supervisor, KernelHostPhase::Ready).await;

        assert_eq!(supervisor.snapshot().generation, 2);
        assert_eq!(supervisor.snapshot().endpoint.unwrap().port, 43124);
        let recovered = serde_json::to_value(bootstrap.read().unwrap()).unwrap();
        assert_eq!(recovered["status"], json!("ready"));
        assert_eq!(recovered["generation"], json!("2"));
        assert!(recovered["credential"]
            .as_str()
            .is_some_and(|credential| !credential.is_empty()));
        supervisor.stop().await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn recovery_uses_bounded_exponential_backoff_then_allows_manual_retry() {
        let first_instance = InstanceId::new(Uuid::new_v4());
        let recovered_instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([
            PendingScript::ready(
                ReadyEvidence {
                    ready: NativeHostReady::new(43123, first_instance),
                    authenticated_instance: first_instance,
                },
                RunningBehavior {
                    exit: AsyncAction::Complete,
                    graceful_stop: AsyncAction::Complete,
                    force_reap: AsyncAction::Complete,
                    force_reap_gate: None,
                },
            ),
            PendingScript::Fail(KernelHostFailure::EarlyExit),
            PendingScript::Fail(KernelHostFailure::EarlyExit),
            PendingScript::Fail(KernelHostFailure::EarlyExit),
            PendingScript::ready(
                ReadyEvidence {
                    ready: NativeHostReady::new(43125, recovered_instance),
                    authenticated_instance: recovered_instance,
                },
                RunningBehavior::graceful(),
            ),
        ]));
        let bootstrap = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let supervisor = KernelHostSupervisor::new_with_bootstrap(
            factory.clone(),
            recovery_timeouts(3),
            bootstrap.clone(),
        );
        supervisor.start(startup()).await.unwrap();
        wait_for_phase(&supervisor, KernelHostPhase::Retrying).await;

        tokio::time::advance(Duration::from_secs(1)).await;
        wait_for_generation(&supervisor, 3).await;
        assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 2);

        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 2);
        tokio::time::advance(Duration::from_secs(1)).await;
        wait_for_generation(&supervisor, 4).await;
        assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 3);

        tokio::time::advance(Duration::from_secs(3)).await;
        assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 3);
        tokio::time::advance(Duration::from_secs(1)).await;
        wait_for_phase(&supervisor, KernelHostPhase::Failed).await;
        assert_eq!(supervisor.snapshot().generation, 4);
        assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 4);
        assert_eq!(
            serde_json::to_value(bootstrap.read().unwrap()).unwrap(),
            json!({
                "status": "failed",
                "bootstrapVersion": 1,
                "generation": "4",
            })
        );

        supervisor.start(startup()).await.unwrap();
        assert_eq!(supervisor.snapshot().generation, 5);
        assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Ready);
        supervisor.stop().await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn explicit_stop_during_backoff_suppresses_automatic_restart() {
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
                force_reap_gate: None,
            },
        )]));
        let bootstrap = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let supervisor = KernelHostSupervisor::new_with_bootstrap(
            factory.clone(),
            recovery_timeouts(3),
            bootstrap.clone(),
        );
        supervisor.start(startup()).await.unwrap();
        wait_for_phase(&supervisor, KernelHostPhase::Retrying).await;

        supervisor.stop().await.unwrap();
        tokio::time::advance(Duration::from_secs(30)).await;

        assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 1);
        assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Dormant);
        assert_eq!(
            serde_json::to_value(bootstrap.read().unwrap()).unwrap(),
            json!({
                "status": "dormant",
                "bootstrapVersion": 1,
                "generation": "2",
            })
        );
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
        let bootstrap_owner = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let bootstrap = bootstrap_owner.open_supervisor_session();
        let enqueue_gate = AsyncMutex::new(());

        let ready = start_transition(
            &mut start_receiver,
            &mut stop_receiver,
            &snapshots,
            &factory,
            &bootstrap,
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
        let bootstrap_owner = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let bootstrap = bootstrap_owner.open_supervisor_session();
        let enqueue_gate = AsyncMutex::new(());

        let ready = start_transition(
            &mut start_receiver,
            &mut stop_receiver,
            &snapshots,
            &factory,
            &bootstrap,
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
        let launch = startup();
        let plan = launch.recovery_plan();
        let mut pending = factory.spawn(launch, 1, &ownership).unwrap();
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
            plan,
            recovery_attempts: 0,
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
        let bootstrap_owner = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let bootstrap = bootstrap_owner.open_supervisor_session();
        bootstrap.begin_start(1).unwrap();
        bootstrap.publish(ready.access.clone()).unwrap();
        let enqueue_gate = AsyncMutex::new(());

        let next = ready_transition(
            &mut start_receiver,
            &mut stop_receiver,
            &snapshots,
            &bootstrap,
            &ownership,
            &enqueue_gate,
            KernelHostTimeouts::uniform(Duration::from_millis(100)),
            Box::new(ready),
        )
        .await;

        assert!(matches!(next, ReadyOutcome::Idle));
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

    #[tokio::test(start_paused = true)]
    async fn wait_exit_failure_is_force_reaped_before_recovery_spawns() {
        let first_instance = InstanceId::new(Uuid::new_v4());
        let second_instance = InstanceId::new(Uuid::new_v4());
        let factory = Arc::new(ScriptedFactory::new([
            PendingScript::ready(
                ReadyEvidence {
                    ready: NativeHostReady::new(43123, first_instance),
                    authenticated_instance: first_instance,
                },
                RunningBehavior {
                    exit: AsyncAction::Fail,
                    graceful_stop: AsyncAction::Complete,
                    force_reap: AsyncAction::Complete,
                    force_reap_gate: None,
                },
            ),
            PendingScript::ready(
                ReadyEvidence {
                    ready: NativeHostReady::new(43124, second_instance),
                    authenticated_instance: second_instance,
                },
                RunningBehavior::graceful(),
            ),
        ]));
        let supervisor = KernelHostSupervisor::new(factory.clone(), recovery_timeouts(3));
        supervisor.start(startup()).await.unwrap();

        wait_for_phase(&supervisor, KernelHostPhase::Retrying).await;

        assert_eq!(factory.events(), vec!["running-force"]);
        assert_eq!(factory.spawn_count.load(Ordering::SeqCst), 1);
        tokio::time::advance(Duration::from_secs(1)).await;
        wait_for_phase(&supervisor, KernelHostPhase::Ready).await;
        supervisor.stop().await.unwrap();
        assert_eq!(factory.events(), vec!["running-force", "running-shutdown"]);
    }

    #[tokio::test]
    async fn wait_exit_failure_revokes_ready_before_force_reap_can_complete() {
        let instance = InstanceId::new(Uuid::new_v4());
        let force_reap_gate = Arc::new(Notify::new());
        let factory = Arc::new(ScriptedFactory::new([PendingScript::ready(
            ReadyEvidence {
                ready: NativeHostReady::new(43123, instance),
                authenticated_instance: instance,
            },
            RunningBehavior {
                exit: AsyncAction::Fail,
                graceful_stop: AsyncAction::Complete,
                force_reap: AsyncAction::Complete,
                force_reap_gate: Some(force_reap_gate.clone()),
            },
        )]));
        let bootstrap = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let supervisor = KernelHostSupervisor::new_with_bootstrap(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_secs(5)),
            bootstrap.clone(),
        );
        let access = supervisor.start(startup()).await.unwrap();
        wait_for_event(&factory, "running-force").await;

        assert!(!access.credential.is_available());
        assert_eq!(supervisor.snapshot().phase, KernelHostPhase::Failed);
        assert_eq!(supervisor.snapshot().generation, 1);
        assert_eq!(supervisor.snapshot().endpoint, None);
        assert_eq!(
            serde_json::to_value(bootstrap.read().unwrap()).unwrap(),
            json!({
                "status": "failed",
                "bootstrapVersion": 1,
                "generation": "1",
            })
        );

        force_reap_gate.notify_one();
    }

    #[tokio::test]
    async fn dropping_supervisor_while_starting_terminates_the_armed_child() {
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
        let bootstrap = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
        let supervisor = Arc::new(KernelHostSupervisor::new_with_bootstrap(
            factory.clone(),
            KernelHostTimeouts::uniform(Duration::from_secs(1)),
            bootstrap.clone(),
        ));
        let start_supervisor = supervisor.clone();
        let start = tokio::spawn(async move { start_supervisor.start(startup()).await });
        wait_for_spawn(&factory).await;
        start.abort();
        let _join_result = start.await;

        drop(supervisor);
        gate.notify_waiters();
        for _attempt in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            serde_json::to_value(bootstrap.read().unwrap()).unwrap(),
            json!({
                "status": "dormant",
                "bootstrapVersion": 1,
            })
        );
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

    async fn wait_for_event(factory: &ScriptedFactory, expected: &'static str) {
        for _attempt in 0..100 {
            if factory.events().contains(&expected) {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("supervisor did not publish event {expected}");
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

    async fn wait_for_generation(supervisor: &KernelHostSupervisor, generation: u64) {
        for _attempt in 0..100 {
            if supervisor.snapshot().generation == generation {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("supervisor did not reach generation {generation}");
    }

    fn recovery_timeouts(max_recovery_attempts: u8) -> KernelHostTimeouts {
        KernelHostTimeouts::uniform(Duration::from_millis(100)).with_recovery(
            Duration::from_secs(1),
            Duration::from_secs(4),
            max_recovery_attempts,
        )
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
