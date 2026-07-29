use std::{
    fmt,
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize as _, Zeroizing};

use crate::{config::NativeLaunchCredential, contract::InstanceId};

pub const NATIVE_HOST_PROTOCOL_VERSION: u16 = 1;
pub const MAX_NATIVE_HOST_FRAME_BYTES: usize = 64 * 1024;

/// A native host launch request read from the inherited control pipe.
///
/// This secret-bearing type deliberately implements neither `Clone` nor
/// `Serialize`. Native hosts must use [`Self::write_json_line`] so the wire
/// format and its single intentional credential exposure stay centralized.
pub struct NativeHostStart {
    workspace_root: PathBuf,
    app_data_root: PathBuf,
    cache_root: PathBuf,
    origin: String,
    credential: NativeLaunchCredential,
}

impl NativeHostStart {
    pub fn desktop(
        workspace_root: PathBuf,
        app_data_root: PathBuf,
        cache_root: PathBuf,
        origin: String,
        credential: NativeLaunchCredential,
    ) -> Self {
        Self {
            workspace_root,
            app_data_root,
            cache_root,
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
                origin: &self.origin,
                credential: self.credential.expose_secret(),
            },
        )
        .map_err(|_| NativeHostProtocolError)
    }

    pub fn into_parts(self) -> (PathBuf, PathBuf, PathBuf, String, NativeLaunchCredential) {
        (
            self.workspace_root,
            self.app_data_root,
            self.cache_root,
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
        Ok(Self {
            workspace_root: frame.workspace_root,
            app_data_root: frame.app_data_root,
            cache_root: frame.cache_root,
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
            "tauri://localhost".to_owned(),
            NativeLaunchCredential::from_secret(CREDENTIAL.to_owned()).unwrap(),
        );
        let mut encoded = Vec::new();
        start.write_json_line(&mut encoded).unwrap();

        let decoded =
            NativeHostStart::read_json_line(&mut BufReader::new(Cursor::new(encoded))).unwrap();
        let (workspace, app_data, cache, origin, credential) = decoded.into_parts();

        assert_eq!(workspace, PathBuf::from("workspace"));
        assert_eq!(app_data, PathBuf::from("app-data"));
        assert_eq!(cache, PathBuf::from("cache"));
        assert_eq!(origin, "tauri://localhost");
        assert!(credential.matches(CREDENTIAL));
    }

    #[test]
    fn shared_start_writer_rejects_an_oversized_frame_before_writing_any_bytes() {
        let start = NativeHostStart::desktop(
            PathBuf::from("workspace"),
            PathBuf::from("app-data"),
            PathBuf::from("cache"),
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
