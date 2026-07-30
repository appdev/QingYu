//! Dormant desktop-to-Kernel bootstrap publication boundary.
//!
//! The owner can retain a supervisor-issued same-generation endpoint and
//! parent credential lease, but production only installs an empty owner. Ready
//! publication remains unreachable until every legacy workspace writer is
//! disabled in the same atomic cutover.

use std::{fmt, sync::Mutex};

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
    #[cfg_attr(not(test), allow(dead_code))]
    Ready(NativeKernelReadyBootstrap),
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeKernelDormantBootstrap {
    status: NativeKernelBootstrapStatus,
    bootstrap_version: u16,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum NativeKernelBootstrapStatus {
    Dormant,
    #[cfg_attr(not(test), allow(dead_code))]
    Ready,
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
}

pub(crate) struct NativeKernelBootstrapOwner {
    state: Mutex<NativeKernelBootstrapState>,
}

struct NativeKernelBootstrapState {
    access: Option<NativeKernelAccess>,
    last_generation: u64,
}

impl NativeKernelBootstrapOwner {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(NativeKernelBootstrapState {
                access: None,
                last_generation: 0,
            }),
        }
    }

    pub(crate) fn read(&self) -> Result<NativeKernelBootstrap, String> {
        let access = self
            .state
            .lock()
            .map_err(|_| bootstrap_unavailable())?
            .access
            .clone();
        Ok(access.map_or_else(NativeKernelBootstrap::dormant, NativeKernelBootstrap::ready))
    }

    #[allow(dead_code)] // Published only by the future atomic runtime-owner cutover.
    pub(crate) fn publish(&self, access: NativeKernelAccess) -> Result<(), String> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                access.credential.revoke();
                if let Some(previous) = poisoned.into_inner().access.take() {
                    previous.credential.revoke();
                }
                return Err(bootstrap_unavailable());
            }
        };
        if state.last_generation >= access.endpoint.generation {
            access.credential.revoke();
            return Err(bootstrap_unavailable());
        }
        state.last_generation = access.endpoint.generation;
        if let Some(previous) = state.access.replace(access) {
            previous.credential.revoke();
        }
        Ok(())
    }

    #[allow(dead_code)] // Cleared by the future supervisor shutdown boundary.
    pub(crate) fn clear(&self) -> Result<(), String> {
        let previous = match self.state.lock() {
            Ok(mut state) => state.access.take(),
            Err(poisoned) => {
                let previous = poisoned.into_inner().access.take();
                if let Some(previous) = previous {
                    previous.credential.revoke();
                }
                return Err(bootstrap_unavailable());
            }
        };
        if let Some(previous) = previous {
            previous.credential.revoke();
        }
        Ok(())
    }
}

impl Drop for NativeKernelBootstrapOwner {
    fn drop(&mut self) {
        let state = match self.state.get_mut() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(access) = state.access.take() {
            access.credential.revoke();
        }
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
        let (owner, _temporary, credential) = ready_owner(1);
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
        let (owner, _temporary, _credential) = ready_owner(u64::MAX);
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
        let (access, _temporary) = ready_access(1);
        let observed = access.credential.clone();
        owner.publish(access).unwrap();

        owner.clear().unwrap();

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
        let (first, _first_temporary) = ready_access(1);
        let first_credential = first.credential.clone();
        let (second, _second_temporary) = ready_access(2);
        let second_credential = second.credential.clone();
        let second_secret = second_credential.with_secret(str::to_owned).unwrap();
        owner.publish(first).unwrap();

        owner.publish(second).unwrap();

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
        let (current, _current_temporary) = ready_access(2);
        let current_credential = current.credential.clone();
        let current_secret = current_credential.with_secret(str::to_owned).unwrap();
        owner.publish(current).unwrap();
        for generation in [1, 2] {
            let (candidate, _candidate_temporary) = ready_access(generation);
            let candidate_credential = candidate.credential.clone();

            assert!(owner.publish(candidate).is_err());
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
        let (current, _current_temporary) = ready_access(2);
        owner.publish(current).unwrap();
        owner.clear().unwrap();

        for generation in [1, 2] {
            let (candidate, _candidate_temporary) = ready_access(generation);
            let candidate_credential = candidate.credential.clone();
            assert!(owner.publish(candidate).is_err());
            assert!(candidate_credential.with_secret(str::to_owned).is_err());
        }
        let (next, _next_temporary) = ready_access(3);
        owner.publish(next).unwrap();
        assert_eq!(
            serde_json::to_value(owner.read().unwrap()).unwrap()["generation"],
            json!("3")
        );
    }

    fn ready_owner(
        generation: u64,
    ) -> (super::NativeKernelBootstrapOwner, tempfile::TempDir, String) {
        let owner = super::NativeKernelBootstrapOwner::new();
        let (access, temporary) = ready_access(generation);
        let credential = access.credential.with_secret(str::to_owned).unwrap();
        owner.publish(access).unwrap();
        (owner, temporary, credential)
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
