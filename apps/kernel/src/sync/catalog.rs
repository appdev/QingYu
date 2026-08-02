//! S3 repository catalog boundary shared by repository discovery and execution.

use std::time::Duration;

use qingyu_dejavu::{
    CloudError, RepositoryCatalogList, RepositoryMetadata, S3AddressingStyle, S3Connection,
    S3RepositoryCatalog, S3TlsVerification, S3TransportOptions,
};

use crate::{
    contract::{
        S3AddressingStyle as ConfigAddressingStyle, S3TlsVerification as ConfigTlsVerification,
    },
    sync::config::{SyncConfig, SyncExecutionTarget},
};

pub(crate) struct KernelS3RepositoryCatalog {
    inner: S3RepositoryCatalog,
}

impl KernelS3RepositoryCatalog {
    pub(crate) fn from_config(config: SyncConfig) -> Result<Self, CloudError> {
        let plan = config
            .into_configured_s3_target()
            .map_err(|_| CloudError::UnsafeKey)?;
        let SyncExecutionTarget::S3 {
            endpoint_url,
            region,
            bucket,
            access_key_id,
            secret_access_key,
            request_timeout_seconds,
            addressing_style,
            tls_verification,
        } = plan
        else {
            return Err(CloudError::UnsafeKey);
        };
        Self::from_s3_parts(
            &endpoint_url,
            &region,
            &bucket,
            access_key_id.expose_secret(),
            secret_access_key.expose_secret(),
            request_timeout_seconds,
            addressing_style,
            tls_verification,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_s3_parts(
        endpoint_url: &str,
        region: &str,
        bucket: &str,
        access_key_id: &str,
        secret_access_key: &str,
        request_timeout_seconds: u32,
        addressing_style: ConfigAddressingStyle,
        tls_verification: ConfigTlsVerification,
    ) -> Result<Self, CloudError> {
        let connection = S3Connection::new(
            endpoint_url,
            region,
            bucket,
            access_key_id,
            secret_access_key,
            match addressing_style {
                ConfigAddressingStyle::Auto => S3AddressingStyle::Auto,
                ConfigAddressingStyle::Path => S3AddressingStyle::Path,
                ConfigAddressingStyle::VirtualHosted => S3AddressingStyle::VirtualHosted,
            },
        )?;
        let options = S3TransportOptions {
            request_timeout: Duration::from_secs(u64::from(request_timeout_seconds)),
            tls_verification: match tls_verification {
                ConfigTlsVerification::Verify => S3TlsVerification::Verify,
                ConfigTlsVerification::Skip => S3TlsVerification::Skip,
            },
            max_attempts: S3TransportOptions::default().max_attempts,
        };
        Ok(Self {
            inner: S3RepositoryCatalog::new(connection, options)?,
        })
    }

    pub(crate) async fn list(&self) -> Result<RepositoryCatalogList, CloudError> {
        self.inner.list().await
    }

    pub(crate) async fn read(&self, repository_id: &str) -> Result<RepositoryMetadata, CloudError> {
        self.inner.read(repository_id).await
    }

    pub(crate) async fn create(
        &self,
        repository_id: &str,
        display_name: &str,
        timestamp: i64,
    ) -> Result<RepositoryMetadata, CloudError> {
        self.inner
            .create(repository_id, display_name, timestamp)
            .await
    }
}
