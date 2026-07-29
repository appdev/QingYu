//! Compatibility adapter for credentials still stored inline in v3 JSON.

use std::{fmt, sync::Mutex};

use zeroize::Zeroize as _;

use crate::{
    contract::CredentialChange,
    ports::{CredentialSecret, CredentialSlot, CredentialStore, PortError},
};

pub struct LegacyInlineCredentialStore {
    values: Mutex<LegacyInlineCredentials>,
}

impl LegacyInlineCredentialStore {
    pub fn new(
        webdav_password: String,
        s3_access_key_id: String,
        s3_secret_access_key: String,
    ) -> Self {
        Self {
            values: Mutex::new(LegacyInlineCredentials {
                webdav_password,
                s3_access_key_id,
                s3_secret_access_key,
            }),
        }
    }

    pub fn snapshot(&self) -> Result<LegacyInlineCredentials, PortError> {
        self.values
            .lock()
            .map(|values| values.clone())
            .map_err(|_| PortError::unavailable())
    }
}

impl CredentialStore for LegacyInlineCredentialStore {
    fn is_present(&self, slot: CredentialSlot) -> Result<bool, PortError> {
        let values = self.values.lock().map_err(|_| PortError::unavailable())?;
        Ok(!values.value(slot).is_empty())
    }

    fn replace(&self, slot: CredentialSlot, value: &CredentialSecret) -> Result<(), PortError> {
        let mut values = self.values.lock().map_err(|_| PortError::unavailable())?;
        let target = values.value_mut(slot);
        target.zeroize();
        target.push_str(value.expose_secret());
        Ok(())
    }

    fn clear(&self, slot: CredentialSlot) -> Result<(), PortError> {
        let mut values = self.values.lock().map_err(|_| PortError::unavailable())?;
        values.value_mut(slot).zeroize();
        Ok(())
    }
}

impl fmt::Debug for LegacyInlineCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LegacyInlineCredentialStore([REDACTED])")
    }
}

#[derive(Clone)]
pub struct LegacyInlineCredentials {
    webdav_password: String,
    s3_access_key_id: String,
    s3_secret_access_key: String,
}

impl LegacyInlineCredentials {
    pub fn into_parts(mut self) -> (String, String, String) {
        (
            std::mem::take(&mut self.webdav_password),
            std::mem::take(&mut self.s3_access_key_id),
            std::mem::take(&mut self.s3_secret_access_key),
        )
    }

    fn value(&self, slot: CredentialSlot) -> &str {
        match slot {
            CredentialSlot::WebDavPassword => &self.webdav_password,
            CredentialSlot::S3AccessKeyId => &self.s3_access_key_id,
            CredentialSlot::S3SecretAccessKey => &self.s3_secret_access_key,
        }
    }

    fn value_mut(&mut self, slot: CredentialSlot) -> &mut String {
        match slot {
            CredentialSlot::WebDavPassword => &mut self.webdav_password,
            CredentialSlot::S3AccessKeyId => &mut self.s3_access_key_id,
            CredentialSlot::S3SecretAccessKey => &mut self.s3_secret_access_key,
        }
    }
}

impl fmt::Debug for LegacyInlineCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LegacyInlineCredentials([REDACTED])")
    }
}

impl Drop for LegacyInlineCredentials {
    fn drop(&mut self) {
        self.webdav_password.zeroize();
        self.s3_access_key_id.zeroize();
        self.s3_secret_access_key.zeroize();
    }
}

pub fn apply_credential_change(
    store: &dyn CredentialStore,
    slot: CredentialSlot,
    change: Option<&CredentialChange>,
) -> Result<(), PortError> {
    match change {
        None | Some(CredentialChange::Keep {}) => Ok(()),
        Some(CredentialChange::Replace { value }) => {
            let secret = CredentialSecret::new(value.clone());
            store.replace(slot, &secret)
        }
        Some(CredentialChange::Clear {}) => store.clear(slot),
    }
}
