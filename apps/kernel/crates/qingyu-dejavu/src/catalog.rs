use std::collections::HashSet;

use crate::{Cloud, CloudError, S3Cloud, S3Connection, S3TransportOptions};

const METADATA_FORMAT_VERSION: u32 = 1;
const MAX_METADATA_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryMetadata {
    pub format_version: u32,
    pub repository_id: String,
    pub display_name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryCatalogEntry {
    pub repository_id: String,
    pub display_name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogIssueKind {
    InvalidRepositoryPrefix,
    MissingMetadata,
    MetadataTooLarge,
    MalformedMetadata,
    InvalidMetadata,
    RepositoryIdMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogIssue {
    pub repository_id: Option<String>,
    pub kind: CatalogIssueKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepositoryCatalogList {
    pub entries: Vec<RepositoryCatalogEntry>,
    pub issues: Vec<CatalogIssue>,
}

pub struct S3RepositoryCatalog {
    cloud: S3Cloud,
}

impl S3RepositoryCatalog {
    pub fn new(connection: S3Connection, options: S3TransportOptions) -> Result<Self, CloudError> {
        Ok(Self {
            cloud: S3Cloud::new_catalog_transport(connection, options)?,
        })
    }

    pub async fn create(
        &self,
        repository_id: &str,
        display_name: &str,
        timestamp: i64,
    ) -> Result<RepositoryMetadata, CloudError> {
        validate_repository_id(repository_id)?;
        let display_name = normalize_display_name(display_name)?.to_string();
        let metadata = RepositoryMetadata {
            format_version: METADATA_FORMAT_VERSION,
            repository_id: repository_id.to_string(),
            display_name,
            created_at: timestamp,
            updated_at: timestamp,
        };
        let bytes = serialize_metadata(&metadata)?;
        let key = metadata_key(repository_id);
        self.cloud.put(&key, &bytes, false).await?;
        Ok(metadata)
    }

    pub async fn list(&self) -> Result<RepositoryCatalogList, CloudError> {
        let directory_listing = self.cloud.list_catalog_directories().await?;
        let mut repository_ids = Vec::new();
        let mut seen = HashSet::new();
        let mut invalid_prefix_count = directory_listing.invalid_direct_object_count;
        for prefix in directory_listing.prefixes {
            let Some(repository_id) = repository_id_from_prefix(&prefix) else {
                invalid_prefix_count = invalid_prefix_count
                    .checked_add(1)
                    .ok_or_else(|| CloudError::backend("catalog_issue_count_overflow"))?;
                continue;
            };
            if !seen.insert(repository_id.to_string()) {
                return Err(CloudError::backend("catalog_duplicate_repository_id"));
            }
            repository_ids.push(repository_id.to_string());
        }
        repository_ids.sort();

        let mut entries: Vec<RepositoryCatalogEntry> = Vec::new();
        let mut issues = Vec::new();
        for repository_id in repository_ids {
            let key = metadata_key(&repository_id);
            let bytes = match self.cloud.get_bounded(&key, MAX_METADATA_BYTES).await {
                Ok(bytes) => bytes,
                Err(CloudError::NotFound) => {
                    issues.push(CatalogIssue {
                        repository_id: Some(repository_id),
                        kind: CatalogIssueKind::MissingMetadata,
                    });
                    continue;
                }
                Err(CloudError::ResponseTooLarge { .. }) => {
                    issues.push(CatalogIssue {
                        repository_id: Some(repository_id),
                        kind: CatalogIssueKind::MetadataTooLarge,
                    });
                    continue;
                }
                Err(error) => return Err(error),
            };
            match decode_metadata(&repository_id, &bytes) {
                Ok(metadata) => entries.push(metadata.into()),
                Err(problem) => issues.push(CatalogIssue {
                    repository_id: Some(repository_id),
                    kind: problem.issue_kind(),
                }),
            }
        }
        for _ in 0..invalid_prefix_count {
            issues.push(CatalogIssue {
                repository_id: None,
                kind: CatalogIssueKind::InvalidRepositoryPrefix,
            });
        }
        entries.sort_by(|left, right| {
            left.display_name
                .cmp(&right.display_name)
                .then_with(|| left.repository_id.cmp(&right.repository_id))
        });
        Ok(RepositoryCatalogList { entries, issues })
    }

    pub async fn read(&self, repository_id: &str) -> Result<RepositoryMetadata, CloudError> {
        validate_repository_id(repository_id)?;
        let bytes = self
            .cloud
            .get_bounded(&metadata_key(repository_id), MAX_METADATA_BYTES)
            .await?;
        decode_metadata(repository_id, &bytes).map_err(MetadataProblem::cloud_error)
    }

    pub async fn rename(
        &self,
        repository_id: &str,
        display_name: &str,
        updated_at: i64,
    ) -> Result<RepositoryMetadata, CloudError> {
        validate_repository_id(repository_id)?;
        let display_name = normalize_display_name(display_name)?.to_string();
        let mut metadata = self.read(repository_id).await?;
        metadata.display_name = display_name;
        metadata.updated_at = updated_at;
        let bytes = serialize_metadata(&metadata)?;
        self.cloud
            .put(&metadata_key(repository_id), &bytes, true)
            .await?;
        Ok(metadata)
    }

    pub async fn delete_repository(&self, repository_id: &str) -> Result<(), CloudError> {
        validate_repository_id(repository_id)?;
        let repository_prefix = format!("{repository_id}/");
        let metadata_key = metadata_key(repository_id);
        let objects = self
            .cloud
            .list_catalog_repository_objects(repository_id)
            .await?;
        let mut seen = HashSet::new();
        let mut ordinary_keys = Vec::new();
        let mut has_metadata = false;
        for object in objects {
            if !object.key.starts_with(&repository_prefix) || !seen.insert(object.key.clone()) {
                return Err(CloudError::UnsafeKey);
            }
            if object.key == metadata_key {
                has_metadata = true;
            } else {
                ordinary_keys.push(object.key);
            }
        }
        ordinary_keys.sort();
        for key in ordinary_keys {
            self.cloud
                .remove_catalog_repository_object(repository_id, &key)
                .await?;
        }
        if has_metadata {
            self.cloud
                .remove_catalog_repository_object(repository_id, &metadata_key)
                .await?;
        }
        Ok(())
    }
}

impl From<RepositoryMetadata> for RepositoryCatalogEntry {
    fn from(metadata: RepositoryMetadata) -> Self {
        Self {
            repository_id: metadata.repository_id,
            display_name: metadata.display_name,
            created_at: metadata.created_at,
            updated_at: metadata.updated_at,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MetadataProblem {
    Malformed,
    Invalid,
    RepositoryIdMismatch,
}

impl MetadataProblem {
    fn issue_kind(self) -> CatalogIssueKind {
        match self {
            Self::Malformed => CatalogIssueKind::MalformedMetadata,
            Self::Invalid => CatalogIssueKind::InvalidMetadata,
            Self::RepositoryIdMismatch => CatalogIssueKind::RepositoryIdMismatch,
        }
    }

    fn cloud_error(self) -> CloudError {
        match self {
            Self::Malformed => CloudError::backend("catalog_malformed_metadata"),
            Self::Invalid => CloudError::backend("catalog_invalid_metadata"),
            Self::RepositoryIdMismatch => CloudError::backend("catalog_repository_id_mismatch"),
        }
    }
}

fn decode_metadata(
    path_repository_id: &str,
    bytes: &[u8],
) -> Result<RepositoryMetadata, MetadataProblem> {
    let metadata: RepositoryMetadata =
        serde_json::from_slice(bytes).map_err(|_| MetadataProblem::Malformed)?;
    if metadata.format_version != METADATA_FORMAT_VERSION
        || validate_repository_id(&metadata.repository_id).is_err()
        || validate_stored_display_name(&metadata.display_name).is_err()
    {
        return Err(MetadataProblem::Invalid);
    }
    if metadata.repository_id != path_repository_id {
        return Err(MetadataProblem::RepositoryIdMismatch);
    }
    Ok(metadata)
}

fn repository_id_from_prefix(prefix: &str) -> Option<&str> {
    let repository_id = prefix.strip_suffix('/')?;
    if repository_id.contains('/') || validate_repository_id(repository_id).is_err() {
        return None;
    }
    Some(repository_id)
}

fn metadata_key(repository_id: &str) -> String {
    format!("{repository_id}/metadata.json")
}

fn serialize_metadata(metadata: &RepositoryMetadata) -> Result<Vec<u8>, CloudError> {
    let bytes = serde_json::to_vec(metadata)
        .map_err(|_| CloudError::backend("catalog_metadata_serialize_failed"))?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err(CloudError::backend("catalog_metadata_too_large"));
    }
    Ok(bytes)
}

fn validate_repository_id(repository_id: &str) -> Result<(), CloudError> {
    let parsed = uuid::Uuid::parse_str(repository_id)
        .map_err(|_| CloudError::backend("catalog_invalid_repository_id"))?;
    if parsed.to_string() != repository_id {
        return Err(CloudError::backend("catalog_invalid_repository_id"));
    }
    Ok(())
}

fn normalize_display_name(display_name: &str) -> Result<&str, CloudError> {
    let normalized = display_name.trim();
    if normalized.is_empty() || normalized.len() > 255 || normalized.chars().any(char::is_control) {
        return Err(CloudError::backend("catalog_invalid_display_name"));
    }
    Ok(normalized)
}

fn validate_stored_display_name(display_name: &str) -> Result<(), CloudError> {
    let normalized = normalize_display_name(display_name)?;
    if normalized != display_name {
        return Err(CloudError::backend("catalog_invalid_display_name"));
    }
    Ok(())
}
