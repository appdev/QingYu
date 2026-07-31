use std::{collections::HashSet, fmt, io};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};
use utoipa::ToSchema;
use uuid::Uuid;
use zeroize::Zeroize;

pub use crate::server::ServerAuthenticationSecret;

#[cfg(test)]
thread_local! {
    static SIGNED_PAYLOAD_SERIALIZATIONS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

macro_rules! uuid_identifier {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
            #[serde(transparent)]
            pub struct $name(Uuid);

            impl $name {
                pub const fn new(value: Uuid) -> Self {
                    Self(value)
                }

                pub const fn as_uuid(&self) -> &Uuid {
                    &self.0
                }
            }
        )+
    };
}

uuid_identifier!(
    RequestId,
    InstanceId,
    WorkspaceId,
    ConnectionId,
    RunId,
    SnapshotId,
);

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct Revision(String);

impl Revision {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidRevision> {
        let value = value.into();
        if value.is_empty() {
            return Err(InvalidRevision);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Revision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("revision must not be empty"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidRevision;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct WorkspaceGeneration(String);

impl WorkspaceGeneration {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidWorkspaceGeneration> {
        let value = value.into();
        if value.is_empty() {
            return Err(InvalidWorkspaceGeneration);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for WorkspaceGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("workspace generation must not be empty"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidWorkspaceGeneration;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(try_from = "u64", into = "u64")]
pub struct SafeUnsignedInteger(u64);

impl SafeUnsignedInteger {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Result<Self, UnsafeWireInteger> {
        if value > MAX_SAFE_INTEGER {
            return Err(UnsafeWireInteger);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for SafeUnsignedInteger {
    type Error = UnsafeWireInteger;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SafeUnsignedInteger> for u64 {
    fn from(value: SafeUnsignedInteger) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(try_from = "u64", into = "u64")]
pub struct PositiveSafeInteger(u64);

impl PositiveSafeInteger {
    pub const fn new(value: u64) -> Result<Self, UnsafeWireInteger> {
        if value == 0 || value > MAX_SAFE_INTEGER {
            return Err(UnsafeWireInteger);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for PositiveSafeInteger {
    type Error = UnsafeWireInteger;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PositiveSafeInteger> for u64 {
    fn from(value: PositiveSafeInteger) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(try_from = "i64", into = "i64")]
pub struct SafeInteger(i64);

impl SafeInteger {
    pub const fn new(value: i64) -> Result<Self, UnsafeWireInteger> {
        if value < -(MAX_SAFE_INTEGER as i64) || value > MAX_SAFE_INTEGER as i64 {
            return Err(UnsafeWireInteger);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for SafeInteger {
    type Error = UnsafeWireInteger;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SafeInteger> for i64 {
    fn from(value: SafeInteger) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsafeWireInteger;

impl fmt::Display for UnsafeWireInteger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("integer exceeds the JavaScript-safe range")
    }
}

impl std::error::Error for UnsafeWireInteger {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    Image,
    Attachment,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FileDocumentKind {
    File,
}

macro_rules! wire_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name {
            $($variant),+
        }
    };
}

wire_enum!(ApiVersion { V1 });
wire_enum!(HostProfile {
    Desktop,
    Server,
    Mobile,
});
wire_enum!(StartupState {
    Starting,
    NeedsOwner,
    NeedsWorkspaceInitialization,
    NeedsCloudBinding,
    Ready,
    RecoverableError,
    FatalError,
});
wire_enum!(WorkspaceReadiness {
    Ready,
    Initializing,
    Unavailable,
    Locked,
});
wire_enum!(DeletionPolicy {
    Recoverable,
    Permanent,
});
wire_enum!(SyncProvider { S3, Webdav });
wire_enum!(SyncMode {
    Automatic,
    StartupExit,
    FullyManual,
});
wire_enum!(SyncConfigReadiness {
    Disabled,
    Incomplete,
    Ready,
});
wire_enum!(S3AddressingStyle {
    Auto,
    Path,
    VirtualHosted,
});
wire_enum!(S3TlsVerification { Verify, Skip });
wire_enum!(SyncCompletionState {
    Idle,
    Attempting,
    Failed,
    Succeeded,
});
wire_enum!(SyncRunCompletionState {
    Attempting,
    Failed,
    Succeeded,
});
wire_enum!(SyncTrigger {
    AppLaunch,
    Interval,
    Manual,
    Save,
    SettingsExit,
});

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct Rfc3339Utc(String);

impl Rfc3339Utc {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, InvalidRfc3339Utc> {
        let parsed =
            OffsetDateTime::parse(value.as_ref(), &Rfc3339).map_err(|_| InvalidRfc3339Utc)?;
        if parsed.offset() != UtcOffset::UTC {
            return Err(InvalidRfc3339Utc);
        }
        let canonical = parsed.format(&Rfc3339).map_err(|_| InvalidRfc3339Utc)?;
        Ok(Self(canonical))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Rfc3339Utc {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("timestamp must be RFC 3339 UTC"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidRfc3339Utc;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(try_from = "u16", into = "u16")]
pub struct PageLimit(u16);

impl PageLimit {
    pub const fn new(value: u16) -> Result<Self, InvalidPageLimit> {
        if value == 0 || value > 200 {
            return Err(InvalidPageLimit);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for PageLimit {
    type Error = InvalidPageLimit;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PageLimit> for u16 {
    fn from(value: PageLimit) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPageLimit;

impl fmt::Display for InvalidPageLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("page limit must be from 1 through 200")
    }
}

impl std::error::Error for InvalidPageLimit {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct Nullable<T>(Option<T>);

impl<T> Nullable<T> {
    pub const fn null() -> Self {
        Self(None)
    }

    pub const fn value(value: T) -> Self {
        Self(Some(value))
    }

    pub const fn as_ref(&self) -> Option<&T> {
        self.0.as_ref()
    }

    pub fn into_option(self) -> Option<T> {
        self.0
    }
}

impl<'de, T> Deserialize<'de> for Nullable<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(Self)
    }
}

fn deserialize_optional_non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn deserialize_required_nullable<'de, D, T>(deserializer: D) -> Result<Nullable<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Nullable)
}

const MAX_DOCUMENT_NAME_BYTES: usize = 255;
const MAX_DOCUMENT_CONTENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SEARCH_QUERY_SCALARS: usize = 512;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct DocumentName(String);

impl DocumentName {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidDocumentName> {
        let value = value.into();
        if !document_name_is_valid(&value) {
            return Err(InvalidDocumentName);
        }
        Ok(Self(value))
    }

    pub fn parse_file(value: impl Into<String>) -> Result<FileDocumentName, InvalidDocumentName> {
        FileDocumentName::parse(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn validate_kind(&self, kind: DocumentKind) -> Result<(), InvalidDocumentName> {
        if kind == DocumentKind::File && !has_markdown_extension(self.as_str()) {
            return Err(InvalidDocumentName);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for DocumentName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("invalid portable document name"))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct FileDocumentName(DocumentName);

impl FileDocumentName {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidDocumentName> {
        let name = DocumentName::parse(value)?;
        name.validate_kind(DocumentKind::File)?;
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn as_document_name(&self) -> &DocumentName {
        &self.0
    }
}

impl<'de> Deserialize<'de> for FileDocumentName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("invalid portable Markdown file name"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidDocumentName;

impl fmt::Display for InvalidDocumentName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid portable document name")
    }
}

impl std::error::Error for InvalidDocumentName {}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct ResourceName(String);

impl ResourceName {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidResourceName> {
        let value = value.into();
        if !document_name_is_valid(&value) {
            return Err(InvalidResourceName);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ResourceName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("invalid portable resource name"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidResourceName;

impl fmt::Display for InvalidResourceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid portable resource name")
    }
}

impl std::error::Error for InvalidResourceName {}

fn document_name_is_valid(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_DOCUMENT_NAME_BYTES
        || matches!(value, "." | "..")
        || value.ends_with(['.', ' '])
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
                )
        })
    {
        return false;
    }

    let ascii_lower = value.to_ascii_lowercase();
    if ascii_lower == ".qingyu"
        || [
            ".qingyu-ui-update-",
            ".qingyu-mcp-update-",
            ".markra-sync-stage-",
        ]
        .iter()
        .any(|prefix| ascii_lower.starts_with(prefix))
    {
        return false;
    }

    let stem = value.split('.').next().unwrap_or_default();
    let stem = stem.to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return false;
    }
    if stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9')
    {
        return false;
    }

    true
}

fn has_markdown_extension(value: &str) -> bool {
    let ascii_lower = value.to_ascii_lowercase();
    ascii_lower.ends_with(".md") || ascii_lower.ends_with(".markdown")
}

#[derive(Clone, Eq, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct DocumentContents(String);

impl DocumentContents {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidDocumentContents> {
        let value = value.into();
        if value.len() > MAX_DOCUMENT_CONTENT_BYTES {
            return Err(InvalidDocumentContents);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    pub(crate) const fn exceeds_limit(value: &str) -> bool {
        value.len() > MAX_DOCUMENT_CONTENT_BYTES
    }

    pub(crate) const fn maximum_bytes() -> usize {
        MAX_DOCUMENT_CONTENT_BYTES
    }
}

impl<'de> Deserialize<'de> for DocumentContents {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("document contents exceed the v1 limit"))
    }
}

impl fmt::Debug for DocumentContents {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DocumentContents([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidDocumentContents;

impl fmt::Display for InvalidDocumentContents {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("document contents exceed the v1 limit")
    }
}

impl std::error::Error for InvalidDocumentContents {}

#[derive(Clone, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct SearchQuery(String);

impl SearchQuery {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, InvalidSearchQuery> {
        let value = value.as_ref().trim();
        if value.is_empty() || value.chars().count() > MAX_SEARCH_QUERY_SCALARS {
            return Err(InvalidSearchQuery);
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SearchQuery {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("invalid search query"))
    }
}

impl fmt::Debug for SearchQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SearchQuery([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSearchQuery;

impl fmt::Display for InvalidSearchQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("search query must contain from 1 through 512 Unicode scalars")
    }
}

impl std::error::Error for InvalidSearchQuery {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LiveHealthResponse {
    pub status: LiveStatus,
    pub api_version: ApiVersion,
}

wire_enum!(LiveStatus { Live });
wire_enum!(ReadyStatus { Ready });

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReadyHealthResponse {
    pub status: ReadyStatus,
    pub api_version: ApiVersion,
    pub instance_id: InstanceId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SystemVersionResponse {
    pub api_version: ApiVersion,
    pub kernel_version: String,
    pub instance_id: InstanceId,
}

wire_enum!(ServerInitializationState {
    Required,
    Initialized,
    Unavailable,
});

wire_enum!(ServerSessionState { Authenticated });

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServerAuthenticationStatusDto {
    pub initialization: ServerInitializationState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServerSessionDto {
    pub state: ServerSessionState,
}

#[derive(Deserialize, Eq, PartialEq, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InitializeServerOwnerRequest {
    #[schema(write_only)]
    initialization_token: String,
    #[schema(write_only)]
    password: String,
}

impl InitializeServerOwnerRequest {
    pub fn into_parts(mut self) -> (ServerAuthenticationSecret, ServerAuthenticationSecret) {
        (
            ServerAuthenticationSecret::new(std::mem::take(&mut self.initialization_token)),
            ServerAuthenticationSecret::new(std::mem::take(&mut self.password)),
        )
    }
}

impl fmt::Debug for InitializeServerOwnerRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InitializeServerOwnerRequest")
            .field("initialization_token", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

impl Drop for InitializeServerOwnerRequest {
    fn drop(&mut self) {
        self.initialization_token.zeroize();
        self.password.zeroize();
    }
}

#[derive(Deserialize, Eq, PartialEq, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateServerSessionRequest {
    #[schema(write_only)]
    password: String,
}

impl CreateServerSessionRequest {
    pub fn into_password(mut self) -> ServerAuthenticationSecret {
        ServerAuthenticationSecret::new(std::mem::take(&mut self.password))
    }
}

impl fmt::Debug for CreateServerSessionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateServerSessionRequest")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

impl Drop for CreateServerSessionRequest {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[derive(Deserialize, Eq, PartialEq, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ChangeServerOwnerPasswordRequest {
    #[schema(write_only)]
    current_password: String,
    #[schema(write_only)]
    new_password: String,
}

impl ChangeServerOwnerPasswordRequest {
    pub fn into_parts(mut self) -> (ServerAuthenticationSecret, ServerAuthenticationSecret) {
        (
            ServerAuthenticationSecret::new(std::mem::take(&mut self.current_password)),
            ServerAuthenticationSecret::new(std::mem::take(&mut self.new_password)),
        )
    }
}

impl fmt::Debug for ChangeServerOwnerPasswordRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChangeServerOwnerPasswordRequest")
            .field("current_password", &"[REDACTED]")
            .field("new_password", &"[REDACTED]")
            .finish()
    }
}

impl Drop for ChangeServerOwnerPasswordRequest {
    fn drop(&mut self) {
        self.current_password.zeroize();
        self.new_password.zeroize();
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeCapabilitiesDto {
    pub documents: bool,
    pub history: bool,
    pub resources: bool,
    pub search: bool,
    pub settings: bool,
    pub sync: bool,
    pub webdav: bool,
    pub s3: bool,
    pub portable_settings: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeStateDto {
    pub profile: HostProfile,
    pub startup_state: StartupState,
    pub capabilities: RuntimeCapabilitiesDto,
    pub instance_id: InstanceId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceDto {
    pub id: WorkspaceId,
    pub generation: WorkspaceGeneration,
    pub display_name: String,
    pub readiness: WorkspaceReadiness,
    pub revision: Revision,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PageQuery {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cursor: Option<PageCursor>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub limit: Option<PageLimit>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ListDocumentsQuery {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cursor: Option<PageCursor>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub limit: Option<PageLimit>,
    #[serde(default)]
    pub parent: WorkspaceRelativePath,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ListWorkspaceInventoryQuery {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cursor: Option<PageCursor>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub limit: Option<PageLimit>,
    #[serde(default)]
    pub parent: WorkspaceRelativePath,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateWorkspaceResourceQuery {
    pub workspace_generation: WorkspaceGeneration,
    pub folder: WorkspaceRelativePath,
    pub name: ResourceName,
    pub kind: ResourceKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocumentEntryDto {
    pub id: DocumentId,
    pub path: WorkspaceRelativePath,
    pub parent: WorkspaceRelativePath,
    pub name: DocumentName,
    pub kind: DocumentKind,
    pub size_bytes: SafeUnsignedInteger,
    pub modified_at: Rfc3339Utc,
    pub revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ResourceEntryDto {
    pub id: ResourceId,
    pub path: WorkspaceRelativePath,
    pub parent: WorkspaceRelativePath,
    pub name: ResourceName,
    pub kind: ResourceKind,
    pub size_bytes: SafeUnsignedInteger,
    pub modified_at: Rfc3339Utc,
    pub revision: Revision,
    pub media_type: String,
    pub previewable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "entryType"
)]
pub enum WorkspaceInventoryEntryDto {
    Document { document: DocumentEntryDto },
    Resource { resource: ResourceEntryDto },
}

impl WorkspaceInventoryEntryDto {
    pub const fn path(&self) -> &WorkspaceRelativePath {
        match self {
            Self::Document { document } => &document.path,
            Self::Resource { resource } => &resource.path,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceInventoryPageDto {
    pub items: Vec<WorkspaceInventoryEntryDto>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub next_cursor: Nullable<PageCursor>,
}

impl DocumentEntryDto {
    pub fn validate(&self) -> Result<(), InvalidDocumentName> {
        self.name.validate_kind(self.kind)
    }
}

impl<'de> Deserialize<'de> for DocumentEntryDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        struct WireDocumentEntry {
            id: DocumentId,
            path: WorkspaceRelativePath,
            parent: WorkspaceRelativePath,
            name: DocumentName,
            kind: DocumentKind,
            size_bytes: SafeUnsignedInteger,
            modified_at: Rfc3339Utc,
            revision: Revision,
        }

        let wire = WireDocumentEntry::deserialize(deserializer)?;
        let entry = Self {
            id: wire.id,
            path: wire.path,
            parent: wire.parent,
            name: wire.name,
            kind: wire.kind,
            size_bytes: wire.size_bytes,
            modified_at: wire.modified_at,
            revision: wire.revision,
        };
        entry
            .validate()
            .map_err(|_| D::Error::custom("document kind and name do not agree"))?;
        Ok(entry)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocumentContentDto {
    pub id: DocumentId,
    pub path: WorkspaceRelativePath,
    pub parent: WorkspaceRelativePath,
    pub name: FileDocumentName,
    pub kind: FileDocumentKind,
    pub size_bytes: SafeUnsignedInteger,
    pub modified_at: Rfc3339Utc,
    pub revision: Revision,
    pub contents: DocumentContents,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum CreatedDocumentDto {
    File {
        id: DocumentId,
        path: WorkspaceRelativePath,
        parent: WorkspaceRelativePath,
        name: FileDocumentName,
        size_bytes: SafeUnsignedInteger,
        modified_at: Rfc3339Utc,
        revision: Revision,
        contents: DocumentContents,
    },
    Directory {
        id: DocumentId,
        path: WorkspaceRelativePath,
        parent: WorkspaceRelativePath,
        name: DocumentName,
        size_bytes: SafeUnsignedInteger,
        modified_at: Rfc3339Utc,
        revision: Revision,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocumentPageDto {
    pub items: Vec<DocumentEntryDto>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub next_cursor: Nullable<PageCursor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum CreateDocumentRequest {
    File {
        workspace_generation: WorkspaceGeneration,
        parent: WorkspaceRelativePath,
        name: FileDocumentName,
        contents: DocumentContents,
    },
    Directory {
        workspace_generation: WorkspaceGeneration,
        parent: WorkspaceRelativePath,
        name: DocumentName,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UpdateDocumentRequest {
    pub workspace_generation: WorkspaceGeneration,
    pub expected_revision: Revision,
    pub contents: DocumentContents,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MoveDocumentRequest {
    pub workspace_generation: WorkspaceGeneration,
    pub expected_revision: Revision,
    pub target_parent: WorkspaceRelativePath,
    pub name: DocumentName,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeleteDocumentRequest {
    pub workspace_generation: WorkspaceGeneration,
    pub expected_revision: Revision,
    pub deletion_policy: DeletionPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HistoryEntryDto {
    pub snapshot_id: SnapshotId,
    pub document_id: DocumentId,
    pub created_at: Rfc3339Utc,
    pub size_bytes: SafeUnsignedInteger,
    pub revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocumentHistoryPageDto {
    pub items: Vec<HistoryEntryDto>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub next_cursor: Nullable<PageCursor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocumentHistorySnapshotDto {
    pub snapshot_id: SnapshotId,
    pub document_id: DocumentId,
    pub created_at: Rfc3339Utc,
    pub size_bytes: SafeUnsignedInteger,
    pub revision: Revision,
    pub contents: DocumentContents,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RestoreDocumentHistoryRequest {
    pub workspace_generation: WorkspaceGeneration,
    pub expected_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SearchWorkspaceQuery {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cursor: Option<PageCursor>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub limit: Option<PageLimit>,
    pub query: SearchQuery,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SearchMatchDto {
    pub document: DocumentEntryDto,
    pub line: PositiveSafeInteger,
    pub column: PositiveSafeInteger,
    pub preview: String,
}

impl fmt::Debug for SearchMatchDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchMatchDto")
            .field("document", &self.document)
            .field("line", &self.line)
            .field("column", &self.column)
            .field("preview", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SearchPageDto {
    pub items: Vec<SearchMatchDto>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub next_cursor: Nullable<PageCursor>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
pub enum SettingKey {
    #[serde(rename = "appearance.mode")]
    AppearanceMode,
    #[serde(rename = "appearance.lightTheme")]
    AppearanceLightTheme,
    #[serde(rename = "appearance.darkTheme")]
    AppearanceDarkTheme,
    #[serde(rename = "theme.customCss.light")]
    ThemeCustomCssLight,
    #[serde(rename = "theme.customCss.dark")]
    ThemeCustomCssDark,
    #[serde(rename = "language")]
    Language,
    #[serde(rename = "editor.bodyFontSize")]
    EditorBodyFontSize,
    #[serde(rename = "editor.contentWidth")]
    EditorContentWidth,
    #[serde(rename = "editor.contentWidthPx")]
    EditorContentWidthPx,
    #[serde(rename = "editor.fontFamily")]
    EditorFontFamily,
    #[serde(rename = "editor.lineHeight")]
    EditorLineHeight,
    #[serde(rename = "editor.paragraphSpacingPx")]
    EditorParagraphSpacingPx,
    #[serde(rename = "editor.showWordCount")]
    EditorShowWordCount,
    #[serde(rename = "editor.wrapCodeBlocks")]
    EditorWrapCodeBlocks,
    #[serde(rename = "editor.viewMode")]
    EditorViewMode,
    #[serde(rename = "files.ignoreRules")]
    FilesIgnoreRules,
    #[serde(rename = "export.fontFamily")]
    ExportFontFamily,
    #[serde(rename = "export.pdfAuthor")]
    ExportPdfAuthor,
    #[serde(rename = "export.pdfFooter")]
    ExportPdfFooter,
    #[serde(rename = "export.pdfHeader")]
    ExportPdfHeader,
    #[serde(rename = "export.pdfHeightMm")]
    ExportPdfHeightMm,
    #[serde(rename = "export.pdfWidthMm")]
    ExportPdfWidthMm,
    #[serde(rename = "export.pdfMarginMm")]
    ExportPdfMarginMm,
    #[serde(rename = "export.pdfMarginPreset")]
    ExportPdfMarginPreset,
    #[serde(rename = "export.pdfPageBreakOnH1")]
    ExportPdfPageBreakOnH1,
    #[serde(rename = "export.pdfPageSize")]
    ExportPdfPageSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct FiniteNumber(f64);

impl FiniteNumber {
    pub fn new(value: f64) -> Result<Self, NonFiniteNumber> {
        if !value.is_finite() {
            return Err(NonFiniteNumber);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for FiniteNumber {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).map_err(|_| D::Error::custom("number must be finite"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NonFiniteNumber;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "source"
)]
pub enum FontFamilyValueDto {
    Theme {
        #[serde(deserialize_with = "deserialize_required_nullable")]
        family: Nullable<String>,
    },
    System {
        family: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SettingValueDto {
    Boolean {
        value: bool,
    },
    Integer {
        value: SafeInteger,
    },
    Number {
        value: FiniteNumber,
    },
    String {
        value: String,
    },
    NullableInteger {
        #[serde(deserialize_with = "deserialize_required_nullable")]
        value: Nullable<SafeInteger>,
    },
    NullableString {
        #[serde(deserialize_with = "deserialize_required_nullable")]
        value: Nullable<String>,
    },
    FontFamily {
        value: FontFamilyValueDto,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SettingEntryDto {
    pub key: SettingKey,
    pub value: SettingValueDto,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SettingsSnapshotDto {
    pub revision: Revision,
    pub values: Vec<SettingEntryDto>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PatchSettingsRequest {
    pub expected_revision: Revision,
    pub values: Vec<SettingEntryDto>,
}

impl PatchSettingsRequest {
    pub fn validate(&self) -> Result<(), InvalidSettingsPatch> {
        if self.values.is_empty() {
            return Err(InvalidSettingsPatch);
        }
        let mut keys = HashSet::with_capacity(self.values.len());
        if self.values.iter().any(|entry| !keys.insert(entry.key)) {
            return Err(InvalidSettingsPatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSettingsPatch;

#[derive(Clone, Eq, PartialEq, Deserialize, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "operation"
)]
pub enum CredentialChange {
    Keep {},
    Replace { value: String },
    Clear {},
}

impl fmt::Debug for CredentialChange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keep {} => formatter.write_str("CredentialChange::Keep"),
            Self::Replace { .. } => {
                formatter.write_str("CredentialChange::Replace { value: [REDACTED] }")
            }
            Self::Clear {} => formatter.write_str("CredentialChange::Clear"),
        }
    }
}

impl Drop for CredentialChange {
    fn drop(&mut self) {
        if let Self::Replace { value } = self {
            value.zeroize();
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CredentialState {
    pub present: bool,
}

wire_enum!(SyncIssueCode {
    Required,
    InvalidUrl,
    UnsafeUrlComponents,
    OutOfRange,
    InvalidPath,
});

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncIssueDto {
    pub field: String,
    pub code: SyncIssueCode,
    pub message: String,
}

impl fmt::Debug for SyncIssueDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncIssueDto")
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SafeEndpointViewDto {
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub value: Nullable<String>,
    pub redacted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WebDavConfigViewDto {
    pub server_url: SafeEndpointViewDto,
    pub username: String,
    pub password: CredentialState,
}

macro_rules! bounded_unsigned {
    ($name:ident, $raw:ty, $minimum:expr, $maximum:expr) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ToSchema)]
        pub struct $name($raw);

        impl $name {
            pub const fn new(value: $raw) -> Result<Self, InvalidBoundedInteger> {
                if value < $minimum || value > $maximum {
                    return Err(InvalidBoundedInteger);
                }
                Ok(Self(value))
            }

            pub const fn get(self) -> $raw {
                self.0
            }
        }

        impl TryFrom<$raw> for $name {
            type Error = InvalidBoundedInteger;

            fn try_from(value: $raw) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for $raw {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                self.0.serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = <$raw>::deserialize(deserializer)?;
                Self::new(value).map_err(D::Error::custom)
            }
        }
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidBoundedInteger;

impl fmt::Display for InvalidBoundedInteger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("integer is outside the allowed range")
    }
}

impl std::error::Error for InvalidBoundedInteger {}

bounded_unsigned!(RequestTimeoutSeconds, u16, 5, 600);
bounded_unsigned!(SyncIntervalSeconds, u32, 30, 43_200);
bounded_unsigned!(HttpStatus, u16, 100, 599);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct S3ConfigViewDto {
    pub endpoint_url: SafeEndpointViewDto,
    pub region: String,
    pub bucket: String,
    pub access_key_id: CredentialState,
    pub secret_access_key: CredentialState,
    pub request_timeout_seconds: RequestTimeoutSeconds,
    pub addressing_style: S3AddressingStyle,
    pub tls_verification: S3TlsVerification,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncConfigViewDto {
    pub revision: Revision,
    pub enabled: bool,
    pub provider: SyncProvider,
    pub remote_root: String,
    pub mode: SyncMode,
    pub interval_seconds: SyncIntervalSeconds,
    pub generate_conflict_document: bool,
    pub configured: bool,
    pub readiness: SyncConfigReadiness,
    pub issues: Vec<SyncIssueDto>,
    pub webdav: WebDavConfigViewDto,
    pub s3: S3ConfigViewDto,
}

#[derive(Clone, Default, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncConfigChangesDto {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub enabled: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub provider: Option<SyncProvider>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub remote_root: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub mode: Option<SyncMode>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub interval_seconds: Option<SyncIntervalSeconds>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub generate_conflict_document: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub webdav_server_url: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub webdav_username: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub webdav_password: Option<CredentialChange>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3_endpoint_url: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3_region: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3_bucket: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3_access_key_id: Option<CredentialChange>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3_secret_access_key: Option<CredentialChange>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3_request_timeout_seconds: Option<RequestTimeoutSeconds>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3_addressing_style: Option<S3AddressingStyle>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3_tls_verification: Option<S3TlsVerification>,
}

impl SyncConfigChangesDto {
    pub fn validate(&self) -> Result<(), EmptySyncConfigChanges> {
        if self.enabled.is_none()
            && self.provider.is_none()
            && self.remote_root.is_none()
            && self.mode.is_none()
            && self.interval_seconds.is_none()
            && self.generate_conflict_document.is_none()
            && self.webdav_server_url.is_none()
            && self.webdav_username.is_none()
            && self.webdav_password.is_none()
            && self.s3_endpoint_url.is_none()
            && self.s3_region.is_none()
            && self.s3_bucket.is_none()
            && self.s3_access_key_id.is_none()
            && self.s3_secret_access_key.is_none()
            && self.s3_request_timeout_seconds.is_none()
            && self.s3_addressing_style.is_none()
            && self.s3_tls_verification.is_none()
        {
            return Err(EmptySyncConfigChanges);
        }
        Ok(())
    }
}

impl fmt::Debug for SyncConfigChangesDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncConfigChangesDto(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmptySyncConfigChanges;

#[derive(Clone, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PatchSyncConfigRequest {
    pub expected_revision: Revision,
    pub changes: SyncConfigChangesDto,
}

impl fmt::Debug for PatchSyncConfigRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PatchSyncConfigRequest(..)")
    }
}

#[derive(Clone, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestSyncConnectionRequest {
    pub expected_revision: Revision,
    pub changes: SyncConfigChangesDto,
}

impl fmt::Debug for TestSyncConnectionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TestSyncConnectionRequest(..)")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncConnectionTestDto {
    pub provider: SyncProvider,
    pub checked_target: String,
    pub config_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncSummaryDto {
    pub bytes_downloaded: SafeUnsignedInteger,
    pub bytes_uploaded: SafeUnsignedInteger,
    pub conflict_files: SafeUnsignedInteger,
    pub downloaded_files: SafeUnsignedInteger,
    pub scanned_files: SafeUnsignedInteger,
    pub skipped_files: SafeUnsignedInteger,
    pub uploaded_files: SafeUnsignedInteger,
}

impl SyncSummaryDto {
    pub const fn empty() -> Self {
        Self {
            bytes_downloaded: SafeUnsignedInteger::ZERO,
            bytes_uploaded: SafeUnsignedInteger::ZERO,
            conflict_files: SafeUnsignedInteger::ZERO,
            downloaded_files: SafeUnsignedInteger::ZERO,
            scanned_files: SafeUnsignedInteger::ZERO,
            skipped_files: SafeUnsignedInteger::ZERO,
            uploaded_files: SafeUnsignedInteger::ZERO,
        }
    }
}

macro_rules! sync_safe_value_enum {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }

            fn accepts(value: &str) -> bool {
                matches!(value, $($wire)|+)
            }
        }
    };
}

sync_safe_value_enum!(SyncSafeErrorCategory {
    Authentication => "authentication",
    Authorization => "authorization",
    Configuration => "configuration",
    Conflict => "conflict",
    Network => "network",
    Provider => "provider",
    Storage => "storage",
    Transport => "transport",
});

sync_safe_value_enum!(SyncSafeErrorCode {
    AuthenticationFailed => "authentication_failed",
    Cancelled => "cancelled",
    ConfigurationInvalid => "configuration_invalid",
    Conflict => "conflict",
    ConnectionFailed => "connection_failed",
    LocalIo => "local_io",
    PermissionDenied => "permission_denied",
    RateLimited => "rate_limited",
    RemoteUnavailable => "remote_unavailable",
    RequestFailed => "request_failed",
    Unknown => "unknown",
});

sync_safe_value_enum!(SyncSafeErrorOperation {
    ApplyConfig => "apply_config",
    DeleteObject => "delete_object",
    DownloadObject => "download_object",
    ListRemote => "list_remote",
    ReadLocal => "read_local",
    ReadManifest => "read_manifest",
    SyncRun => "sync_run",
    TestConnection => "test_connection",
    UploadObject => "upload_object",
    WriteLocal => "write_local",
    WriteManifest => "write_manifest",
});

sync_safe_value_enum!(SyncSafeHttpMethod {
    Delete => "DELETE",
    Get => "GET",
    Head => "HEAD",
    Post => "POST",
    Propfind => "PROPFIND",
    Put => "PUT",
});

sync_safe_value_enum!(SyncSafeProviderErrorCode {
    AccessDenied => "AccessDenied",
    Conflict => "Conflict",
    Forbidden => "Forbidden",
    InvalidRequest => "InvalidRequest",
    Locked => "Locked",
    NoSuchBucket => "NoSuchBucket",
    NoSuchKey => "NoSuchKey",
    NotFound => "NotFound",
    PreconditionFailed => "PreconditionFailed",
    RequestTimeout => "RequestTimeout",
    ServerError => "ServerError",
    SlowDown => "SlowDown",
    TooManyRequests => "TooManyRequests",
    Unauthorized => "Unauthorized",
    Unknown => "Unknown",
});

#[derive(Clone, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncSafeErrorDto {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    category: Option<String>,
    code: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    http_status: Option<HttpStatus>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    method: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    object_id: Option<String>,
    operation: String,
    provider: SyncProvider,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    provider_error_code: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    relative_path: Option<WorkspaceRelativePath>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    request_id: Option<RequestId>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    run_id: Option<RunId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SyncSafeErrorWire {
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    category: Option<String>,
    code: String,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    http_status: Option<HttpStatus>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    method: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    object_id: Option<String>,
    operation: String,
    provider: SyncProvider,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    provider_error_code: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    relative_path: Option<WorkspaceRelativePath>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    request_id: Option<RequestId>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    run_id: Option<RunId>,
}

impl SyncSafeErrorDto {
    pub fn new(
        provider: SyncProvider,
        operation: SyncSafeErrorOperation,
        code: SyncSafeErrorCode,
    ) -> Self {
        Self {
            category: None,
            code: code.as_str().to_owned(),
            http_status: None,
            method: None,
            object_id: None,
            operation: operation.as_str().to_owned(),
            provider,
            provider_error_code: None,
            relative_path: None,
            request_id: None,
            run_id: None,
        }
    }

    pub fn with_category(mut self, category: SyncSafeErrorCategory) -> Self {
        self.category = Some(category.as_str().to_owned());
        self
    }

    pub fn with_http_status(mut self, http_status: HttpStatus) -> Self {
        self.http_status = Some(http_status);
        self
    }

    pub fn with_method(mut self, method: SyncSafeHttpMethod) -> Self {
        self.method = Some(method.as_str().to_owned());
        self
    }

    pub fn with_provider_error_code(
        mut self,
        provider_error_code: SyncSafeProviderErrorCode,
    ) -> Self {
        self.provider_error_code = Some(provider_error_code.as_str().to_owned());
        self
    }

    pub fn with_relative_path(mut self, relative_path: WorkspaceRelativePath) -> Self {
        self.relative_path = Some(relative_path);
        self
    }

    pub fn with_request_id(mut self, request_id: RequestId) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub fn with_run_id(mut self, run_id: RunId) -> Self {
        self.run_id = Some(run_id);
        self
    }

    pub fn category(&self) -> Option<&str> {
        self.category.as_deref()
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub const fn http_status(&self) -> Option<HttpStatus> {
        self.http_status
    }

    pub fn method(&self) -> Option<&str> {
        self.method.as_deref()
    }

    pub fn object_id(&self) -> Option<&str> {
        self.object_id.as_deref()
    }

    pub fn operation(&self) -> &str {
        &self.operation
    }

    pub const fn provider(&self) -> SyncProvider {
        self.provider
    }

    pub fn provider_error_code(&self) -> Option<&str> {
        self.provider_error_code.as_deref()
    }

    pub fn relative_path(&self) -> Option<&WorkspaceRelativePath> {
        self.relative_path.as_ref()
    }

    pub const fn request_id(&self) -> Option<RequestId> {
        self.request_id
    }

    pub const fn run_id(&self) -> Option<RunId> {
        self.run_id
    }

    fn validate(&self) -> Result<(), InvalidSyncSafeErrorField> {
        validate_sync_safe_enum(
            self.category.as_deref(),
            SyncSafeErrorCategory::accepts,
            InvalidSyncSafeErrorField::Category,
        )?;
        validate_sync_safe_enum(
            Some(&self.code),
            SyncSafeErrorCode::accepts,
            InvalidSyncSafeErrorField::Code,
        )?;
        validate_sync_safe_enum(
            self.method.as_deref(),
            SyncSafeHttpMethod::accepts,
            InvalidSyncSafeErrorField::Method,
        )?;
        if self.object_id.is_some() {
            return Err(InvalidSyncSafeErrorField::ObjectId);
        }
        validate_sync_safe_enum(
            Some(&self.operation),
            SyncSafeErrorOperation::accepts,
            InvalidSyncSafeErrorField::Operation,
        )?;
        validate_sync_safe_enum(
            self.provider_error_code.as_deref(),
            SyncSafeProviderErrorCode::accepts,
            InvalidSyncSafeErrorField::ProviderErrorCode,
        )
    }
}

impl<'de> Deserialize<'de> for SyncSafeErrorDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SyncSafeErrorWire::deserialize(deserializer)?;
        Self::try_from(wire).map_err(D::Error::custom)
    }
}

impl TryFrom<SyncSafeErrorWire> for SyncSafeErrorDto {
    type Error = InvalidSyncSafeErrorField;

    fn try_from(wire: SyncSafeErrorWire) -> Result<Self, Self::Error> {
        let value = Self {
            category: wire.category,
            code: wire.code,
            http_status: wire.http_status,
            method: wire.method,
            object_id: wire.object_id,
            operation: wire.operation,
            provider: wire.provider,
            provider_error_code: wire.provider_error_code,
            relative_path: wire.relative_path,
            request_id: wire.request_id,
            run_id: wire.run_id,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Drop for SyncSafeErrorDto {
    fn drop(&mut self) {
        self.category.zeroize();
        self.code.zeroize();
        self.method.zeroize();
        self.object_id.zeroize();
        self.operation.zeroize();
        self.provider_error_code.zeroize();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidSyncSafeErrorField {
    Category,
    Code,
    Method,
    ObjectId,
    Operation,
    ProviderErrorCode,
}

impl fmt::Display for InvalidSyncSafeErrorField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync error contains an unsafe public field")
    }
}

impl std::error::Error for InvalidSyncSafeErrorField {}

fn validate_sync_safe_enum(
    value: Option<&str>,
    accepts: impl FnOnce(&str) -> bool,
    field: InvalidSyncSafeErrorField,
) -> Result<(), InvalidSyncSafeErrorField> {
    let Some(value) = value else {
        return Ok(());
    };
    if !accepts(value) {
        return Err(field);
    }
    Ok(())
}

impl fmt::Debug for SyncSafeErrorDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncSafeErrorDto")
            .field("provider", &self.provider)
            .field("http_status", &self.http_status)
            .field("request_id", &self.request_id)
            .field("run_id", &self.run_id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncStatusDto {
    pub completion_state: SyncCompletionState,
    pub provider: SyncProvider,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub config_revision: Nullable<Revision>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub active_run_id: Nullable<RunId>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub last_attempt_at: Nullable<Rfc3339Utc>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub last_successful_sync_at: Nullable<Rfc3339Utc>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub last_trigger: Nullable<SyncTrigger>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub summary: Nullable<SyncSummaryDto>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub error: Nullable<SyncSafeErrorDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TriggerSyncRunRequest {
    pub expected_config_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncRunAcceptedDto {
    pub run_id: RunId,
    pub accepted_at: Rfc3339Utc,
    pub config_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncRunStatusDto {
    pub run_id: RunId,
    pub provider: SyncProvider,
    pub config_revision: Revision,
    pub completion_state: SyncRunCompletionState,
    pub accepted_at: Rfc3339Utc,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub finished_at: Nullable<Rfc3339Utc>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub summary: Nullable<SyncSummaryDto>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub error: Nullable<SyncSafeErrorDto>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    InvalidWorkspacePath,
    InvalidDocumentName,
    Unauthorized,
    InitializationRequired,
    AlreadyInitialized,
    InvalidCredentials,
    CsrfRejected,
    AuthenticationRateLimited,
    AuthenticationUnavailable,
    HostNotAllowed,
    OriginNotAllowed,
    KernelNotReady,
    WorkspaceUnavailable,
    WorkspaceLocked,
    DocumentNotFound,
    ResourceNotFound,
    DocumentAlreadyExists,
    DocumentTooLarge,
    ResourceTooLarge,
    DocumentInvalidEncoding,
    RevisionConflict,
    SettingsRevisionConflict,
    SyncConfigRevisionConflict,
    InvalidSettingsField,
    SettingsUnavailable,
    SyncConfigAbsent,
    SyncConfigInvalid,
    SyncNotReady,
    SyncRunUnavailable,
    InternalError,
}

wire_enum!(ValidationIssueCode {
    Required,
    InvalidFormat,
    OutOfRange,
    Conflict,
    UnsafeValue,
});

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
pub enum ValidationField {
    #[serde(rename = "request")]
    Request,
    #[serde(rename = "workspaceGeneration")]
    WorkspaceGeneration,
    #[serde(rename = "parent")]
    Parent,
    #[serde(rename = "name")]
    Name,
    #[serde(rename = "kind")]
    Kind,
    #[serde(rename = "contents")]
    Contents,
    #[serde(rename = "expectedRevision")]
    ExpectedRevision,
    #[serde(rename = "targetParent")]
    TargetParent,
    #[serde(rename = "deletionPolicy")]
    DeletionPolicy,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "limit")]
    Limit,
    #[serde(rename = "query")]
    Query,
    #[serde(rename = "snapshotId")]
    SnapshotId,
    #[serde(rename = "values")]
    Values,
    #[serde(rename = "changes")]
    Changes,
    #[serde(rename = "provider")]
    Provider,
    #[serde(rename = "mode")]
    Mode,
    #[serde(rename = "remoteRoot")]
    RemoteRoot,
    #[serde(rename = "intervalSeconds")]
    IntervalSeconds,
    #[serde(rename = "webdav")]
    Webdav,
    #[serde(rename = "s3")]
    S3,
    #[serde(rename = "endpointUrl")]
    EndpointUrl,
    #[serde(rename = "username")]
    Username,
    #[serde(rename = "password")]
    Password,
    #[serde(rename = "accessKeyId")]
    AccessKeyId,
    #[serde(rename = "secretAccessKey")]
    SecretAccessKey,
    #[serde(rename = "bucket")]
    Bucket,
    #[serde(rename = "region")]
    Region,
    #[serde(rename = "addressingStyle")]
    AddressingStyle,
    #[serde(rename = "tlsVerification")]
    TlsVerification,
    #[serde(rename = "expectedConfigRevision")]
    ExpectedConfigRevision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
enum SafeValidationMessage {
    #[serde(rename = "This field is required.")]
    Required,
    #[serde(rename = "This field has an invalid format.")]
    InvalidFormat,
    #[serde(rename = "This field is outside the supported range.")]
    OutOfRange,
    #[serde(rename = "This field conflicts with another value.")]
    Conflict,
    #[serde(rename = "This field contains an unsafe value.")]
    UnsafeValue,
}

impl From<ValidationIssueCode> for SafeValidationMessage {
    fn from(code: ValidationIssueCode) -> Self {
        match code {
            ValidationIssueCode::Required => Self::Required,
            ValidationIssueCode::InvalidFormat => Self::InvalidFormat,
            ValidationIssueCode::OutOfRange => Self::OutOfRange,
            ValidationIssueCode::Conflict => Self::Conflict,
            ValidationIssueCode::UnsafeValue => Self::UnsafeValue,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ValidationIssueDto {
    pub field: ValidationField,
    pub code: ValidationIssueCode,
    message: SafeValidationMessage,
}

impl ValidationIssueDto {
    pub fn new(field: ValidationField, code: ValidationIssueCode) -> Self {
        Self {
            field,
            code,
            message: code.into(),
        }
    }

    pub const fn message(&self) -> &'static str {
        match self.message {
            SafeValidationMessage::Required => "This field is required.",
            SafeValidationMessage::InvalidFormat => "This field has an invalid format.",
            SafeValidationMessage::OutOfRange => "This field is outside the supported range.",
            SafeValidationMessage::Conflict => "This field conflicts with another value.",
            SafeValidationMessage::UnsafeValue => "This field contains an unsafe value.",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct ValidationIssues(Vec<ValidationIssueDto>);

impl ValidationIssues {
    pub fn new(
        first: ValidationIssueDto,
        remaining: impl IntoIterator<Item = ValidationIssueDto>,
    ) -> Self {
        let mut issues = vec![first];
        issues.extend(remaining);
        Self(issues)
    }

    pub fn try_from_vec(issues: Vec<ValidationIssueDto>) -> Result<Self, EmptyValidationIssues> {
        if issues.is_empty() {
            return Err(EmptyValidationIssues);
        }
        Ok(Self(issues))
    }

    pub fn as_slice(&self) -> &[ValidationIssueDto] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ValidationIssues {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let issues = Vec::<ValidationIssueDto>::deserialize(deserializer)?;
        Self::try_from_vec(issues)
            .map_err(|_| D::Error::custom("validation issues must not be empty"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmptyValidationIssues;

impl fmt::Display for EmptyValidationIssues {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("validation issues must not be empty")
    }
}

impl std::error::Error for EmptyValidationIssues {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ErrorDetails {
    RevisionConflict {
        #[serde(
            default,
            deserialize_with = "deserialize_optional_non_null",
            skip_serializing_if = "Option::is_none"
        )]
        current_revision: Option<Revision>,
    },
    Validation {
        issues: ValidationIssues,
    },
    Startup {
        state: StartupState,
    },
    RateLimit {
        retry_after_seconds: PositiveSafeInteger,
    },
}

impl ErrorDetails {
    pub fn current_revision(&self) -> Option<&Revision> {
        match self {
            Self::RevisionConflict { current_revision } => current_revision.as_ref(),
            Self::Validation { .. } | Self::Startup { .. } | Self::RateLimit { .. } => None,
        }
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ApiErrorEnvelope {
    pub(crate) code: ErrorCode,
    pub(crate) message: String,
    pub(crate) request_id: RequestId,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) details: Option<ErrorDetails>,
}

impl fmt::Debug for ApiErrorEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApiErrorEnvelope")
            .field("code", &self.code)
            .field("request_id", &self.request_id)
            .field("details", &self.details)
            .finish()
    }
}

impl ApiErrorEnvelope {
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub const fn details(&self) -> Option<&ErrorDetails> {
        self.details.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct ProtocolVersion(u8);

impl ProtocolVersion {
    pub const fn new(value: u8) -> Result<Self, InvalidProtocolScalar> {
        if value != 1 {
            return Err(InvalidProtocolScalar);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct ReadySequence(u8);

impl ReadySequence {
    pub const fn new(value: u8) -> Result<Self, InvalidProtocolScalar> {
        if value != 0 {
            return Err(InvalidProtocolScalar);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ReadySequence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SnapshotRequired;

impl utoipa::PartialSchema for SnapshotRequired {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        <bool as utoipa::PartialSchema>::schema()
    }
}

impl ToSchema for SnapshotRequired {}

impl SnapshotRequired {
    pub const fn required() -> Self {
        Self
    }

    pub const fn get(self) -> bool {
        true
    }
}

impl Serialize for SnapshotRequired {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for SnapshotRequired {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if bool::deserialize(deserializer)? {
            Ok(Self)
        } else {
            Err(D::Error::custom("snapshotRequired must be true"))
        }
    }
}

bounded_unsigned!(EventSequence, u64, 1, MAX_SAFE_INTEGER);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidProtocolScalar;

impl fmt::Display for InvalidProtocolScalar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid protocol scalar")
    }
}

impl std::error::Error for InvalidProtocolScalar {}

wire_enum!(AuthenticateFrameType { Authenticate });

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AuthenticateFrame {
    #[serde(rename = "type")]
    pub frame_type: AuthenticateFrameType,
    pub protocol_version: ProtocolVersion,
    pub credential: String,
}

impl fmt::Debug for AuthenticateFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticateFrame")
            .field("frame_type", &self.frame_type)
            .field("protocol_version", &self.protocol_version)
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

impl Drop for AuthenticateFrame {
    fn drop(&mut self) {
        self.credential.zeroize();
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ResourceRefDto {
    Workspace {
        id: WorkspaceId,
    },
    Document {
        id: DocumentId,
    },
    Settings {},
    SyncConfig {},
    SyncStatus {
        #[serde(deserialize_with = "deserialize_required_nullable")]
        run_id: Nullable<RunId>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum DomainEvent {
    WorkspaceChanged {
        workspace: WorkspaceDto,
    },
    DocumentCreated {
        document: DocumentEntryDto,
    },
    DocumentChanged {
        document: DocumentEntryDto,
    },
    DocumentMoved {
        document: DocumentEntryDto,
        previous_path: WorkspaceRelativePath,
    },
    DocumentDeleted {
        document_id: DocumentId,
        previous_path: WorkspaceRelativePath,
        workspace_generation: WorkspaceGeneration,
        revision: Revision,
    },
    SettingsChanged {
        settings: SettingsSnapshotDto,
    },
    SyncConfigChanged {
        config: SyncConfigViewDto,
    },
    SyncStatusChanged {
        status: SyncStatusDto,
    },
}

wire_enum!(GapReason {
    BufferOverflow,
    SequenceExhausted,
});
wire_enum!(ReloadScope {
    Workspace,
    Documents,
    Settings,
    SyncConfig,
    SyncStatus,
});
wire_enum!(FrameErrorCode {
    Unauthorized,
    InvalidFrame,
    UnsupportedVersion,
});

#[derive(Clone, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ServerFrame {
    Ready {
        protocol_version: ProtocolVersion,
        connection_id: ConnectionId,
        instance_id: InstanceId,
        sequence: ReadySequence,
        snapshot_required: SnapshotRequired,
    },
    Event {
        protocol_version: ProtocolVersion,
        connection_id: ConnectionId,
        sequence: EventSequence,
        resource: ResourceRefDto,
        revision: Revision,
        event: Box<DomainEvent>,
    },
    Gap {
        protocol_version: ProtocolVersion,
        connection_id: ConnectionId,
        sequence: EventSequence,
        reason: GapReason,
        reload_scopes: Vec<ReloadScope>,
    },
    Error {
        protocol_version: ProtocolVersion,
        code: FrameErrorCode,
        message: String,
    },
}

impl fmt::Debug for ServerFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready {
                protocol_version,
                connection_id,
                instance_id,
                sequence,
                snapshot_required,
            } => formatter
                .debug_struct("ServerFrame::Ready")
                .field("protocol_version", protocol_version)
                .field("connection_id", connection_id)
                .field("instance_id", instance_id)
                .field("sequence", sequence)
                .field("snapshot_required", snapshot_required)
                .finish(),
            Self::Event {
                protocol_version,
                connection_id,
                sequence,
                resource,
                revision,
                ..
            } => formatter
                .debug_struct("ServerFrame::Event")
                .field("protocol_version", protocol_version)
                .field("connection_id", connection_id)
                .field("sequence", sequence)
                .field("resource", resource)
                .field("revision", revision)
                .field("event", &"[REDACTED]")
                .finish(),
            Self::Gap {
                protocol_version,
                connection_id,
                sequence,
                reason,
                reload_scopes,
            } => formatter
                .debug_struct("ServerFrame::Gap")
                .field("protocol_version", protocol_version)
                .field("connection_id", connection_id)
                .field("sequence", sequence)
                .field("reason", reason)
                .field("reload_scopes", reload_scopes)
                .finish(),
            Self::Error {
                protocol_version,
                code,
                ..
            } => formatter
                .debug_struct("ServerFrame::Error")
                .field("protocol_version", protocol_version)
                .field("code", code)
                .field("message", &"[REDACTED]")
                .finish(),
        }
    }
}

const DOCUMENT_ID_DOMAIN: &[u8] = b"document-id-v1";
const RESOURCE_ID_DOMAIN: &[u8] = b"resource-id-v1";
const PAGE_CURSOR_DOMAIN: &[u8] = b"page-cursor-v1";
const MAX_DOCUMENT_ID_LENGTH: usize = 8_192;
const MAX_RESOURCE_ID_LENGTH: usize = 8_192;
const MAX_PAGE_CURSOR_LENGTH: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SignedIdentityAllocation {
    json_bytes: usize,
    token_bytes: usize,
    transient_bytes: usize,
}

impl SignedIdentityAllocation {
    pub(crate) const fn token_bytes(self) -> usize {
        self.token_bytes
    }

    pub(crate) const fn transient_bytes(self) -> usize {
        self.transient_bytes
    }
}

macro_rules! signed_wire_token {
    ($name:ident, $maximum_length:expr) => {
        #[derive(Clone, Eq, Hash, PartialEq, Serialize, ToSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, InvalidWireIdentity> {
                let value = value.into();
                if !signed_token_syntax_is_valid(&value, $maximum_length) {
                    return Err(InvalidWireIdentity);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(|_| D::Error::custom("invalid signed identity"))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(..)"))
            }
        }
    };
}

signed_wire_token!(DocumentId, MAX_DOCUMENT_ID_LENGTH);
signed_wire_token!(ResourceId, MAX_RESOURCE_ID_LENGTH);
signed_wire_token!(PageCursor, MAX_PAGE_CURSOR_LENGTH);

pub struct WireIdentityKey([u8; 32]);

impl WireIdentityKey {
    pub fn generate() -> Result<Self, WireIdentityKeyGenerationError> {
        let mut key = [0_u8; 32];
        getrandom::fill(&mut key).map_err(|_| WireIdentityKeyGenerationError)?;
        Ok(Self(key))
    }

    pub fn issue_document_id(
        &self,
        workspace_id: WorkspaceId,
        workspace_generation: &WorkspaceGeneration,
        kind: DocumentKind,
        relative_path: &WorkspaceRelativePath,
    ) -> Result<DocumentId, InvalidWireIdentity> {
        let allocation =
            self.document_id_allocation(workspace_id, workspace_generation, kind, relative_path)?;
        let payload = DocumentIdPayload {
            version: 1,
            workspace_id,
            workspace_generation: workspace_generation.clone(),
            kind,
            relative_path: relative_path.clone(),
        };
        DocumentId::parse(self.issue_signed_with_allocation(
            DOCUMENT_ID_DOMAIN,
            &payload,
            allocation,
        )?)
    }

    pub(crate) fn document_id_allocation(
        &self,
        workspace_id: WorkspaceId,
        workspace_generation: &WorkspaceGeneration,
        kind: DocumentKind,
        relative_path: &WorkspaceRelativePath,
    ) -> Result<SignedIdentityAllocation, InvalidWireIdentity> {
        signed_identity_allocation(
            &BorrowedDocumentIdPayload {
                version: 1,
                workspace_id,
                workspace_generation,
                kind,
                relative_path,
            },
            workspace_generation,
            relative_path,
            MAX_DOCUMENT_ID_LENGTH,
        )
    }

    pub fn verify_document_id(
        &self,
        document_id: &DocumentId,
        expected_workspace_id: WorkspaceId,
        expected_workspace_generation: &WorkspaceGeneration,
        expected_kind: DocumentKind,
    ) -> Result<WorkspaceRelativePath, InvalidWireIdentity> {
        let payload: DocumentIdPayload =
            self.verify_signed(DOCUMENT_ID_DOMAIN, document_id.as_str())?;
        if payload.version != 1
            || payload.workspace_id != expected_workspace_id
            || payload.workspace_generation != *expected_workspace_generation
            || payload.kind != expected_kind
        {
            return Err(InvalidWireIdentity);
        }
        Ok(payload.relative_path)
    }

    pub fn issue_resource_id(
        &self,
        workspace_id: WorkspaceId,
        workspace_generation: &WorkspaceGeneration,
        kind: ResourceKind,
        relative_path: &WorkspaceRelativePath,
    ) -> Result<ResourceId, InvalidWireIdentity> {
        let allocation =
            self.resource_id_allocation(workspace_id, workspace_generation, kind, relative_path)?;
        let payload = ResourceIdPayload {
            version: 1,
            workspace_id,
            workspace_generation: workspace_generation.clone(),
            kind,
            relative_path: relative_path.clone(),
        };
        ResourceId::parse(self.issue_signed_with_allocation(
            RESOURCE_ID_DOMAIN,
            &payload,
            allocation,
        )?)
    }

    pub(crate) fn resource_id_allocation(
        &self,
        workspace_id: WorkspaceId,
        workspace_generation: &WorkspaceGeneration,
        kind: ResourceKind,
        relative_path: &WorkspaceRelativePath,
    ) -> Result<SignedIdentityAllocation, InvalidWireIdentity> {
        signed_identity_allocation(
            &BorrowedResourceIdPayload {
                version: 1,
                workspace_id,
                workspace_generation,
                kind,
                relative_path,
            },
            workspace_generation,
            relative_path,
            MAX_RESOURCE_ID_LENGTH,
        )
    }

    pub fn verify_resource_id(
        &self,
        resource_id: &ResourceId,
        expected_workspace_id: WorkspaceId,
        expected_workspace_generation: &WorkspaceGeneration,
        expected_kind: ResourceKind,
    ) -> Result<WorkspaceRelativePath, InvalidWireIdentity> {
        let payload: ResourceIdPayload =
            self.verify_signed(RESOURCE_ID_DOMAIN, resource_id.as_str())?;
        if payload.version != 1
            || payload.workspace_id != expected_workspace_id
            || payload.workspace_generation != *expected_workspace_generation
            || payload.kind != expected_kind
        {
            return Err(InvalidWireIdentity);
        }
        Ok(payload.relative_path)
    }

    pub fn issue_page_cursor(
        &self,
        context: &PageCursorContext,
        last_logical_identity: impl Into<String>,
    ) -> Result<PageCursor, InvalidWireIdentity> {
        let last_logical_identity = last_logical_identity.into();
        if last_logical_identity.is_empty() {
            return Err(InvalidWireIdentity);
        }
        let payload = PageCursorPayload {
            version: 1,
            operation: context.operation.clone(),
            query_digest: context.query_digest,
            workspace_generation: context.workspace_generation.clone(),
            last_logical_identity,
        };
        PageCursor::parse(self.issue_signed(PAGE_CURSOR_DOMAIN, &payload)?)
    }

    pub fn verify_page_cursor(
        &self,
        cursor: &PageCursor,
        context: &PageCursorContext,
    ) -> Result<String, InvalidWireIdentity> {
        let payload: PageCursorPayload = self.verify_signed(PAGE_CURSOR_DOMAIN, cursor.as_str())?;
        if payload.version != 1
            || payload.operation != context.operation
            || payload.query_digest != context.query_digest
            || payload.workspace_generation != context.workspace_generation
            || payload.last_logical_identity.is_empty()
        {
            return Err(InvalidWireIdentity);
        }
        Ok(payload.last_logical_identity)
    }

    fn issue_signed<T: Serialize>(
        &self,
        domain: &[u8],
        payload: &T,
    ) -> Result<String, InvalidWireIdentity> {
        #[cfg(test)]
        SIGNED_PAYLOAD_SERIALIZATIONS.set(SIGNED_PAYLOAD_SERIALIZATIONS.get().saturating_add(1));
        let payload = serde_json::to_vec(payload).map_err(|_| InvalidWireIdentity)?;
        let signature = self.sign(domain, &payload)?;
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }

    fn issue_signed_with_allocation<T: Serialize>(
        &self,
        domain: &[u8],
        payload: &T,
        allocation: SignedIdentityAllocation,
    ) -> Result<String, InvalidWireIdentity> {
        #[cfg(test)]
        SIGNED_PAYLOAD_SERIALIZATIONS.set(SIGNED_PAYLOAD_SERIALIZATIONS.get().saturating_add(1));
        let mut payload_bytes = Vec::with_capacity(allocation.json_bytes);
        serde_json::to_writer(&mut payload_bytes, payload).map_err(|_| InvalidWireIdentity)?;
        if payload_bytes.len() != allocation.json_bytes {
            return Err(InvalidWireIdentity);
        }
        let signature = self.sign(domain, &payload_bytes)?;
        let mut token = String::with_capacity(allocation.token_bytes);
        URL_SAFE_NO_PAD.encode_string(payload_bytes, &mut token);
        token.push('.');
        URL_SAFE_NO_PAD.encode_string(signature, &mut token);
        if token.len() != allocation.token_bytes {
            return Err(InvalidWireIdentity);
        }
        Ok(token)
    }

    fn verify_signed<T: for<'de> Deserialize<'de>>(
        &self,
        domain: &[u8],
        token: &str,
    ) -> Result<T, InvalidWireIdentity> {
        let (payload, signature) = split_signed_token(token)?;
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| InvalidWireIdentity)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| InvalidWireIdentity)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).map_err(|_| InvalidWireIdentity)?;
        mac.update(domain);
        mac.update(&[0]);
        mac.update(&payload);
        mac.verify_slice(&signature)
            .map_err(|_| InvalidWireIdentity)?;
        serde_json::from_slice(&payload).map_err(|_| InvalidWireIdentity)
    }

    fn sign(&self, domain: &[u8], payload: &[u8]) -> Result<[u8; 32], InvalidWireIdentity> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).map_err(|_| InvalidWireIdentity)?;
        mac.update(domain);
        mac.update(&[0]);
        mac.update(payload);
        Ok(mac.finalize().into_bytes().into())
    }
}

impl fmt::Debug for WireIdentityKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WireIdentityKey([REDACTED])")
    }
}

impl Drop for WireIdentityKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageCursorContext {
    operation: String,
    query_digest: [u8; 32],
    workspace_generation: WorkspaceGeneration,
}

impl PageCursorContext {
    pub fn new<Snapshot: Serialize + ?Sized>(
        operation: impl Into<String>,
        normalized_query: impl AsRef<str>,
        workspace_generation: &WorkspaceGeneration,
        collection_snapshot: &Snapshot,
    ) -> Result<Self, InvalidWireIdentity> {
        let operation = operation.into();
        if operation.is_empty() || operation.chars().any(char::is_control) {
            return Err(InvalidWireIdentity);
        }
        let normalized_query = normalized_query.as_ref().as_bytes();
        let query_length =
            u64::try_from(normalized_query.len()).map_err(|_| InvalidWireIdentity)?;
        let mut context_digest = Sha256::new();
        context_digest.update(b"qingyu-page-cursor-context-v2\0");
        context_digest.update(query_length.to_be_bytes());
        context_digest.update(normalized_query);
        serde_json::to_writer(DigestWriter(&mut context_digest), collection_snapshot)
            .map_err(|_| InvalidWireIdentity)?;
        Ok(Self {
            operation,
            query_digest: context_digest.finalize().into(),
            workspace_generation: workspace_generation.clone(),
        })
    }
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DocumentIdPayload {
    version: u8,
    workspace_id: WorkspaceId,
    workspace_generation: WorkspaceGeneration,
    kind: DocumentKind,
    relative_path: WorkspaceRelativePath,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BorrowedDocumentIdPayload<'a> {
    version: u8,
    workspace_id: WorkspaceId,
    workspace_generation: &'a WorkspaceGeneration,
    kind: DocumentKind,
    relative_path: &'a WorkspaceRelativePath,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ResourceIdPayload {
    version: u8,
    workspace_id: WorkspaceId,
    workspace_generation: WorkspaceGeneration,
    kind: ResourceKind,
    relative_path: WorkspaceRelativePath,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BorrowedResourceIdPayload<'a> {
    version: u8,
    workspace_id: WorkspaceId,
    workspace_generation: &'a WorkspaceGeneration,
    kind: ResourceKind,
    relative_path: &'a WorkspaceRelativePath,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PageCursorPayload {
    version: u8,
    operation: String,
    query_digest: [u8; 32],
    workspace_generation: WorkspaceGeneration,
    last_logical_identity: String,
}

struct DigestWriter<'a>(&'a mut Sha256);

#[derive(Default)]
struct SerializedLengthWriter {
    length: usize,
}

impl io::Write for SerializedLengthWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.length = self.length.checked_add(bytes.len()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "serialized value is too large")
        })?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn signed_identity_allocation(
    payload: &impl Serialize,
    workspace_generation: &WorkspaceGeneration,
    relative_path: &WorkspaceRelativePath,
    maximum_token_bytes: usize,
) -> Result<SignedIdentityAllocation, InvalidWireIdentity> {
    let mut writer = SerializedLengthWriter::default();
    serde_json::to_writer(&mut writer, payload).map_err(|_| InvalidWireIdentity)?;
    let payload_token_bytes = base64_unpadded_length(writer.length)?;
    let signature_token_bytes = base64_unpadded_length(32)?;
    let token_bytes = payload_token_bytes
        .checked_add(1)
        .and_then(|bytes| bytes.checked_add(signature_token_bytes))
        .ok_or(InvalidWireIdentity)?;
    if token_bytes > maximum_token_bytes {
        return Err(InvalidWireIdentity);
    }
    let transient_bytes = workspace_generation
        .as_str()
        .len()
        .checked_add(relative_path.as_str().len())
        .and_then(|bytes| bytes.checked_add(writer.length))
        .ok_or(InvalidWireIdentity)?;
    Ok(SignedIdentityAllocation {
        json_bytes: writer.length,
        token_bytes,
        transient_bytes,
    })
}

fn base64_unpadded_length(bytes: usize) -> Result<usize, InvalidWireIdentity> {
    let complete = bytes.checked_div(3).ok_or(InvalidWireIdentity)?;
    let remainder = bytes % 3;
    complete
        .checked_mul(4)
        .and_then(|encoded| {
            encoded.checked_add(match remainder {
                0 => 0,
                1 => 2,
                _ => 3,
            })
        })
        .ok_or(InvalidWireIdentity)
}

impl io::Write for DigestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidWireIdentity;

impl fmt::Display for InvalidWireIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid wire identity")
    }
}

impl std::error::Error for InvalidWireIdentity {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WireIdentityKeyGenerationError;

impl fmt::Display for WireIdentityKeyGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("wire identity key generation failed")
    }
}

impl std::error::Error for WireIdentityKeyGenerationError {}

fn signed_token_syntax_is_valid(value: &str, maximum_length: usize) -> bool {
    if value.is_empty() || value.len() > maximum_length || !value.is_ascii() {
        return false;
    }
    let Ok((payload, signature)) = split_signed_token(value) else {
        return false;
    };
    !payload.is_empty()
        && !signature.is_empty()
        && payload.bytes().all(is_base64url_byte)
        && signature.bytes().all(is_base64url_byte)
}

fn split_signed_token(value: &str) -> Result<(&str, &str), InvalidWireIdentity> {
    let mut parts = value.split('.');
    let payload = parts.next().ok_or(InvalidWireIdentity)?;
    let signature = parts.next().ok_or(InvalidWireIdentity)?;
    if parts.next().is_some() {
        return Err(InvalidWireIdentity);
    }
    Ok((payload, signature))
}

fn is_base64url_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct WorkspaceRelativePath(String);

impl<'de> Deserialize<'de> for WorkspaceRelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("invalid workspace-relative path"))
    }
}

impl WorkspaceRelativePath {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidWorkspaceRelativePath> {
        let value = value.into();
        if !workspace_relative_path_is_valid(&value) {
            return Err(InvalidWorkspaceRelativePath);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidWorkspaceRelativePath;

fn workspace_relative_path_is_valid(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    if value.starts_with(['/', '\\'])
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || value
            .as_bytes()
            .get(..2)
            .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1] == b':')
    {
        return false;
    }

    value
        .split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

#[cfg(test)]
mod tests {
    use super::{
        DocumentKind, ResourceKind, WireIdentityKey, WorkspaceGeneration, WorkspaceId,
        WorkspaceRelativePath, SIGNED_PAYLOAD_SERIALIZATIONS,
    };
    use uuid::Uuid;

    #[test]
    fn oversized_document_and_resource_ids_are_rejected_before_payload_serialization() {
        let key = WireIdentityKey([7; 32]);
        let workspace_id = WorkspaceId::new(Uuid::nil());
        let generation = WorkspaceGeneration::parse("generation-1").unwrap();
        let path = WorkspaceRelativePath::parse("x".repeat(16 * 1024)).unwrap();

        SIGNED_PAYLOAD_SERIALIZATIONS.set(0);
        assert!(key
            .issue_document_id(workspace_id, &generation, DocumentKind::File, &path)
            .is_err());
        assert_eq!(SIGNED_PAYLOAD_SERIALIZATIONS.get(), 0);

        SIGNED_PAYLOAD_SERIALIZATIONS.set(0);
        assert!(key
            .issue_resource_id(workspace_id, &generation, ResourceKind::Attachment, &path)
            .is_err());
        assert_eq!(SIGNED_PAYLOAD_SERIALIZATIONS.get(), 0);
    }

    #[test]
    fn preflighted_document_and_resource_ids_keep_the_legacy_signed_bytes() {
        let key = WireIdentityKey([11; 32]);
        let workspace_id = WorkspaceId::new(Uuid::nil());
        let generation = WorkspaceGeneration::parse("generation-\n-\\").unwrap();
        let path = WorkspaceRelativePath::parse("folder/图\"片.md").unwrap();

        let legacy_document = key
            .issue_signed(
                super::DOCUMENT_ID_DOMAIN,
                &super::DocumentIdPayload {
                    version: 1,
                    workspace_id,
                    workspace_generation: generation.clone(),
                    kind: DocumentKind::File,
                    relative_path: path.clone(),
                },
            )
            .unwrap();
        let document = key
            .issue_document_id(workspace_id, &generation, DocumentKind::File, &path)
            .unwrap();
        assert_eq!(document.as_str(), legacy_document);

        let legacy_resource = key
            .issue_signed(
                super::RESOURCE_ID_DOMAIN,
                &super::ResourceIdPayload {
                    version: 1,
                    workspace_id,
                    workspace_generation: generation.clone(),
                    kind: ResourceKind::Attachment,
                    relative_path: path.clone(),
                },
            )
            .unwrap();
        let resource = key
            .issue_resource_id(workspace_id, &generation, ResourceKind::Attachment, &path)
            .unwrap();
        assert_eq!(resource.as_str(), legacy_resource);
    }
}
