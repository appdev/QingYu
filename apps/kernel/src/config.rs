use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::contract::{InstanceId, WireIdentityKey};

pub struct KernelConfig {
    instance_id: InstanceId,
    launch_epoch: KernelLaunchEpoch,
    native_launch_credential: NativeLaunchCredential,
    wire_identity_key: WireIdentityKey,
}

impl KernelConfig {
    pub fn generate() -> Result<Self, KernelConfigGenerationError> {
        Self::generate_with_native_launch_credential(NativeLaunchCredential::generate()?)
    }

    pub fn generate_with_native_launch_credential(
        native_launch_credential: NativeLaunchCredential,
    ) -> Result<Self, KernelConfigGenerationError> {
        let wire_identity_key =
            WireIdentityKey::generate().map_err(|_| KernelConfigGenerationError)?;
        Ok(Self {
            instance_id: InstanceId::new(Uuid::new_v4()),
            launch_epoch: KernelLaunchEpoch(Uuid::new_v4()),
            native_launch_credential,
            wire_identity_key,
        })
    }

    pub const fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    pub const fn wire_identity_key(&self) -> &WireIdentityKey {
        &self.wire_identity_key
    }

    pub const fn launch_epoch(&self) -> &KernelLaunchEpoch {
        &self.launch_epoch
    }

    pub const fn native_launch_credential(&self) -> &NativeLaunchCredential {
        &self.native_launch_credential
    }
}

impl fmt::Debug for KernelConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KernelConfig")
            .field("instance_id", &self.instance_id)
            .field("launch_epoch", &"KernelLaunchEpoch(..)")
            .field("native_launch_credential", &"[REDACTED]")
            .field("wire_identity_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelConfigGenerationError;

impl fmt::Display for KernelConfigGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Kernel launch identity generation failed")
    }
}

impl std::error::Error for KernelConfigGenerationError {}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct KernelLaunchEpoch(Uuid);

impl KernelLaunchEpoch {
    pub(crate) const fn value(self) -> Uuid {
        self.0
    }
}

impl fmt::Debug for KernelLaunchEpoch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KernelLaunchEpoch(..)")
    }
}

/// A per-launch bearer secret used only by the native loopback transport.
///
/// This type deliberately has no serialization or cloning implementation. The
/// native host must opt in to exposing the encoded value when constructing the
/// inherited startup payload, and inbound comparisons stay inside this type.
pub struct NativeLaunchCredential(String);

impl NativeLaunchCredential {
    pub fn generate() -> Result<Self, KernelConfigGenerationError> {
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(|_| KernelConfigGenerationError)?;
        let encoded = URL_SAFE_NO_PAD.encode(random);
        random.zeroize();
        Ok(Self(encoded))
    }

    pub fn from_secret(mut secret: String) -> Result<Self, InvalidNativeLaunchCredential> {
        let mut decoded = match URL_SAFE_NO_PAD.decode(secret.as_bytes()) {
            Ok(decoded) => decoded,
            Err(_) => {
                secret.zeroize();
                return Err(InvalidNativeLaunchCredential);
            }
        };
        let is_valid = decoded.len() == 32 && URL_SAFE_NO_PAD.encode(&decoded) == secret;
        decoded.zeroize();
        if !is_valid {
            secret.zeroize();
            return Err(InvalidNativeLaunchCredential);
        }
        Ok(Self(secret))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }

    pub fn matches(&self, candidate: &str) -> bool {
        bool::from(self.0.as_bytes().ct_eq(candidate.as_bytes()))
    }
}

impl fmt::Debug for NativeLaunchCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeLaunchCredential([REDACTED])")
    }
}

impl Drop for NativeLaunchCredential {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidNativeLaunchCredential;

impl fmt::Display for InvalidNativeLaunchCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("native launch credential is not a canonical 32-byte secret")
    }
}

impl std::error::Error for InvalidNativeLaunchCredential {}
