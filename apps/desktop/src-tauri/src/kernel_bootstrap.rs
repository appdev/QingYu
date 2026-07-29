//! Dormant desktop-to-Kernel bootstrap wire contract.
//!
//! The Ready representation freezes the Task 6 response shape for tests only.
//! It must remain unreachable in production until the supervisor owns a
//! same-generation endpoint and parent credential lease and every legacy
//! workspace writer has been disabled in the same atomic cutover.

use std::fmt;

use qingyu_kernel::{config::NativeLaunchCredential, contract::InstanceId};
use serde::Serializer;

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
struct NativeKernelBootstrapCredential(NativeLaunchCredential);

impl serde::Serialize for NativeKernelBootstrapCredential {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.expose_secret())
    }
}

impl fmt::Debug for NativeKernelBootstrapCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[cfg(test)]
impl NativeKernelBootstrap {
    fn ready_for_test(
        generation: u64,
        port: u16,
        instance_id: InstanceId,
        credential: NativeLaunchCredential,
    ) -> Self {
        Self(NativeKernelBootstrapRepresentation::Ready(
            NativeKernelReadyBootstrap {
                status: NativeKernelBootstrapStatus::Ready,
                bootstrap_version: NATIVE_KERNEL_BOOTSTRAP_VERSION,
                generation: NativeKernelBootstrapGeneration(generation),
                port,
                instance_id,
                credential: NativeKernelBootstrapCredential(credential),
            },
        ))
    }
}

#[tauri::command]
pub(crate) const fn read_native_kernel_bootstrap() -> NativeKernelBootstrap {
    NativeKernelBootstrap(NativeKernelBootstrapRepresentation::Dormant(
        NativeKernelDormantBootstrap {
            status: NativeKernelBootstrapStatus::Dormant,
            bootstrap_version: NATIVE_KERNEL_BOOTSTRAP_VERSION,
        },
    ))
}

#[cfg(test)]
mod tests {
    use qingyu_kernel::{config::NativeLaunchCredential, contract::InstanceId};
    use serde_json::json;
    use uuid::Uuid;

    const CREDENTIAL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const INSTANCE_ID: &str = "123e4567-e89b-42d3-a456-426614174000";

    #[test]
    fn production_bootstrap_is_exactly_version_one_dormant() {
        let value = serde_json::to_value(super::read_native_kernel_bootstrap())
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
        let value = serde_json::to_value(ready_bootstrap(1))
            .expect("future ready bootstrap should serialize");

        assert_eq!(
            value,
            json!({
                "status": "ready",
                "bootstrapVersion": 1,
                "generation": "1",
                "port": 49152,
                "instanceId": INSTANCE_ID,
                "credential": CREDENTIAL,
            })
        );
    }

    #[test]
    fn future_ready_generation_is_a_lossless_decimal_string() {
        let value = serde_json::to_value(ready_bootstrap(u64::MAX))
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
        let credential = super::NativeKernelBootstrapCredential(
            NativeLaunchCredential::from_secret(CREDENTIAL.to_owned())
                .expect("test credential should be canonical base64url"),
        );

        assert_eq!(format!("{credential:?}"), "[REDACTED]");
    }

    fn ready_bootstrap(generation: u64) -> super::NativeKernelBootstrap {
        super::NativeKernelBootstrap::ready_for_test(
            generation,
            49152,
            InstanceId::new(Uuid::parse_str(INSTANCE_ID).expect("test instance ID should parse")),
            NativeLaunchCredential::from_secret(CREDENTIAL.to_owned())
                .expect("test credential should be canonical base64url"),
        )
    }
}
