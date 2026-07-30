//! Platform-neutral scheduling for Kernel-owned sync triggers.

use std::{
    fmt,
    future::pending,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::Notify;

use crate::{
    contract::{DomainEvent, SyncConfigReadiness, SyncMode, SyncTrigger},
    events::EventSubscription,
    runtime::{KernelRuntime, SyncApiService as _},
    services::sync::{KernelSyncTriggerResult, SyncService},
};

pub struct KernelSyncScheduler {
    control: Arc<SchedulerControl>,
    service: Arc<SyncService>,
}

impl KernelSyncScheduler {
    pub fn start(
        runtime: Arc<KernelRuntime>,
        service: Arc<SyncService>,
    ) -> Result<Self, KernelSyncSchedulerStartError> {
        let control = Arc::new(SchedulerControl::default());
        let task_control = control.clone();
        let task_service = service.clone();
        let sleeper = runtime.ports().sleeper().clone();
        let subscription = runtime.event_broker().subscribe();
        let task_state = Arc::new(SchedulerTaskState {
            control: task_control.clone(),
            dropped: AtomicBool::new(false),
        });
        let task = SchedulerTask {
            state: task_state.clone(),
            inner: Box::pin(run_scheduler(
                task_control,
                task_service,
                sleeper,
                subscription,
            )),
        };
        let spawn_result = runtime.ports().spawn_background(Box::pin(task));
        if spawn_result.is_err() || task_state.dropped.load(Ordering::Acquire) {
            control.close(&service);
            return Err(KernelSyncSchedulerStartError);
        }
        Ok(Self { control, service })
    }

    pub async fn app_launch(&self) -> KernelSyncTriggerResult {
        self.service
            .trigger_kernel_sync(SyncTrigger::AppLaunch)
            .await
    }

    pub async fn save(&self) -> KernelSyncTriggerResult {
        self.service.trigger_kernel_sync(SyncTrigger::Save).await
    }

    pub async fn settings_exit(&self) -> KernelSyncTriggerResult {
        self.service
            .trigger_kernel_sync(SyncTrigger::SettingsExit)
            .await
    }

    pub async fn close(&self) {
        self.control.close(&self.service);
        self.control.wait_ended().await;
    }
}

impl Drop for KernelSyncScheduler {
    fn drop(&mut self) {
        self.control.close(&self.service);
    }
}

impl fmt::Debug for KernelSyncScheduler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KernelSyncScheduler(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSyncSchedulerStartError;

impl fmt::Display for KernelSyncSchedulerStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the Kernel sync scheduler could not start")
    }
}

impl std::error::Error for KernelSyncSchedulerStartError {}

#[derive(Default)]
struct SchedulerControl {
    closed: AtomicBool,
    ended: AtomicBool,
    ended_notification: Notify,
    wake: Notify,
}

impl SchedulerControl {
    fn close(&self, service: &SyncService) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            service.close_kernel_triggers();
            self.wake.notify_waiters();
        }
    }

    fn finish(&self) {
        self.ended.store(true, Ordering::Release);
        self.ended_notification.notify_waiters();
    }

    async fn wait_closed(&self) {
        loop {
            if self.closed.load(Ordering::Acquire) {
                return;
            }
            let notified = self.wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.closed.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    async fn wait_ended(&self) {
        loop {
            if self.ended.load(Ordering::Acquire) {
                return;
            }
            let notified = self.ended_notification.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.ended.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

struct SchedulerTaskState {
    control: Arc<SchedulerControl>,
    dropped: AtomicBool,
}

struct SchedulerTask {
    state: Arc<SchedulerTaskState>,
    inner: crate::ports::BoxTaskFuture,
}

impl std::future::Future for SchedulerTask {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.inner.as_mut().poll(context)
    }
}

impl Drop for SchedulerTask {
    fn drop(&mut self) {
        self.state.dropped.store(true, Ordering::Release);
        self.state.control.finish();
    }
}

async fn run_scheduler(
    control: Arc<SchedulerControl>,
    service: Arc<SyncService>,
    sleeper: Arc<dyn crate::ports::Sleeper>,
    mut subscription: EventSubscription,
) {
    loop {
        if control.closed.load(Ordering::Acquire) {
            return;
        }
        let interval = current_interval(service.as_ref()).await;
        let Some(interval) = interval else {
            tokio::select! {
                _closed = control.wait_closed() => return,
                _changed = wait_for_config_change(&mut subscription) => {}
            }
            continue;
        };
        let sleep = sleeper.sleep(interval);
        tokio::pin!(sleep);
        let slept = tokio::select! {
            _closed = control.wait_closed() => return,
            _changed = wait_for_config_change(&mut subscription) => continue,
            result = &mut sleep => result,
        };
        if slept.is_err() {
            tokio::select! {
                _closed = control.wait_closed() => return,
                _changed = wait_for_config_change(&mut subscription) => {}
            }
            continue;
        }
        let (_disposition, settlement) = service
            .trigger_kernel_sync(SyncTrigger::Interval)
            .await
            .into_parts();
        settlement.wait().await;
    }
}

async fn current_interval(service: &SyncService) -> Option<Duration> {
    let config = service.get_sync_config().await.ok()?;
    if config.enabled
        && config.readiness == SyncConfigReadiness::Ready
        && config.mode == SyncMode::Automatic
    {
        Some(Duration::from_secs(u64::from(
            config.interval_seconds.get(),
        )))
    } else {
        None
    }
}

async fn wait_for_config_change(subscription: &mut EventSubscription) {
    loop {
        match subscription.recv().await {
            Ok(publication)
                if matches!(publication.event, DomainEvent::SyncConfigChanged { .. }) =>
            {
                return;
            }
            Ok(_) => {}
            Err(crate::events::EventReceiveError::Lagged) => return,
            Err(crate::events::EventReceiveError::Closed) => pending().await,
        }
    }
}
