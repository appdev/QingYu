use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroize as _;

pub(crate) struct SecretDigest([u8; 32]);

impl SecretDigest {
    pub(crate) fn from_candidate(candidate: &str) -> Self {
        Self(Sha256::digest(candidate.as_bytes()).into())
    }

    pub(crate) fn matches(&self, candidate: &str) -> bool {
        let mut candidate_digest: [u8; 32] = Sha256::digest(candidate.as_bytes()).into();
        let matches = bool::from(self.0.ct_eq(&candidate_digest));
        candidate_digest.zeroize();
        matches
    }
}

impl Drop for SecretDigest {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretDigest([REDACTED])")
    }
}

pub(crate) struct ExposedSecret(String);

impl ExposedSecret {
    pub(crate) fn generate() -> Result<Self, RandomSecretError> {
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(|_| RandomSecretError)?;
        let encoded = URL_SAFE_NO_PAD.encode(random);
        random.zeroize();
        Ok(Self(encoded))
    }

    pub(crate) fn expose(&self) -> &str {
        self.0.as_str()
    }

    pub(crate) fn digest(&self) -> SecretDigest {
        SecretDigest::from_candidate(self.expose())
    }
}

impl Drop for ExposedSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for ExposedSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExposedSecret([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RandomSecretError;
