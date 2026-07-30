pub mod system;

use std::{fmt, future::Future, pin::Pin, sync::Arc, time::Duration};

use zeroize::Zeroize as _;

use crate::{
    contract::Rfc3339Utc,
    events::{EventPublication, EventSink, EventSinkError},
    workspace::lock::InstanceLockLease,
};

pub type BoxTaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
pub type BoxSleepFuture<'a> = Pin<Box<dyn Future<Output = Result<(), PortError>> + Send + 'a>>;

pub trait Clock: Send + Sync {
    fn now(&self) -> Result<Rfc3339Utc, PortError>;
}

pub trait Sleeper: Send + Sync {
    fn sleep(&self, duration: Duration) -> BoxSleepFuture<'_>;
}

pub trait TaskSpawner: Send + Sync {
    fn spawn(&self, task: BoxTaskFuture) -> Result<(), PortError>;
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CredentialSlot {
    WebDavPassword,
    S3AccessKeyId,
    S3SecretAccessKey,
}

pub struct CredentialSecret(String);

impl CredentialSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CredentialSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialSecret([REDACTED])")
    }
}

impl Drop for CredentialSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub trait CredentialStore: Send + Sync {
    fn is_present(&self, slot: CredentialSlot) -> Result<bool, PortError>;
    fn replace(&self, slot: CredentialSlot, value: &CredentialSecret) -> Result<(), PortError>;
    fn clear(&self, slot: CredentialSlot) -> Result<(), PortError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticRecord {
    pub code: &'static str,
}

pub trait DiagnosticsSink: Send + Sync {
    fn emit(&self, record: DiagnosticRecord) -> Result<(), PortError>;
}

pub trait NetworkReachability: Send + Sync {
    fn is_reachable(&self) -> Result<bool, PortError>;
}

pub struct KernelPorts {
    event_sink: Arc<dyn EventSink>,
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
    task_spawner: Arc<dyn TaskSpawner>,
    instance_lease: Option<Arc<InstanceLockLease>>,
    credential_store: Arc<dyn CredentialStore>,
    diagnostics: Arc<dyn DiagnosticsSink>,
    network_reachability: Arc<dyn NetworkReachability>,
}

impl KernelPorts {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_sink: Arc<dyn EventSink>,
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
        task_spawner: Arc<dyn TaskSpawner>,
        credential_store: Arc<dyn CredentialStore>,
        diagnostics: Arc<dyn DiagnosticsSink>,
        network_reachability: Arc<dyn NetworkReachability>,
    ) -> Self {
        Self {
            event_sink,
            clock,
            sleeper,
            task_spawner,
            instance_lease: None,
            credential_store,
            diagnostics,
            network_reachability,
        }
    }

    pub fn unavailable() -> Self {
        let unavailable = Arc::new(UnavailablePort);
        Self::new(
            unavailable.clone(),
            unavailable.clone(),
            unavailable.clone(),
            unavailable.clone(),
            unavailable.clone(),
            unavailable.clone(),
            unavailable,
        )
    }

    pub fn event_sink(&self) -> &Arc<dyn EventSink> {
        &self.event_sink
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    pub fn sleeper(&self) -> &Arc<dyn Sleeper> {
        &self.sleeper
    }

    pub(crate) fn bind_instance_lease(&mut self, lease: Arc<InstanceLockLease>) {
        assert!(self.instance_lease.replace(lease).is_none());
    }

    pub(crate) fn spawn_background(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        let lease = self
            .instance_lease
            .as_ref()
            .cloned()
            .ok_or_else(PortError::unavailable)?;
        self.task_spawner.spawn(Box::pin(async move {
            task.await;
            drop(lease);
        }))
    }

    pub(crate) fn spawn_unretained_background(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        self.task_spawner.spawn(task)
    }

    pub fn credential_store(&self) -> &Arc<dyn CredentialStore> {
        &self.credential_store
    }

    pub fn diagnostics(&self) -> &Arc<dyn DiagnosticsSink> {
        &self.diagnostics
    }

    pub fn network_reachability(&self) -> &Arc<dyn NetworkReachability> {
        &self.network_reachability
    }
}

impl fmt::Debug for KernelPorts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KernelPorts(..)")
    }
}

struct UnavailablePort;

impl EventSink for UnavailablePort {
    fn publish(&self, _publication: &EventPublication) -> Result<(), EventSinkError> {
        Err(EventSinkError)
    }
}

impl Clock for UnavailablePort {
    fn now(&self) -> Result<Rfc3339Utc, PortError> {
        Err(PortError::unavailable())
    }
}

impl Sleeper for UnavailablePort {
    fn sleep(&self, _duration: Duration) -> BoxSleepFuture<'_> {
        Box::pin(async { Err(PortError::unavailable()) })
    }
}

impl TaskSpawner for UnavailablePort {
    fn spawn(&self, _task: BoxTaskFuture) -> Result<(), PortError> {
        Err(PortError::unavailable())
    }
}

impl CredentialStore for UnavailablePort {
    fn is_present(&self, _slot: CredentialSlot) -> Result<bool, PortError> {
        Err(PortError::unavailable())
    }

    fn replace(&self, _slot: CredentialSlot, _value: &CredentialSecret) -> Result<(), PortError> {
        Err(PortError::unavailable())
    }

    fn clear(&self, _slot: CredentialSlot) -> Result<(), PortError> {
        Err(PortError::unavailable())
    }
}

impl DiagnosticsSink for UnavailablePort {
    fn emit(&self, _record: DiagnosticRecord) -> Result<(), PortError> {
        Err(PortError::unavailable())
    }
}

impl NetworkReachability for UnavailablePort {
    fn is_reachable(&self) -> Result<bool, PortError> {
        Err(PortError::unavailable())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortErrorKind {
    Unavailable,
    Rejected,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PortError {
    kind: PortErrorKind,
}

impl PortError {
    pub const fn new(kind: PortErrorKind) -> Self {
        Self { kind }
    }

    pub const fn unavailable() -> Self {
        Self::new(PortErrorKind::Unavailable)
    }

    pub const fn kind(self) -> PortErrorKind {
        self.kind
    }
}

impl fmt::Debug for PortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for PortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            PortErrorKind::Unavailable => formatter.write_str("a Kernel port is unavailable"),
            PortErrorKind::Rejected => formatter.write_str("a Kernel port rejected the operation"),
        }
    }
}

impl std::error::Error for PortError {}
