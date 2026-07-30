//! Reusable system-backed ports for native and server Kernel hosts.

use std::{fmt, sync::Arc, sync::Mutex, time::Duration};

use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    contract::Rfc3339Utc,
    events::{EventPublication, EventSink, EventSinkError},
    ports::{
        BoxSleepFuture, BoxTaskFuture, Clock, CredentialSecret, CredentialSlot, CredentialStore,
        DiagnosticRecord, DiagnosticsSink, KernelPorts, NetworkReachability, PortError, Sleeper,
        TaskSpawner,
    },
};

#[derive(Clone, Copy, Debug, Default)]
pub struct UtcSystemClock;

impl Clock for UtcSystemClock {
    fn now(&self) -> Result<Rfc3339Utc, PortError> {
        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| PortError::unavailable())?;
        Rfc3339Utc::parse(timestamp).map_err(|_| PortError::unavailable())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep(&self, duration: Duration) -> BoxSleepFuture<'_> {
        Box::pin(async move {
            tokio::runtime::Handle::try_current().map_err(|_| PortError::unavailable())?;
            tokio::time::sleep(duration).await;
            Ok(())
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TokioTaskSpawner;

impl TaskSpawner for TokioTaskSpawner {
    fn spawn(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        let handle = tokio::runtime::Handle::try_current().map_err(|_| PortError::unavailable())?;
        let _join_handle = handle.spawn(task);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn publish(&self, _publication: &EventPublication) -> Result<(), EventSinkError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopDiagnosticsSink;

impl DiagnosticsSink for NoopDiagnosticsSink {
    fn emit(&self, _record: DiagnosticRecord) -> Result<(), PortError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AlwaysReachableNetwork;

impl NetworkReachability for AlwaysReachableNetwork {
    fn is_reachable(&self) -> Result<bool, PortError> {
        Ok(true)
    }
}

#[derive(Default)]
pub struct MemoryCredentialStore {
    values: Mutex<MemoryCredentials>,
}

impl CredentialStore for MemoryCredentialStore {
    fn is_present(&self, slot: CredentialSlot) -> Result<bool, PortError> {
        let values = self.values.lock().map_err(|_| PortError::unavailable())?;
        Ok(values.value(slot).is_some())
    }

    fn replace(&self, slot: CredentialSlot, value: &CredentialSecret) -> Result<(), PortError> {
        let mut values = self.values.lock().map_err(|_| PortError::unavailable())?;
        *values.value_mut(slot) = Some(CredentialSecret::new(value.expose_secret().to_owned()));
        Ok(())
    }

    fn clear(&self, slot: CredentialSlot) -> Result<(), PortError> {
        let removed = {
            let mut values = self.values.lock().map_err(|_| PortError::unavailable())?;
            values.value_mut(slot).take()
        };
        drop(removed);
        Ok(())
    }
}

impl fmt::Debug for MemoryCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MemoryCredentialStore([REDACTED])")
    }
}

#[derive(Default)]
struct MemoryCredentials {
    webdav_password: Option<CredentialSecret>,
    s3_access_key_id: Option<CredentialSecret>,
    s3_secret_access_key: Option<CredentialSecret>,
}

impl MemoryCredentials {
    fn value(&self, slot: CredentialSlot) -> &Option<CredentialSecret> {
        match slot {
            CredentialSlot::WebDavPassword => &self.webdav_password,
            CredentialSlot::S3AccessKeyId => &self.s3_access_key_id,
            CredentialSlot::S3SecretAccessKey => &self.s3_secret_access_key,
        }
    }

    fn value_mut(&mut self, slot: CredentialSlot) -> &mut Option<CredentialSecret> {
        match slot {
            CredentialSlot::WebDavPassword => &mut self.webdav_password,
            CredentialSlot::S3AccessKeyId => &mut self.s3_access_key_id,
            CredentialSlot::S3SecretAccessKey => &mut self.s3_secret_access_key,
        }
    }
}

impl fmt::Debug for MemoryCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MemoryCredentials([REDACTED])")
    }
}

pub fn system_kernel_ports() -> KernelPorts {
    KernelPorts::new(
        Arc::new(NoopEventSink),
        Arc::new(UtcSystemClock),
        Arc::new(TokioSleeper),
        Arc::new(TokioTaskSpawner),
        Arc::new(MemoryCredentialStore::default()),
        Arc::new(NoopDiagnosticsSink),
        Arc::new(AlwaysReachableNetwork),
    )
}
