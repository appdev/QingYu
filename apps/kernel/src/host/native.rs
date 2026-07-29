use std::{
    fmt,
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use zeroize::Zeroize as _;

use crate::{config::NativeLaunchCredential, contract::InstanceId};

pub const NATIVE_HOST_PROTOCOL_VERSION: u16 = 1;
pub const MAX_NATIVE_HOST_FRAME_BYTES: usize = 64 * 1024;

/// A native host launch request read from the inherited control pipe.
///
/// This secret-bearing type deliberately implements neither `Clone` nor
/// `Serialize`. Native hosts must use [`Self::write_json_line`] so the wire
/// format and its single intentional credential exposure stay centralized.
pub struct NativeHostStart {
    launch: NativeHostLaunch,
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
            launch: NativeHostLaunch::Desktop {
                workspace_root,
                app_data_root,
                cache_root,
            },
            origin,
            credential,
        }
    }

    pub fn server(origin: String, credential: NativeLaunchCredential) -> Self {
        Self {
            launch: NativeHostLaunch::Server,
            origin,
            credential,
        }
    }

    pub fn mobile(
        app_data_root: PathBuf,
        cache_root: PathBuf,
        managed_name: String,
        origin: String,
        credential: NativeLaunchCredential,
    ) -> Self {
        Self {
            launch: NativeHostLaunch::Mobile {
                app_data_root,
                cache_root,
                managed_name,
            },
            origin,
            credential,
        }
    }

    pub fn read_json_line(reader: &mut impl BufRead) -> Result<Self, NativeHostProtocolError> {
        let mut bytes = read_required_json_line(reader)?;
        let decoded = serde_json::from_slice::<RawStartFrame>(&bytes);
        bytes.zeroize();
        let decoded = decoded.map_err(|_| NativeHostProtocolError)?;
        decoded.try_into()
    }

    pub fn write_json_line(&self, writer: &mut impl Write) -> Result<(), NativeHostProtocolError> {
        match &self.launch {
            NativeHostLaunch::Desktop {
                workspace_root,
                app_data_root,
                cache_root,
            } => serde_json::to_writer(
                &mut *writer,
                &StartFrameRef::Desktop {
                    kind: StartFrameKind::Start,
                    protocol_version: NATIVE_HOST_PROTOCOL_VERSION,
                    workspace_root,
                    app_data_root,
                    cache_root,
                    origin: &self.origin,
                    credential: self.credential.expose_secret(),
                },
            ),
            NativeHostLaunch::Server => serde_json::to_writer(
                &mut *writer,
                &StartFrameRef::Server {
                    kind: StartFrameKind::Start,
                    protocol_version: NATIVE_HOST_PROTOCOL_VERSION,
                    origin: &self.origin,
                    credential: self.credential.expose_secret(),
                },
            ),
            NativeHostLaunch::Mobile {
                app_data_root,
                cache_root,
                managed_name,
            } => serde_json::to_writer(
                &mut *writer,
                &StartFrameRef::Mobile {
                    kind: StartFrameKind::Start,
                    protocol_version: NATIVE_HOST_PROTOCOL_VERSION,
                    app_data_root,
                    cache_root,
                    managed_name,
                    origin: &self.origin,
                    credential: self.credential.expose_secret(),
                },
            ),
        }
        .map_err(|_| NativeHostProtocolError)?;
        writer
            .write_all(b"\n")
            .map_err(|_| NativeHostProtocolError)?;
        writer.flush().map_err(|_| NativeHostProtocolError)
    }

    pub fn into_parts(self) -> (NativeHostLaunch, String, NativeLaunchCredential) {
        (self.launch, self.origin, self.credential)
    }
}

impl fmt::Debug for NativeHostStart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeHostStart([REDACTED])")
    }
}

pub enum NativeHostLaunch {
    Desktop {
        workspace_root: PathBuf,
        app_data_root: PathBuf,
        cache_root: PathBuf,
    },
    Server,
    Mobile {
        app_data_root: PathBuf,
        cache_root: PathBuf,
        managed_name: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHostControl {
    Shutdown,
    EndOfStream,
}

impl NativeHostControl {
    pub fn read_json_line(reader: &mut impl BufRead) -> Result<Self, NativeHostProtocolError> {
        let Some(mut bytes) = read_optional_json_line(reader)? else {
            return Ok(Self::EndOfStream);
        };
        let decoded = serde_json::from_slice::<RawControlFrame>(&bytes);
        bytes.zeroize();
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
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "profile"
)]
enum RawStartFrame {
    Desktop {
        #[serde(rename = "type")]
        kind: StartFrameKind,
        protocol_version: u16,
        workspace_root: PathBuf,
        app_data_root: PathBuf,
        cache_root: PathBuf,
        origin: String,
        credential: CredentialInput,
    },
    Server {
        #[serde(rename = "type")]
        kind: StartFrameKind,
        protocol_version: u16,
        origin: String,
        credential: CredentialInput,
    },
    Mobile {
        #[serde(rename = "type")]
        kind: StartFrameKind,
        protocol_version: u16,
        app_data_root: PathBuf,
        cache_root: PathBuf,
        managed_name: String,
        origin: String,
        credential: CredentialInput,
    },
}

impl TryFrom<RawStartFrame> for NativeHostStart {
    type Error = NativeHostProtocolError;

    fn try_from(frame: RawStartFrame) -> Result<Self, Self::Error> {
        let (protocol_version, launch, origin, credential) = match frame {
            RawStartFrame::Desktop {
                kind: StartFrameKind::Start,
                protocol_version,
                workspace_root,
                app_data_root,
                cache_root,
                origin,
                credential,
            } => (
                protocol_version,
                NativeHostLaunch::Desktop {
                    workspace_root,
                    app_data_root,
                    cache_root,
                },
                origin,
                credential,
            ),
            RawStartFrame::Server {
                kind: StartFrameKind::Start,
                protocol_version,
                origin,
                credential,
            } => (
                protocol_version,
                NativeHostLaunch::Server,
                origin,
                credential,
            ),
            RawStartFrame::Mobile {
                kind: StartFrameKind::Start,
                protocol_version,
                app_data_root,
                cache_root,
                managed_name,
                origin,
                credential,
            } => (
                protocol_version,
                NativeHostLaunch::Mobile {
                    app_data_root,
                    cache_root,
                    managed_name,
                },
                origin,
                credential,
            ),
        };
        if protocol_version != NATIVE_HOST_PROTOCOL_VERSION {
            return Err(NativeHostProtocolError);
        }
        Ok(Self {
            launch,
            origin,
            credential: credential.into_native()?,
        })
    }
}

#[derive(Serialize)]
#[serde(
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "profile"
)]
enum StartFrameRef<'a> {
    Desktop {
        #[serde(rename = "type")]
        kind: StartFrameKind,
        protocol_version: u16,
        workspace_root: &'a Path,
        app_data_root: &'a Path,
        cache_root: &'a Path,
        origin: &'a str,
        credential: &'a str,
    },
    Server {
        #[serde(rename = "type")]
        kind: StartFrameKind,
        protocol_version: u16,
        origin: &'a str,
        credential: &'a str,
    },
    Mobile {
        #[serde(rename = "type")]
        kind: StartFrameKind,
        protocol_version: u16,
        app_data_root: &'a Path,
        cache_root: &'a Path,
        managed_name: &'a str,
        origin: &'a str,
        credential: &'a str,
    },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum StartFrameKind {
    Start,
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

fn read_required_json_line(reader: &mut impl BufRead) -> Result<Vec<u8>, NativeHostProtocolError> {
    read_optional_json_line(reader)?.ok_or(NativeHostProtocolError)
}

fn read_optional_json_line(
    reader: &mut impl BufRead,
) -> Result<Option<Vec<u8>>, NativeHostProtocolError> {
    let limit =
        u64::try_from(MAX_NATIVE_HOST_FRAME_BYTES + 2).map_err(|_| NativeHostProtocolError)?;
    let mut bytes = Vec::new();
    let read = std::io::Read::take(reader, limit)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| NativeHostProtocolError)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.last() != Some(&b'\n') {
        bytes.zeroize();
        return Err(NativeHostProtocolError);
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.len() > MAX_NATIVE_HOST_FRAME_BYTES {
        bytes.zeroize();
        return Err(NativeHostProtocolError);
    }
    Ok(Some(bytes))
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
        let (launch, origin, credential) = decoded.into_parts();

        assert!(matches!(launch, NativeHostLaunch::Desktop { .. }));
        assert_eq!(origin, "tauri://localhost");
        assert!(credential.matches(CREDENTIAL));
    }
}
