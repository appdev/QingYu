use std::sync::{Arc, Mutex, MutexGuard};

use crate::kernel_host::NativeKernelAccess;

pub(crate) struct KernelEndpointRecord;

pub(crate) struct KernelEndpointRecordWriter {
    shared: Arc<KernelEndpointRecordShared>,
}

#[derive(Clone)]
pub(crate) struct KernelEndpointRecordReader {
    shared: Arc<KernelEndpointRecordShared>,
}

pub(crate) struct KernelEndpointRecordTransaction<'a> {
    state: MutexGuard<'a, KernelEndpointRecordState>,
}

struct KernelEndpointRecordShared {
    state: Mutex<KernelEndpointRecordState>,
}

struct KernelEndpointRecordState {
    access: Option<NativeKernelAccess>,
    last_generation: u64,
    closed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KernelEndpointRecordError {
    Closed,
    StaleGeneration,
    Unavailable,
}

impl KernelEndpointRecord {
    pub(crate) fn create() -> (KernelEndpointRecordWriter, KernelEndpointRecordReader) {
        let shared = Arc::new(KernelEndpointRecordShared {
            state: Mutex::new(KernelEndpointRecordState {
                access: None,
                last_generation: 0,
                closed: false,
            }),
        });
        (
            KernelEndpointRecordWriter {
                shared: Arc::clone(&shared),
            },
            KernelEndpointRecordReader { shared },
        )
    }
}

impl KernelEndpointRecordWriter {
    pub(crate) fn replace(
        &self,
        access: NativeKernelAccess,
    ) -> Result<(), KernelEndpointRecordError> {
        let mut transaction = match self.transaction() {
            Ok(transaction) => transaction,
            Err(error) => {
                access.credential.revoke();
                return Err(error);
            }
        };
        transaction.replace(access)
    }

    /// Retires only an endpoint no newer than the committing lifecycle edge.
    /// A delayed stale edge therefore cannot clear a recovered endpoint.
    pub(crate) fn clear_through(&self, generation: u64) -> Result<bool, KernelEndpointRecordError> {
        Ok(self.transaction()?.clear_through(generation))
    }

    pub(crate) fn transaction(
        &self,
    ) -> Result<KernelEndpointRecordTransaction<'_>, KernelEndpointRecordError> {
        let state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                revoke_current(&mut state);
                state.closed = true;
                return Err(KernelEndpointRecordError::Unavailable);
            }
        };
        if state.closed {
            return Err(KernelEndpointRecordError::Closed);
        }
        Ok(KernelEndpointRecordTransaction { state })
    }

    pub(crate) fn reader(&self) -> KernelEndpointRecordReader {
        KernelEndpointRecordReader {
            shared: Arc::clone(&self.shared),
        }
    }

    pub(crate) fn close(&self) {
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        revoke_current(&mut state);
        state.closed = true;
    }
}

impl KernelEndpointRecordTransaction<'_> {
    pub(crate) fn replace(
        &mut self,
        access: NativeKernelAccess,
    ) -> Result<(), KernelEndpointRecordError> {
        if access.endpoint.generation <= self.state.last_generation {
            access.credential.revoke();
            return Err(KernelEndpointRecordError::StaleGeneration);
        }
        revoke_current(&mut self.state);
        self.state.last_generation = access.endpoint.generation;
        self.state.access = Some(access);
        Ok(())
    }

    pub(crate) fn clear_through(&mut self, generation: u64) -> bool {
        let should_clear = self
            .state
            .access
            .as_ref()
            .is_some_and(|access| access.endpoint.generation <= generation);
        if should_clear {
            revoke_current(&mut self.state);
        }
        should_clear
    }
}

impl Drop for KernelEndpointRecordWriter {
    fn drop(&mut self) {
        self.close();
    }
}

impl KernelEndpointRecordReader {
    pub(crate) fn read(&self) -> Result<Option<NativeKernelAccess>, KernelEndpointRecordError> {
        let state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                revoke_current(&mut state);
                state.closed = true;
                return Err(KernelEndpointRecordError::Unavailable);
            }
        };
        if state.closed {
            return Ok(None);
        }
        Ok(state.access.clone())
    }
}

fn revoke_current(state: &mut KernelEndpointRecordState) {
    if let Some(access) = state.access.take() {
        access.credential.revoke();
    }
}

#[cfg(test)]
mod tests {
    use qingyu_kernel::{config::NativeLaunchCredential, contract::InstanceId};
    use uuid::Uuid;

    use crate::kernel_host::{KernelEndpoint, NativeKernelAccess, NativeKernelCredentialLease};

    use super::{KernelEndpointRecord, KernelEndpointRecordError};

    const CREDENTIAL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn replacement_is_strictly_monotonic_and_revokes_every_retired_credential() {
        let (writer, reader) = KernelEndpointRecord::create();
        let first = access(1);
        let first_credential = first.credential.clone();
        writer.replace(first).unwrap();

        let second = access(2);
        let second_credential = second.credential.clone();
        writer.replace(second).unwrap();

        assert!(!first_credential.is_available());
        assert_eq!(reader.read().unwrap().unwrap().endpoint.generation, 2);

        let stale = access(2);
        let stale_credential = stale.credential.clone();
        assert_eq!(
            writer.replace(stale),
            Err(KernelEndpointRecordError::StaleGeneration)
        );
        assert!(!stale_credential.is_available());
        assert!(second_credential.is_available());
    }

    #[test]
    fn stale_clear_cannot_retire_fresh_access_but_matching_clear_can() {
        let (writer, reader) = KernelEndpointRecord::create();
        let current = access(2);
        let credential = current.credential.clone();
        writer.replace(current).unwrap();

        assert!(!writer.clear_through(1).unwrap());
        assert_eq!(reader.read().unwrap().unwrap().endpoint.generation, 2);
        assert!(credential.is_available());

        assert!(writer.clear_through(2).unwrap());
        assert!(reader.read().unwrap().is_none());
        assert!(!credential.is_available());
    }

    #[test]
    fn close_revokes_current_access_and_rejects_and_revokes_future_candidates() {
        let (writer, reader) = KernelEndpointRecord::create();
        let current = access(1);
        let current_credential = current.credential.clone();
        writer.replace(current).unwrap();

        writer.close();

        assert!(!current_credential.is_available());
        assert!(reader.read().unwrap().is_none());
        let candidate = access(2);
        let candidate_credential = candidate.credential.clone();
        assert_eq!(
            writer.replace(candidate),
            Err(KernelEndpointRecordError::Closed)
        );
        assert!(!candidate_credential.is_available());
    }

    #[test]
    fn dropping_the_only_writer_closes_the_record_without_giving_the_reader_write_access() {
        let (writer, reader) = KernelEndpointRecord::create();
        let current = access(1);
        let credential = current.credential.clone();
        writer.replace(current).unwrap();

        drop(writer);

        assert!(!credential.is_available());
        assert!(reader.read().unwrap().is_none());
        let reader_clone = reader.clone();
        assert!(reader_clone.read().unwrap().is_none());
    }

    fn access(generation: u64) -> NativeKernelAccess {
        NativeKernelAccess {
            endpoint: KernelEndpoint {
                generation,
                port: 49_152,
                instance_id: InstanceId::new(Uuid::new_v4()),
            },
            credential: NativeKernelCredentialLease::new(
                NativeLaunchCredential::from_secret(CREDENTIAL.to_owned()).unwrap(),
            ),
        }
    }
}
