use std::{fmt, io, path::PathBuf};

use futures::{future, StreamExt};
use rmcp::{
    service::{RxJsonRpcMessage, ServiceRole, TxJsonRpcMessage},
    transport::{async_rw::JsonRpcMessageCodec, sink_stream::SinkStreamTransport},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{FramedRead, FramedWrite};

const APP_IDENTIFIER: &str = "dev.markra.app";

pub(crate) fn application_data_dir() -> Result<PathBuf, LocalIpcError> {
    dirs::data_dir()
        .map(|path| path.join(APP_IDENTIFIER))
        .ok_or_else(LocalIpcError::invalid_endpoint)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalIpcEndpoint {
    address: PathBuf,
}

impl LocalIpcEndpoint {
    pub(crate) fn for_app() -> Result<Self, LocalIpcError> {
        #[cfg(unix)]
        {
            return Ok(Self {
                address: application_data_dir()?
                    .join("mcp-runtime")
                    .join("qingyu.sock"),
            });
        }
        #[cfg(windows)]
        {
            let sid = current_user_sid_string().map_err(|_| LocalIpcError::invalid_endpoint())?;
            Ok(Self {
                address: PathBuf::from(windows_pipe_name_for_sid(&sid)),
            })
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(address: PathBuf) -> Self {
        Self { address }
    }

    pub(crate) fn health_label(&self) -> &'static str {
        "local-ipc"
    }

    pub(crate) async fn bind(&self) -> io::Result<LocalIpcListener> {
        bind(self).await
    }

    pub(crate) async fn connect(&self) -> io::Result<LocalIpcClientStream> {
        connect(self).await
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalIpcError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl LocalIpcError {
    fn invalid_endpoint() -> Self {
        Self {
            code: "mcp_ipc_endpoint_invalid",
            message: "QingYu MCP could not determine a private local IPC endpoint.",
        }
    }
}

impl fmt::Display for LocalIpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LocalIpcError {}

pub(crate) fn bounded_transport<Role, Stream>(
    stream: Stream,
    max_length: usize,
) -> SinkStreamTransport<
    FramedWrite<tokio::io::WriteHalf<Stream>, JsonRpcMessageCodec<TxJsonRpcMessage<Role>>>,
    impl futures::Stream<Item = RxJsonRpcMessage<Role>> + Send + Unpin,
>
where
    Role: ServiceRole,
    Stream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (read, write) = tokio::io::split(stream);
    let reader = FramedRead::new(
        read,
        JsonRpcMessageCodec::<RxJsonRpcMessage<Role>>::new_with_max_length(max_length),
    )
    .take_while(|result| future::ready(result.is_ok()))
    .filter_map(|result| future::ready(result.ok()));
    let writer = FramedWrite::new(
        write,
        JsonRpcMessageCodec::<TxJsonRpcMessage<Role>>::new_with_max_length(max_length),
    );
    SinkStreamTransport::new(writer, reader)
}

#[cfg(unix)]
pub(crate) type LocalIpcClientStream = tokio::net::UnixStream;

#[cfg(unix)]
pub(crate) type LocalIpcServerStream = tokio::net::UnixStream;

#[cfg(unix)]
pub(crate) struct LocalIpcListener {
    identity: (u64, u64),
    listener: tokio::net::UnixListener,
    socket_path: PathBuf,
}

#[cfg(unix)]
impl LocalIpcListener {
    pub(crate) async fn accept(&mut self) -> io::Result<LocalIpcServerStream> {
        self.listener.accept().await.map(|(stream, _)| stream)
    }
}

#[cfg(unix)]
impl Drop for LocalIpcListener {
    fn drop(&mut self) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        let owns_socket = std::fs::symlink_metadata(&self.socket_path)
            .map(|metadata| {
                metadata.file_type().is_socket()
                    && (metadata.dev(), metadata.ino()) == self.identity
            })
            .unwrap_or(false);
        if owns_socket {
            let _remove_result = std::fs::remove_file(&self.socket_path);
        }
    }
}

#[cfg(unix)]
async fn bind(endpoint: &LocalIpcEndpoint) -> io::Result<LocalIpcListener> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

    let socket_path = &endpoint.address;
    let parent = socket_path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing IPC parent"))?;
    std::fs::create_dir_all(parent)?;
    if let Ok(metadata) = std::fs::symlink_metadata(socket_path) {
        if !metadata.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "QingYu MCP IPC path is occupied by a non-socket file",
            ));
        }
        match tokio::net::UnixStream::connect(socket_path).await {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "QingYu MCP IPC endpoint is already active",
                ));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                let stale_metadata = std::fs::symlink_metadata(socket_path)?;
                if !stale_metadata.file_type().is_socket() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "QingYu MCP IPC path changed while checking a stale socket",
                    ));
                }
                std::fs::remove_file(socket_path)?;
            }
            Err(error) => return Err(error),
        }
    }
    let listener = tokio::net::UnixListener::bind(socket_path)?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;
    let metadata = std::fs::symlink_metadata(socket_path)?;
    Ok(LocalIpcListener {
        identity: (metadata.dev(), metadata.ino()),
        listener,
        socket_path: socket_path.clone(),
    })
}

#[cfg(unix)]
async fn connect(endpoint: &LocalIpcEndpoint) -> io::Result<LocalIpcClientStream> {
    tokio::net::UnixStream::connect(&endpoint.address).await
}

#[cfg(windows)]
pub(crate) type LocalIpcClientStream = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(windows)]
pub(crate) type LocalIpcServerStream = tokio::net::windows::named_pipe::NamedPipeServer;

#[cfg(any(windows, test))]
fn windows_pipe_name_for_sid(sid: &str) -> String {
    let user_component = sid
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .collect::<String>();
    format!(r"\\.\pipe\qingyu-mcp-{APP_IDENTIFIER}-{user_component}")
}

#[cfg(any(windows, test))]
fn windows_pipe_sddl_for_sid(sid: &str) -> String {
    format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})")
}

#[cfg(windows)]
fn current_user_sid_string() -> io::Result<String> {
    use std::{mem::size_of, ptr, slice};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
            TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut required = 0u32;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut required);
            if required == 0 {
                return Err(io::Error::last_os_error());
            }
            let word_count = (required as usize).div_ceil(size_of::<usize>());
            let mut storage = vec![0usize; word_count];
            if GetTokenInformation(
                token,
                TokenUser,
                storage.as_mut_ptr().cast(),
                required,
                &mut required,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let token_user = &*storage.as_ptr().cast::<TOKEN_USER>();
            let mut string_sid = ptr::null_mut();
            if ConvertSidToStringSidW(token_user.User.Sid, &mut string_sid) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut length = 0usize;
            while *string_sid.add(length) != 0 {
                length += 1;
            }
            let decoded = String::from_utf16(slice::from_raw_parts(string_sid, length))
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid user SID"));
            LocalFree(string_sid.cast());
            decoded
        })();
        CloseHandle(token);
        result
    }
}

#[cfg(windows)]
struct WindowsPipeSecurity {
    descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR,
}

#[cfg(windows)]
impl WindowsPipeSecurity {
    fn for_current_user() -> io::Result<Self> {
        use std::ptr;

        use windows_sys::Win32::Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            PSECURITY_DESCRIPTOR,
        };

        let sid = current_user_sid_string()?;
        let wide = windows_pipe_sddl_for_sid(&sid)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        unsafe {
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self { descriptor })
    }

    fn attributes(&mut self) -> windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>()
                as u32,
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: 0,
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsPipeSecurity {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.descriptor.cast());
        }
    }
}

#[cfg(windows)]
pub(crate) struct LocalIpcListener {
    pipe_name: String,
    pending: Option<LocalIpcServerStream>,
    first: bool,
}

#[cfg(windows)]
impl LocalIpcListener {
    fn create_instance(&mut self) -> io::Result<LocalIpcServerStream> {
        let mut options = tokio::net::windows::named_pipe::ServerOptions::new();
        options.reject_remote_clients(true);
        if self.first {
            options.first_pipe_instance(true);
            self.first = false;
        }
        let mut security = WindowsPipeSecurity::for_current_user()?;
        let mut attributes = security.attributes();
        unsafe {
            options.create_with_security_attributes_raw(
                &self.pipe_name,
                (&mut attributes as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES).cast(),
            )
        }
    }

    pub(crate) async fn accept(&mut self) -> io::Result<LocalIpcServerStream> {
        let server = match self.pending.take() {
            Some(server) => server,
            None => self.create_instance()?,
        };
        server.connect().await?;
        self.pending = Some(self.create_instance()?);
        Ok(server)
    }
}

#[cfg(windows)]
async fn bind(endpoint: &LocalIpcEndpoint) -> io::Result<LocalIpcListener> {
    let pipe_name = endpoint.address.to_string_lossy().into_owned();
    let mut listener = LocalIpcListener {
        pipe_name,
        pending: None,
        first: true,
    };
    listener.pending = Some(listener.create_instance()?);
    Ok(listener)
}

#[cfg(windows)]
async fn connect(endpoint: &LocalIpcEndpoint) -> io::Result<LocalIpcClientStream> {
    tokio::net::windows::named_pipe::ClientOptions::new()
        .open(endpoint.address.to_string_lossy().as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_pipe_identity_is_scoped_to_the_user_sid() {
        let first = windows_pipe_name_for_sid("S-1-5-21-1000");
        let second = windows_pipe_name_for_sid("S-1-5-21-2000");

        assert_ne!(first, second);
        assert_eq!(first, r"\\.\pipe\qingyu-mcp-dev.markra.app-S-1-5-21-1000");
    }

    #[test]
    fn windows_pipe_dacl_allows_only_system_and_the_current_user() {
        assert_eq!(
            windows_pipe_sddl_for_sid("S-1-5-21-1000"),
            "D:P(A;;GA;;;SY)(A;;GA;;;S-1-5-21-1000)"
        );
    }
}
