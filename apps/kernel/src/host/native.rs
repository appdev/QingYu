use std::{
    fmt,
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use cap_fs_ext::MetadataExt as _;
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize as _, Zeroizing};

use crate::{
    config::NativeLaunchCredential, contract::InstanceId, workspace::primary::PrimaryWorkspaceState,
};

pub const NATIVE_HOST_PROTOCOL_VERSION: u16 = 2;
pub const MAX_NATIVE_HOST_FRAME_BYTES: usize = 64 * 1024;

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NativeHostWorkspaceState {
    primary_workspace: PrimaryWorkspaceState,
    root_binding: String,
}

impl NativeHostWorkspaceState {
    pub fn for_workspace(
        workspace_root: &Path,
        display_name: impl Into<String>,
    ) -> Result<Self, NativeHostProtocolError> {
        let directory = Dir::open_ambient_dir(workspace_root, cap_std::ambient_authority())
            .map_err(|_| NativeHostProtocolError)?;
        let state = Self {
            primary_workspace: PrimaryWorkspaceState::new(display_name)
                .map_err(|_| NativeHostProtocolError)?,
            root_binding: root_binding(&directory)?,
        };
        state.validate()?;
        Ok(state)
    }

    /// Decodes a host-persisted, path-free workspace identity.
    ///
    /// Native shells own this value as an opaque record. Decoding always
    /// revalidates its schema and canonical root binding before it can be
    /// supplied to a child Kernel.
    pub fn from_value(value: Value) -> Result<Self, NativeHostProtocolError> {
        let state: Self = serde_json::from_value(value).map_err(|_| NativeHostProtocolError)?;
        state.validate()?;
        Ok(state)
    }

    /// Encodes the opaque state for host-owned persistence.
    pub fn to_value(&self) -> Result<Value, NativeHostProtocolError> {
        self.validate()?;
        serde_json::to_value(self).map_err(|_| NativeHostProtocolError)
    }

    /// Checks whether this persisted identity belongs to the retained
    /// filesystem object currently addressed by `workspace_root`.
    pub fn matches_workspace(
        &self,
        workspace_root: &Path,
    ) -> Result<bool, NativeHostProtocolError> {
        self.validate()?;
        let directory = Dir::open_ambient_dir(workspace_root, cap_std::ambient_authority())
            .map_err(|_| NativeHostProtocolError)?;
        Ok(self.root_binding == root_binding(&directory)?)
    }

    fn validate(&self) -> Result<(), NativeHostProtocolError> {
        self.primary_workspace
            .validate()
            .map_err(|_| NativeHostProtocolError)?;
        let decoded = URL_SAFE_NO_PAD
            .decode(self.root_binding.as_bytes())
            .map_err(|_| NativeHostProtocolError)?;
        if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(decoded) != self.root_binding {
            return Err(NativeHostProtocolError);
        }
        Ok(())
    }

    pub(crate) fn validate_directory(
        &self,
        directory: &Dir,
    ) -> Result<(), NativeHostProtocolError> {
        self.validate()?;
        if self.root_binding != root_binding(directory)? {
            return Err(NativeHostProtocolError);
        }
        Ok(())
    }

    pub(crate) fn into_primary_workspace(self) -> PrimaryWorkspaceState {
        self.primary_workspace
    }

    pub fn display_name(&self) -> &str {
        self.primary_workspace.display_name()
    }
}

impl fmt::Debug for NativeHostWorkspaceState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeHostWorkspaceState([REDACTED])")
    }
}

#[cfg(not(windows))]
fn root_binding(directory: &Dir) -> Result<String, NativeHostProtocolError> {
    let metadata = directory
        .dir_metadata()
        .map_err(|_| NativeHostProtocolError)?;
    if !metadata.is_dir() {
        return Err(NativeHostProtocolError);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"qingyu-native-workspace-root-unix-v1\0");
    hasher.update(metadata.dev().to_le_bytes());
    hasher.update(metadata.ino().to_le_bytes());
    Ok(URL_SAFE_NO_PAD.encode(hasher.finalize()))
}

#[cfg(windows)]
fn root_binding(directory: &Dir) -> Result<String, NativeHostProtocolError> {
    use std::{mem, os::windows::io::AsRawHandle as _};

    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO},
    };

    let metadata = directory
        .dir_metadata()
        .map_err(|_| NativeHostProtocolError)?;
    if !metadata.is_dir() {
        return Err(NativeHostProtocolError);
    }
    let mut info = FILE_ID_INFO::default();
    // SAFETY: `directory` owns a live directory handle, `info` is writable for
    // exactly the supplied `FILE_ID_INFO` size, and the call does not retain it.
    let result = unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle() as HANDLE,
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            u32::try_from(mem::size_of::<FILE_ID_INFO>()).map_err(|_| NativeHostProtocolError)?,
        )
    };
    if result == 0 {
        return Err(NativeHostProtocolError);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"qingyu-native-workspace-root-windows-v1\0");
    hasher.update(info.VolumeSerialNumber.to_le_bytes());
    hasher.update(info.FileId.Identifier);
    Ok(URL_SAFE_NO_PAD.encode(hasher.finalize()))
}

/// A native host launch request read from the inherited control pipe.
///
/// This secret-bearing type deliberately implements neither `Clone` nor
/// `Serialize`. Native hosts must use [`Self::write_json_line`] so the wire
/// format and its single intentional credential exposure stay centralized.
pub struct NativeHostStart {
    workspace_root: PathBuf,
    app_data_root: PathBuf,
    cache_root: PathBuf,
    workspace_state: NativeHostWorkspaceState,
    origin: String,
    credential: NativeLaunchCredential,
}

impl NativeHostStart {
    pub fn desktop(
        workspace_root: PathBuf,
        app_data_root: PathBuf,
        cache_root: PathBuf,
        workspace_state: NativeHostWorkspaceState,
        origin: String,
        credential: NativeLaunchCredential,
    ) -> Self {
        Self {
            workspace_root,
            app_data_root,
            cache_root,
            workspace_state,
            origin,
            credential,
        }
    }

    pub fn read_json_line(reader: &mut impl BufRead) -> Result<Self, NativeHostProtocolError> {
        let bytes = read_required_json_line(reader)?;
        let decoded = serde_json::from_slice::<RawStartFrame>(&bytes);
        drop(bytes);
        let decoded = decoded.map_err(|_| NativeHostProtocolError)?;
        decoded.try_into()
    }

    pub fn write_json_line(&self, writer: &mut impl Write) -> Result<(), NativeHostProtocolError> {
        let mut size_check = FrameSizeCounter::new(MAX_NATIVE_HOST_FRAME_BYTES);
        self.write_json_payload(&mut size_check)?;
        self.write_json_payload(writer)?;
        writer
            .write_all(b"\n")
            .map_err(|_| NativeHostProtocolError)?;
        writer.flush().map_err(|_| NativeHostProtocolError)
    }

    fn write_json_payload(&self, writer: &mut impl Write) -> Result<(), NativeHostProtocolError> {
        serde_json::to_writer(
            writer,
            &StartFrameRef {
                kind: StartFrameKind::Start,
                protocol_version: NATIVE_HOST_PROTOCOL_VERSION,
                profile: DesktopProfile::Desktop,
                workspace_root: &self.workspace_root,
                app_data_root: &self.app_data_root,
                cache_root: &self.cache_root,
                workspace_state: &self.workspace_state,
                origin: &self.origin,
                credential: self.credential.expose_secret(),
            },
        )
        .map_err(|_| NativeHostProtocolError)
    }

    pub fn into_parts(
        self,
    ) -> (
        PathBuf,
        PathBuf,
        PathBuf,
        NativeHostWorkspaceState,
        String,
        NativeLaunchCredential,
    ) {
        (
            self.workspace_root,
            self.app_data_root,
            self.cache_root,
            self.workspace_state,
            self.origin,
            self.credential,
        )
    }
}

impl fmt::Debug for NativeHostStart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeHostStart([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHostControl {
    Shutdown,
    EndOfStream,
}

impl NativeHostControl {
    pub fn read_json_line(reader: &mut impl BufRead) -> Result<Self, NativeHostProtocolError> {
        let Some(bytes) = read_optional_json_line(reader)? else {
            return Ok(Self::EndOfStream);
        };
        let decoded = serde_json::from_slice::<RawControlFrame>(&bytes);
        drop(bytes);
        let decoded = decoded.map_err(|_| NativeHostProtocolError)?;
        if decoded.protocol_version != NATIVE_HOST_PROTOCOL_VERSION {
            return Err(NativeHostProtocolError);
        }
        match decoded.kind {
            ControlFrameKind::Shutdown => Ok(Self::Shutdown),
        }
    }

    pub fn write_shutdown_json_line(
        writer: &mut impl Write,
    ) -> Result<(), NativeHostProtocolError> {
        serde_json::to_writer(
            &mut *writer,
            &ControlFrameRef {
                kind: ControlFrameKind::Shutdown,
                protocol_version: NATIVE_HOST_PROTOCOL_VERSION,
            },
        )
        .map_err(|_| NativeHostProtocolError)?;
        writer
            .write_all(b"\n")
            .map_err(|_| NativeHostProtocolError)?;
        writer.flush().map_err(|_| NativeHostProtocolError)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NativeHostReady {
    #[serde(rename = "type")]
    kind: ReadyFrameKind,
    protocol_version: u16,
    port: u16,
    instance_id: InstanceId,
}

impl NativeHostReady {
    pub const fn new(port: u16, instance_id: InstanceId) -> Self {
        Self {
            kind: ReadyFrameKind::Ready,
            protocol_version: NATIVE_HOST_PROTOCOL_VERSION,
            port,
            instance_id,
        }
    }

    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub const fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    pub fn read_json_line(reader: &mut impl BufRead) -> Result<Self, NativeHostProtocolError> {
        let bytes = read_required_json_line(reader)?;
        let ready = serde_json::from_slice::<Self>(&bytes).map_err(|_| NativeHostProtocolError)?;
        if ready.protocol_version != NATIVE_HOST_PROTOCOL_VERSION {
            return Err(NativeHostProtocolError);
        }
        Ok(ready)
    }

    pub fn write_json_line(&self, writer: &mut impl Write) -> Result<(), NativeHostProtocolError> {
        serde_json::to_writer(&mut *writer, self).map_err(|_| NativeHostProtocolError)?;
        writer
            .write_all(b"\n")
            .map_err(|_| NativeHostProtocolError)?;
        writer.flush().map_err(|_| NativeHostProtocolError)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeHostProtocolError;

impl fmt::Display for NativeHostProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("native host protocol error")
    }
}

impl std::error::Error for NativeHostProtocolError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawStartFrame {
    #[serde(rename = "type")]
    kind: StartFrameKind,
    protocol_version: u16,
    profile: DesktopProfile,
    workspace_root: PathBuf,
    app_data_root: PathBuf,
    cache_root: PathBuf,
    workspace_state: NativeHostWorkspaceState,
    origin: String,
    credential: CredentialInput,
}

impl TryFrom<RawStartFrame> for NativeHostStart {
    type Error = NativeHostProtocolError;

    fn try_from(frame: RawStartFrame) -> Result<Self, Self::Error> {
        if frame.protocol_version != NATIVE_HOST_PROTOCOL_VERSION {
            return Err(NativeHostProtocolError);
        }
        let StartFrameKind::Start = frame.kind;
        let DesktopProfile::Desktop = frame.profile;
        frame
            .workspace_state
            .validate()
            .map_err(|_| NativeHostProtocolError)?;
        Ok(Self {
            workspace_root: frame.workspace_root,
            app_data_root: frame.app_data_root,
            cache_root: frame.cache_root,
            workspace_state: frame.workspace_state,
            origin: frame.origin,
            credential: frame.credential.into_native()?,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartFrameRef<'a> {
    #[serde(rename = "type")]
    kind: StartFrameKind,
    protocol_version: u16,
    profile: DesktopProfile,
    workspace_root: &'a Path,
    app_data_root: &'a Path,
    cache_root: &'a Path,
    workspace_state: &'a NativeHostWorkspaceState,
    origin: &'a str,
    credential: &'a str,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum StartFrameKind {
    Start,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DesktopProfile {
    Desktop,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawControlFrame {
    #[serde(rename = "type")]
    kind: ControlFrameKind,
    protocol_version: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ControlFrameRef {
    #[serde(rename = "type")]
    kind: ControlFrameKind,
    protocol_version: u16,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ControlFrameKind {
    Shutdown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ReadyFrameKind {
    Ready,
}

#[derive(Deserialize)]
#[serde(transparent)]
struct CredentialInput(String);

impl CredentialInput {
    fn into_native(mut self) -> Result<NativeLaunchCredential, NativeHostProtocolError> {
        NativeLaunchCredential::from_secret(std::mem::take(&mut self.0))
            .map_err(|_| NativeHostProtocolError)
    }
}

impl Drop for CredentialInput {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

fn read_required_json_line(
    reader: &mut impl BufRead,
) -> Result<Zeroizing<Vec<u8>>, NativeHostProtocolError> {
    read_optional_json_line(reader)?.ok_or(NativeHostProtocolError)
}

fn read_optional_json_line(
    reader: &mut impl BufRead,
) -> Result<Option<Zeroizing<Vec<u8>>>, NativeHostProtocolError> {
    let limit =
        u64::try_from(MAX_NATIVE_HOST_FRAME_BYTES + 2).map_err(|_| NativeHostProtocolError)?;
    let mut bytes = Zeroizing::new(Vec::new());
    let read = std::io::Read::take(reader, limit)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| NativeHostProtocolError)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.last() != Some(&b'\n') {
        return Err(NativeHostProtocolError);
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.len() > MAX_NATIVE_HOST_FRAME_BYTES {
        return Err(NativeHostProtocolError);
    }
    Ok(Some(bytes))
}

struct FrameSizeCounter {
    written: usize,
    limit: usize,
}

impl FrameSizeCounter {
    const fn new(limit: usize) -> Self {
        Self { written: 0, limit }
    }
}

impl Write for FrameSizeCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self
            .written
            .checked_add(bytes.len())
            .filter(|written| *written <= self.limit)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "native host frame exceeds the protocol limit",
                )
            })?;
        self.written = written;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use static_assertions::assert_not_impl_any;

    use super::*;

    const CREDENTIAL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn workspace_state(display_name: &str) -> NativeHostWorkspaceState {
        let directory = tempfile::tempdir().unwrap();
        NativeHostWorkspaceState::for_workspace(directory.path(), display_name).unwrap()
    }

    #[test]
    fn secret_bearing_start_is_not_cloneable_or_generically_serializable() {
        assert_not_impl_any!(NativeHostStart: Clone, Serialize);
    }

    #[test]
    fn start_debug_output_is_fully_redacted() {
        let start = NativeHostStart::desktop(
            PathBuf::from("private-workspace"),
            PathBuf::from("private-app-data"),
            PathBuf::from("private-cache"),
            workspace_state("Private Workspace"),
            "tauri://localhost".to_owned(),
            NativeLaunchCredential::from_secret(CREDENTIAL.to_owned()).unwrap(),
        );

        assert_eq!(format!("{start:?}"), "NativeHostStart([REDACTED])");
    }

    #[test]
    fn shared_start_writer_round_trips_through_the_bounded_parser() {
        let start = NativeHostStart::desktop(
            PathBuf::from("workspace"),
            PathBuf::from("app-data"),
            PathBuf::from("cache"),
            workspace_state("Workspace"),
            "tauri://localhost".to_owned(),
            NativeLaunchCredential::from_secret(CREDENTIAL.to_owned()).unwrap(),
        );
        let mut encoded = Vec::new();
        start.write_json_line(&mut encoded).unwrap();

        let decoded =
            NativeHostStart::read_json_line(&mut BufReader::new(Cursor::new(encoded))).unwrap();
        let (workspace, app_data, cache, workspace_state, origin, credential) =
            decoded.into_parts();

        assert_eq!(workspace, PathBuf::from("workspace"));
        assert_eq!(app_data, PathBuf::from("app-data"));
        assert_eq!(cache, PathBuf::from("cache"));
        assert_eq!(workspace_state.display_name(), "Workspace");
        assert_eq!(origin, "tauri://localhost");
        assert!(credential.matches(CREDENTIAL));
    }

    #[test]
    fn start_parser_accepts_a_committed_opaque_workspace_state() {
        let encoded = format!(
            "{}\n",
            serde_json::json!({
                "type": "start",
                "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
                "profile": "desktop",
                "workspaceRoot": "workspace",
                "appDataRoot": "app-data",
                "cacheRoot": "cache",
                "workspaceState": {
                    "primaryWorkspace": {
                        "schemaVersion": 1,
                        "revisionSeed": "8b14d937-76b2-4776-9ae4-a9c6e0c403c4",
                        "displayName": "Notes"
                    },
                    "rootBinding": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                },
                "origin": "tauri://localhost",
                "credential": CREDENTIAL,
            })
        );

        NativeHostStart::read_json_line(&mut BufReader::new(Cursor::new(encoded)))
            .expect("a host-committed workspace state must be accepted");
    }

    #[test]
    fn persisted_workspace_state_round_trips_and_matches_only_its_directory() {
        let first = tempfile::tempdir().expect("first workspace");
        let second = tempfile::tempdir().expect("second workspace");
        let state = NativeHostWorkspaceState::for_workspace(first.path(), "Notes")
            .expect("workspace state");

        let decoded = NativeHostWorkspaceState::from_value(
            state.to_value().expect("persisted workspace state"),
        )
        .expect("decoded workspace state");

        assert_eq!(decoded, state);
        assert!(decoded
            .matches_workspace(first.path())
            .expect("matching workspace"));
        assert!(!decoded
            .matches_workspace(second.path())
            .expect("different workspace"));
    }

    #[test]
    fn persisted_workspace_state_rejects_noncanonical_or_unknown_fields() {
        let state = workspace_state("Notes");
        let mut value = state.to_value().expect("persisted workspace state");
        value["rootBinding"] = Value::String("not-canonical".to_string());
        assert_eq!(
            NativeHostWorkspaceState::from_value(value).expect_err("invalid binding"),
            NativeHostProtocolError
        );

        let mut value = state.to_value().expect("persisted workspace state");
        value["workspacePath"] = Value::String("/private/notes".to_string());
        assert_eq!(
            NativeHostWorkspaceState::from_value(value).expect_err("unknown path field"),
            NativeHostProtocolError
        );
    }

    #[test]
    fn shared_start_writer_rejects_an_oversized_frame_before_writing_any_bytes() {
        let start = NativeHostStart::desktop(
            PathBuf::from("workspace"),
            PathBuf::from("app-data"),
            PathBuf::from("cache"),
            workspace_state("Workspace"),
            "x".repeat(MAX_NATIVE_HOST_FRAME_BYTES),
            NativeLaunchCredential::from_secret(CREDENTIAL.to_owned()).unwrap(),
        );
        let mut output = Vec::new();

        let error = start
            .write_json_line(&mut output)
            .expect_err("oversized native-host frame must fail before publication");

        assert_eq!(error, NativeHostProtocolError);
        assert!(output.is_empty());
    }

    #[test]
    fn server_profile_is_not_part_of_the_desktop_native_host_protocol() {
        let encoded = format!(
            "{}\n",
            serde_json::json!({
                "type": "start",
                "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
                "profile": "server",
                "workspaceRoot": "workspace",
                "appDataRoot": "app-data",
                "cacheRoot": "cache",
                "origin": "http://127.0.0.1:3000",
                "credential": CREDENTIAL,
            })
        );

        let error = NativeHostStart::read_json_line(&mut BufReader::new(Cursor::new(encoded)))
            .expect_err("Docker server bootstrap must not inherit desktop stdin lease semantics");

        assert_eq!(error, NativeHostProtocolError);
    }
}
