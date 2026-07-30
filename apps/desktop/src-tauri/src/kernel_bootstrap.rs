//! Dormant desktop-to-Kernel bootstrap publication boundary.
//!
//! The owner can retain a supervisor-issued same-generation endpoint and
//! parent credential lease, but production only installs an empty owner. Ready
//! publication remains unreachable until every legacy workspace writer is
//! disabled in the same atomic cutover.

use std::{
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use qingyu_kernel::contract::InstanceId;
use serde::Serializer;

use crate::kernel_host::{NativeKernelAccess, NativeKernelCredentialLease};

const NATIVE_KERNEL_BOOTSTRAP_VERSION: u16 = 1;

#[derive(serde::Serialize)]
#[serde(transparent)]
pub(crate) struct NativeKernelBootstrap(NativeKernelBootstrapRepresentation);

#[derive(serde::Serialize)]
#[serde(untagged)]
enum NativeKernelBootstrapRepresentation {
    Dormant(NativeKernelDormantBootstrap),
    Lifecycle(NativeKernelLifecycleBootstrap),
    #[cfg_attr(not(test), allow(dead_code))]
    Ready(NativeKernelReadyBootstrap),
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeKernelDormantBootstrap {
    status: NativeKernelBootstrapStatus,
    bootstrap_version: u16,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum NativeKernelBootstrapStatus {
    Dormant,
    Starting,
    Retrying,
    #[cfg_attr(not(test), allow(dead_code))]
    Ready,
    Failed,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeKernelLifecycleBootstrap {
    status: NativeKernelBootstrapStatus,
    bootstrap_version: u16,
    generation: NativeKernelBootstrapGeneration,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeKernelReadyBootstrap {
    status: NativeKernelBootstrapStatus,
    bootstrap_version: u16,
    generation: NativeKernelBootstrapGeneration,
    port: u16,
    instance_id: InstanceId,
    credential: NativeKernelBootstrapCredential,
}

struct NativeKernelBootstrapGeneration(u64);

impl serde::Serialize for NativeKernelBootstrapGeneration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

#[cfg_attr(not(test), allow(dead_code))]
struct NativeKernelBootstrapCredential(NativeKernelCredentialLease);

impl serde::Serialize for NativeKernelBootstrapCredential {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0
            .with_secret(|secret| serializer.serialize_str(secret))
            .map_err(|_| serde::ser::Error::custom("native Kernel credential unavailable"))?
    }
}

impl fmt::Debug for NativeKernelBootstrapCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl NativeKernelBootstrap {
    fn dormant() -> Self {
        Self(NativeKernelBootstrapRepresentation::Dormant(
            NativeKernelDormantBootstrap {
                status: NativeKernelBootstrapStatus::Dormant,
                bootstrap_version: NATIVE_KERNEL_BOOTSTRAP_VERSION,
            },
        ))
    }

    fn ready(access: NativeKernelAccess) -> Self {
        Self(NativeKernelBootstrapRepresentation::Ready(
            NativeKernelReadyBootstrap {
                status: NativeKernelBootstrapStatus::Ready,
                bootstrap_version: NATIVE_KERNEL_BOOTSTRAP_VERSION,
                generation: NativeKernelBootstrapGeneration(access.endpoint.generation),
                port: access.endpoint.port,
                instance_id: access.endpoint.instance_id,
                credential: NativeKernelBootstrapCredential(access.credential),
            },
        ))
    }

    fn lifecycle(status: NativeKernelBootstrapStatus, generation: u64) -> Self {
        Self(NativeKernelBootstrapRepresentation::Lifecycle(
            NativeKernelLifecycleBootstrap {
                status,
                bootstrap_version: NATIVE_KERNEL_BOOTSTRAP_VERSION,
                generation: NativeKernelBootstrapGeneration(generation),
            },
        ))
    }
}

#[derive(Clone)]
pub(crate) struct NativeKernelBootstrapOwner {
    shared: Arc<NativeKernelBootstrapShared>,
}

struct NativeKernelBootstrapShared {
    state: Mutex<NativeKernelBootstrapState>,
}

struct NativeKernelBootstrapState {
    publication: NativeKernelBootstrapPublication,
    last_generation: u64,
    next_supervisor_epoch: u64,
    active_supervisor_epoch: Option<u64>,
}

#[derive(Clone)]
pub(crate) struct NativeKernelBootstrapSession {
    inner: Arc<NativeKernelBootstrapSessionInner>,
}

struct NativeKernelBootstrapSessionInner {
    shared: Arc<NativeKernelBootstrapShared>,
    epoch: Option<u64>,
    closed: AtomicBool,
}

enum NativeKernelBootstrapPublication {
    Dormant,
    Lifecycle {
        status: NativeKernelBootstrapStatus,
        generation: u64,
    },
    Ready(NativeKernelAccess),
}

impl NativeKernelBootstrapPublication {
    fn generation(&self) -> Option<u64> {
        match self {
            Self::Dormant => None,
            Self::Lifecycle { generation, .. } => Some(*generation),
            Self::Ready(access) => Some(access.endpoint.generation),
        }
    }

    fn revoke_access(&mut self) {
        if let Self::Ready(access) = self {
            access.credential.revoke();
        }
    }
}

impl NativeKernelBootstrapOwner {
    pub(crate) fn new() -> Self {
        Self {
            shared: Arc::new(NativeKernelBootstrapShared {
                state: Mutex::new(NativeKernelBootstrapState {
                    publication: NativeKernelBootstrapPublication::Dormant,
                    last_generation: 0,
                    next_supervisor_epoch: 0,
                    active_supervisor_epoch: None,
                }),
            }),
        }
    }

    pub(crate) fn read(&self) -> Result<NativeKernelBootstrap, String> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| bootstrap_unavailable())?;
        Ok(match &state.publication {
            NativeKernelBootstrapPublication::Dormant => NativeKernelBootstrap::dormant(),
            NativeKernelBootstrapPublication::Lifecycle { status, generation } => {
                NativeKernelBootstrap::lifecycle(*status, *generation)
            }
            NativeKernelBootstrapPublication::Ready(access) => {
                NativeKernelBootstrap::ready(access.clone())
            }
        })
    }

    pub(crate) fn open_supervisor_session(&self) -> NativeKernelBootstrapSession {
        let epoch = match self.shared.state.lock() {
            Ok(mut state) => match state.next_supervisor_epoch.checked_add(1) {
                Some(epoch) => {
                    state.next_supervisor_epoch = epoch;
                    Some(epoch)
                }
                None => None,
            },
            Err(_) => None,
        };
        NativeKernelBootstrapSession {
            inner: Arc::new(NativeKernelBootstrapSessionInner {
                shared: Arc::clone(&self.shared),
                epoch,
                closed: AtomicBool::new(epoch.is_none()),
            }),
        }
    }
}

impl NativeKernelBootstrapSession {
    pub(crate) fn last_generation(&self) -> Result<u64, String> {
        if self.is_closed() {
            return Err(bootstrap_unavailable());
        }
        self.inner
            .shared
            .state
            .lock()
            .map(|state| state.last_generation)
            .map_err(|_| bootstrap_unavailable())
    }

    pub(crate) fn publish(&self, access: NativeKernelAccess) -> Result<(), String> {
        if self.is_closed() {
            access.credential.revoke();
            return Err(bootstrap_unavailable());
        }
        let mut state = match self.inner.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                access.credential.revoke();
                let mut state = poisoned.into_inner();
                if self.owns_transition(&state) {
                    state.publication.revoke_access();
                }
                return Err(bootstrap_unavailable());
            }
        };
        if self.is_closed() || !self.owns_transition(&state) {
            access.credential.revoke();
            return Err(bootstrap_unavailable());
        }
        let generation = access.endpoint.generation;
        let same_generation_transition = state.last_generation == generation
            && matches!(
                state.publication,
                NativeKernelBootstrapPublication::Lifecycle {
                    status: NativeKernelBootstrapStatus::Starting
                        | NativeKernelBootstrapStatus::Retrying,
                    generation: current,
                } if current == generation
            );
        if state.last_generation > generation
            || (state.last_generation == generation && !same_generation_transition)
        {
            access.credential.revoke();
            return Err(bootstrap_unavailable());
        }
        state.last_generation = generation;
        state.publication.revoke_access();
        state.publication = NativeKernelBootstrapPublication::Ready(access);
        Ok(())
    }

    pub(crate) fn begin_start(&self, generation: u64) -> Result<(), String> {
        if self.is_closed() {
            return Err(bootstrap_unavailable());
        }
        let mut state = self
            .inner
            .shared
            .state
            .lock()
            .map_err(|_| bootstrap_unavailable())?;
        if self.is_closed() || generation <= state.last_generation {
            return Err(bootstrap_unavailable());
        }
        match state.active_supervisor_epoch {
            None => state.active_supervisor_epoch = self.inner.epoch,
            Some(epoch) if Some(epoch) == self.inner.epoch => {}
            Some(_) => return Err(bootstrap_unavailable()),
        }
        state.publication.revoke_access();
        state.last_generation = generation;
        state.publication = NativeKernelBootstrapPublication::Lifecycle {
            status: NativeKernelBootstrapStatus::Starting,
            generation,
        };
        Ok(())
    }

    pub(crate) fn begin_retry(&self, generation: u64) -> Result<(), String> {
        self.begin_owned_lifecycle(NativeKernelBootstrapStatus::Retrying, generation)
    }

    pub(crate) fn continue_start(&self, generation: u64) -> Result<(), String> {
        let mut state = self
            .inner
            .shared
            .state
            .lock()
            .map_err(|_| bootstrap_unavailable())?;
        if self.is_closed()
            || !self.owns_transition(&state)
            || !matches!(
                state.publication,
                NativeKernelBootstrapPublication::Lifecycle {
                    status: NativeKernelBootstrapStatus::Retrying,
                    generation: current,
                } if current == generation
            )
        {
            return Err(bootstrap_unavailable());
        }
        state.publication = NativeKernelBootstrapPublication::Lifecycle {
            status: NativeKernelBootstrapStatus::Starting,
            generation,
        };
        Ok(())
    }

    fn begin_owned_lifecycle(
        &self,
        status: NativeKernelBootstrapStatus,
        generation: u64,
    ) -> Result<(), String> {
        if self.is_closed() {
            return Err(bootstrap_unavailable());
        }
        let mut state = self
            .inner
            .shared
            .state
            .lock()
            .map_err(|_| bootstrap_unavailable())?;
        if self.is_closed() || !self.owns_transition(&state) || generation <= state.last_generation
        {
            return Err(bootstrap_unavailable());
        }
        state.publication.revoke_access();
        state.last_generation = generation;
        state.publication = NativeKernelBootstrapPublication::Lifecycle { status, generation };
        Ok(())
    }

    pub(crate) fn fail_generation(&self, generation: u64) -> Result<bool, String> {
        self.finish_generation(NativeKernelBootstrapStatus::Failed, generation)
    }

    pub(crate) fn finish_stop(&self, generation: u64) -> Result<bool, String> {
        self.finish_generation(NativeKernelBootstrapStatus::Dormant, generation)
    }

    fn finish_generation(
        &self,
        status: NativeKernelBootstrapStatus,
        generation: u64,
    ) -> Result<bool, String> {
        if self.is_closed() {
            return Ok(false);
        }
        let mut state = self
            .inner
            .shared
            .state
            .lock()
            .map_err(|_| bootstrap_unavailable())?;
        if self.is_closed()
            || !self.owns_transition(&state)
            || state.publication.generation() != Some(generation)
        {
            return Ok(false);
        }
        state.publication.revoke_access();
        state.publication = NativeKernelBootstrapPublication::Lifecycle { status, generation };
        Ok(true)
    }

    pub(crate) fn clear_generation(&self, generation: u64) -> Result<bool, String> {
        if self.is_closed() {
            return Ok(false);
        }
        let cleared = match self.inner.shared.state.lock() {
            Ok(mut state) => {
                if !self.is_closed()
                    && self.owns_transition(&state)
                    && state.publication.generation() == Some(generation)
                {
                    state.publication.revoke_access();
                    state.publication = NativeKernelBootstrapPublication::Dormant;
                    true
                } else {
                    false
                }
            }
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                if self.owns_transition(&state) {
                    state.publication.revoke_access();
                }
                return Err(bootstrap_unavailable());
            }
        };
        Ok(cleared)
    }

    pub(crate) fn close(&self) {
        self.inner.close();
    }

    fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst) || self.inner.epoch.is_none()
    }

    fn owns_transition(&self, state: &NativeKernelBootstrapState) -> bool {
        self.inner.epoch.is_some() && state.active_supervisor_epoch == self.inner.epoch
    }
}

impl NativeKernelBootstrapSessionInner {
    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if self.epoch.is_some() && state.active_supervisor_epoch == self.epoch {
            state.publication.revoke_access();
            state.publication = NativeKernelBootstrapPublication::Dormant;
            state.active_supervisor_epoch = None;
        }
    }
}

impl Drop for NativeKernelBootstrapSessionInner {
    fn drop(&mut self) {
        self.close();
    }
}

impl Drop for NativeKernelBootstrapShared {
    fn drop(&mut self) {
        let state = match self.state.get_mut() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.publication.revoke_access();
    }
}

fn bootstrap_unavailable() -> String {
    "native Kernel bootstrap unavailable".to_owned()
}

#[tauri::command]
pub(crate) fn read_native_kernel_bootstrap(
    owner: tauri::State<'_, NativeKernelBootstrapOwner>,
) -> Result<NativeKernelBootstrap, String> {
    owner.read()
}

#[cfg(test)]
mod tests {
    use qingyu_kernel::contract::InstanceId;
    use serde_json::json;
    use uuid::Uuid;

    use crate::kernel_host::{KernelEndpoint, NativeKernelAccess, NativeKernelLaunch};

    const CREDENTIAL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const INSTANCE_ID: &str = "123e4567-e89b-42d3-a456-426614174000";

    #[test]
    fn production_bootstrap_is_exactly_version_one_dormant() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let value = serde_json::to_value(owner.read().unwrap())
            .expect("dormant bootstrap should serialize");

        assert_eq!(
            value,
            json!({
                "status": "dormant",
                "bootstrapVersion": 1,
            })
        );
    }

    #[test]
    fn future_ready_bootstrap_has_the_exact_version_one_wire_shape() {
        let (owner, _session, _temporary, credential) = ready_owner(1);
        let value = serde_json::to_value(owner.read().unwrap())
            .expect("future ready bootstrap should serialize");

        assert_eq!(
            value,
            json!({
                "status": "ready",
                "bootstrapVersion": 1,
                "generation": "1",
                "port": 49152,
                "instanceId": INSTANCE_ID,
                "credential": credential,
            })
        );
    }

    #[test]
    fn future_ready_generation_is_a_lossless_decimal_string() {
        let (owner, _session, _temporary, _credential) = ready_owner(u64::MAX);
        let value = serde_json::to_value(owner.read().unwrap())
            .expect("future ready bootstrap should serialize");

        assert_eq!(
            value.get("generation"),
            Some(&json!("18446744073709551615"))
        );
        assert!(!value
            .as_object()
            .expect("bootstrap should be an object")
            .contains_key("workspaceRoot"));
        assert!(!value
            .as_object()
            .expect("bootstrap should be an object")
            .contains_key("origin"));
    }

    #[test]
    fn future_ready_credential_debug_is_fixed_redaction() {
        let (access, _temporary) = ready_access(1);
        let credential = super::NativeKernelBootstrapCredential(access.credential);

        assert_eq!(format!("{credential:?}"), "[REDACTED]");
    }

    #[test]
    fn clearing_ready_bootstrap_revokes_the_published_credential() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let session = owner.open_supervisor_session();
        let (access, _temporary) = ready_access(1);
        let observed = access.credential.clone();
        session.begin_start(1).unwrap();
        session.publish(access).unwrap();

        session.close();

        assert!(observed.with_secret(str::to_owned).is_err());
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap(),
            json!({
                "status": "dormant",
                "bootstrapVersion": 1,
            })
        );
    }

    #[test]
    fn replacing_ready_bootstrap_revokes_only_the_previous_generation() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let session = owner.open_supervisor_session();
        let (first, _first_temporary) = ready_access(1);
        let first_credential = first.credential.clone();
        let (second, _second_temporary) = ready_access(2);
        let second_credential = second.credential.clone();
        let second_secret = second_credential.with_secret(str::to_owned).unwrap();
        session.begin_start(1).unwrap();
        session.publish(first).unwrap();

        session.begin_start(2).unwrap();
        session.publish(second).unwrap();

        assert!(first_credential.with_secret(str::to_owned).is_err());
        assert_eq!(
            second_credential.with_secret(str::to_owned).unwrap(),
            second_secret
        );
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap()["generation"],
            json!("2")
        );
    }

    #[test]
    fn stale_or_duplicate_publication_is_rejected_without_replacing_ready() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let session = owner.open_supervisor_session();
        let (current, _current_temporary) = ready_access(2);
        let current_credential = current.credential.clone();
        let current_secret = current_credential.with_secret(str::to_owned).unwrap();
        session.begin_start(2).unwrap();
        session.publish(current).unwrap();
        for generation in [1, 2] {
            let (candidate, _candidate_temporary) = ready_access(generation);
            let candidate_credential = candidate.credential.clone();

            assert!(session.publish(candidate).is_err());
            assert!(candidate_credential.with_secret(str::to_owned).is_err());
        }

        assert_eq!(
            current_credential.with_secret(str::to_owned).unwrap(),
            current_secret
        );
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap()["generation"],
            json!("2")
        );
    }

    #[test]
    fn clearing_ready_keeps_the_generation_fence_against_delayed_publication() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let session = owner.open_supervisor_session();
        let (current, _current_temporary) = ready_access(2);
        session.begin_start(2).unwrap();
        session.publish(current).unwrap();
        session.clear_generation(2).unwrap();

        for generation in [1, 2] {
            let (candidate, _candidate_temporary) = ready_access(generation);
            let candidate_credential = candidate.credential.clone();
            assert!(session.publish(candidate).is_err());
            assert!(candidate_credential.with_secret(str::to_owned).is_err());
        }
        let (next, _next_temporary) = ready_access(3);
        session.begin_start(3).unwrap();
        session.publish(next).unwrap();
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap()["generation"],
            json!("3")
        );
    }

    #[test]
    fn generation_scoped_clear_never_revokes_a_newer_publication() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let session = owner.open_supervisor_session();
        let (current, _current_temporary) = ready_access(2);
        let credential = current.credential.clone();
        let secret = credential.with_secret(str::to_owned).unwrap();
        session.begin_start(2).unwrap();
        session.publish(current).unwrap();

        assert!(!session.clear_generation(1).unwrap());
        assert_eq!(credential.with_secret(str::to_owned).unwrap(), secret);
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap()["generation"],
            json!("2")
        );

        assert!(session.clear_generation(2).unwrap());
        assert!(credential.with_secret(str::to_owned).is_err());
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap()["status"],
            json!("dormant")
        );
    }

    #[test]
    fn lifecycle_statuses_publish_generation_without_a_credential() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let session = owner.open_supervisor_session();

        session.begin_start(1).unwrap();
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap(),
            json!({
                "status": "starting",
                "bootstrapVersion": 1,
                "generation": "1",
            })
        );

        session.begin_retry(2).unwrap();
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap(),
            json!({
                "status": "retrying",
                "bootstrapVersion": 1,
                "generation": "2",
            })
        );

        assert!(session.fail_generation(2).unwrap());
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap(),
            json!({
                "status": "failed",
                "bootstrapVersion": 1,
                "generation": "2",
            })
        );

        assert!(session.finish_stop(2).unwrap());
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap(),
            json!({
                "status": "dormant",
                "bootstrapVersion": 1,
                "generation": "2",
            })
        );
    }

    #[test]
    fn stale_lifecycle_update_cannot_revoke_or_replace_newer_ready() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let session = owner.open_supervisor_session();
        session.begin_start(1).unwrap();
        session.begin_retry(2).unwrap();
        let (current, _temporary) = ready_access(2);
        let current_credential = current.credential.clone();
        session.publish(current).unwrap();

        assert!(!session.fail_generation(1).unwrap());
        assert!(!session.finish_stop(1).unwrap());

        assert!(current_credential.is_available());
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap()["status"],
            json!("ready")
        );
    }

    #[test]
    fn cloned_owner_shares_publication_without_revoking_on_partial_drop() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let observer = owner.clone();
        let session = owner.open_supervisor_session();
        let (access, _temporary) = ready_access(1);
        let credential = access.credential.clone();
        session.begin_start(1).unwrap();
        session.publish(access).unwrap();

        drop(owner);

        assert!(credential.is_available());
        assert_eq!(
            serde_json::to_value(observer.read().unwrap()).unwrap()["generation"],
            json!("1")
        );
        session.close();
        assert!(!credential.is_available());
    }

    #[test]
    fn supervisor_sessions_are_exclusive_and_stale_updates_are_inert() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let active = owner.open_supervisor_session();
        let stale = owner.open_supervisor_session();
        let (access, _temporary) = ready_access(1);
        let credential = access.credential.clone();
        active.begin_start(1).unwrap();
        active.publish(access).unwrap();

        assert!(stale.begin_start(1).is_err());
        assert!(!stale.fail_generation(1).unwrap());
        stale.close();

        assert!(credential.is_available());
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap()["status"],
            json!("ready")
        );
    }

    #[test]
    fn closed_session_cannot_mutate_or_revoke_a_successor() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let retired = owner.open_supervisor_session();
        retired.begin_start(1).unwrap();
        retired.fail_generation(1).unwrap();
        retired.close();
        let successor = owner.open_supervisor_session();
        let (access, _temporary) = ready_access(2);
        let credential = access.credential.clone();
        successor.begin_start(2).unwrap();
        successor.publish(access).unwrap();
        let (delayed, _delayed_temporary) = ready_access(2);
        let delayed_credential = delayed.credential.clone();

        assert!(retired.begin_start(3).is_err());
        assert!(retired.publish(delayed).is_err());
        assert!(!retired.fail_generation(2).unwrap());
        assert!(!retired.finish_stop(2).unwrap());
        assert!(!retired.clear_generation(2).unwrap());

        assert!(!delayed_credential.is_available());
        assert!(credential.is_available());
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap()["generation"],
            json!("2")
        );
    }

    #[test]
    fn closed_unclaimed_session_cannot_later_claim_a_transition() {
        let owner = super::NativeKernelBootstrapOwner::new();
        let retired = owner.open_supervisor_session();
        let successor = owner.open_supervisor_session();
        retired.close();

        successor.begin_start(1).unwrap();
        assert!(retired.begin_start(2).is_err());

        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap(),
            json!({
                "status": "starting",
                "bootstrapVersion": 1,
                "generation": "1",
            })
        );
    }

    fn ready_owner(
        generation: u64,
    ) -> (
        super::NativeKernelBootstrapOwner,
        super::NativeKernelBootstrapSession,
        tempfile::TempDir,
        String,
    ) {
        let owner = super::NativeKernelBootstrapOwner::new();
        let session = owner.open_supervisor_session();
        let (access, temporary) = ready_access(generation);
        let credential = access.credential.with_secret(str::to_owned).unwrap();
        session.begin_start(generation).unwrap();
        session.publish(access).unwrap();
        (owner, session, temporary, credential)
    }

    fn ready_access(generation: u64) -> (NativeKernelAccess, tempfile::TempDir) {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        let workspace_state = qingyu_kernel::host::native::NativeHostWorkspaceState::for_workspace(
            &workspace, "Notes",
        )
        .unwrap();
        let launch = NativeKernelLaunch::desktop(
            workspace,
            app_data,
            cache,
            workspace_state,
            "http://127.0.0.1:1420".to_owned(),
        )
        .unwrap();
        let (_startup, credential) = launch.into_parts();
        credential
            .with_secret(|secret| assert_eq!(secret.len(), CREDENTIAL.len()))
            .unwrap();
        (
            NativeKernelAccess {
                endpoint: KernelEndpoint {
                    generation,
                    port: 49152,
                    instance_id: InstanceId::new(
                        Uuid::parse_str(INSTANCE_ID).expect("test instance ID should parse"),
                    ),
                },
                credential,
            },
            temporary,
        )
    }
}
