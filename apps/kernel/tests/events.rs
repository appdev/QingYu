use std::{net::SocketAddr, sync::Arc, time::Duration};

use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        DomainEvent, EventSequence, FrameErrorCode, GapReason, ProtocolVersion, ReloadScope,
        ResourceRefDto, Revision, ServerFrame, WorkspaceDto, WorkspaceGeneration, WorkspaceId,
        WorkspaceReadiness,
    },
    events::{ConnectionSequence, ConnectionSequenceStep, EventPublication},
    paths::KernelPaths,
    ports::KernelPorts,
    runtime::KernelRuntime,
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::timeout,
};

const ORIGIN: &str = "tauri://localhost";
const WS_AUTH_CLOSE: u16 = 4001;
const WS_RELOAD_CLOSE: u16 = 4009;

struct LiveEventsApi {
    runtime: Arc<KernelRuntime>,
    credential: String,
    address: SocketAddr,
    server: JoinHandle<()>,
    _root: tempfile::TempDir,
}

impl LiveEventsApi {
    async fn start() -> Self {
        let root = tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let app_data = root.path().join("app-data");
        let cache = root.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();

        let config = KernelConfig::generate().unwrap();
        let credential = config.native_launch_credential().expose_secret().to_owned();
        let runtime = KernelRuntime::activate(
            config,
            KernelPaths::desktop(&workspace, &app_data, &cache).unwrap(),
            KernelPorts::unavailable(),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let policy = TransportPolicy::loopback(&address.to_string(), ORIGIN).unwrap();
        let router = build_router(runtime.clone(), policy);
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        Self {
            runtime,
            credential,
            address,
            server,
            _root: root,
        }
    }

    async fn connect(&self) -> RawWebSocket {
        RawWebSocket::connect(self.address, ORIGIN).await
    }

    fn authenticate_frame(&self) -> Value {
        json!({
            "type": "authenticate",
            "protocolVersion": 1,
            "credential": self.credential,
        })
    }
}

impl Drop for LiveEventsApi {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_broker_subscribes_only_after_auth_and_starts_with_ready_then_sequence_one() {
    let api = LiveEventsApi::start().await;
    let mut socket = api.connect().await;

    assert_eq!(api.runtime.event_broker().subscriber_count(), 0);
    api.runtime
        .event_broker()
        .publish(&workspace_publication("pre-auth"))
        .unwrap();
    assert_eq!(api.runtime.event_broker().subscriber_count(), 0);

    socket.send_json(&api.authenticate_frame()).await;
    let ready = socket.read_server_frame().await;
    match ready {
        ServerFrame::Ready {
            protocol_version,
            connection_id: _,
            instance_id,
            sequence,
            snapshot_required,
        } => {
            assert_eq!(protocol_version, ProtocolVersion::new(1).unwrap());
            assert_eq!(instance_id, api.runtime.instance_id());
            assert_eq!(sequence.get(), 0);
            assert!(snapshot_required.get());
        }
        other => panic!("expected ready as the first server frame, got {other:?}"),
    }
    assert_eq!(api.runtime.event_broker().subscriber_count(), 1);

    api.runtime
        .event_broker()
        .publish(&workspace_publication("post-auth"))
        .unwrap();
    match socket.read_server_frame().await {
        ServerFrame::Event {
            sequence,
            revision,
            event,
            ..
        } => {
            assert_eq!(sequence.get(), 1);
            assert_eq!(revision.as_str(), "post-auth");
            assert!(matches!(*event, DomainEvent::WorkspaceChanged { .. }));
        }
        other => panic!("expected the first post-auth publication, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_first_frames_emit_one_safe_error_then_close_4001() {
    let api = LiveEventsApi::start().await;
    let cases = [
        (
            json!({
                "type": "authenticate",
                "protocolVersion": 1,
                "credential": "not-the-launch-credential",
            }),
            FrameErrorCode::Unauthorized,
        ),
        (
            json!({
                "type": "authenticate",
                "protocolVersion": 2,
                "credential": api.credential,
            }),
            FrameErrorCode::UnsupportedVersion,
        ),
        (
            json!({"type": "not-authenticate"}),
            FrameErrorCode::InvalidFrame,
        ),
    ];

    for (first_frame, expected_code) in cases {
        let mut socket = api.connect().await;
        socket.send_json(&first_frame).await;
        assert_safe_error_then_close(&mut socket, expected_code, WS_AUTH_CLOSE).await;
    }
    let mut malformed = api.connect().await;
    malformed.send_text("{").await;
    assert_safe_error_then_close(&mut malformed, FrameErrorCode::InvalidFrame, WS_AUTH_CLOSE).await;
    assert_eq!(api.runtime.event_broker().subscriber_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn missing_authentication_hits_the_three_second_deadline_and_closes_4001() {
    let api = LiveEventsApi::start().await;
    let mut socket = api.connect().await;

    timeout(
        Duration::from_millis(3_750),
        assert_safe_error_then_close(&mut socket, FrameErrorCode::InvalidFrame, WS_AUTH_CLOSE),
    )
    .await
    .expect("authentication deadline must be enforced after three seconds");
    assert_eq!(api.runtime.event_broker().subscriber_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn broadcast_lag_emits_one_buffer_overflow_gap_then_closes_4009() {
    let api = LiveEventsApi::start().await;
    let mut socket = api.connect().await;
    socket.send_json(&api.authenticate_frame()).await;
    assert!(matches!(
        socket.read_server_frame().await,
        ServerFrame::Ready { .. }
    ));

    // No await occurs while filling the bounded broker. On this current-thread
    // runtime the authenticated receiver cannot drain until after it has lagged.
    for index in 0..=api.runtime.event_broker().capacity() {
        api.runtime
            .event_broker()
            .publish(&workspace_publication(&format!("lag-{index}")))
            .unwrap();
    }

    match socket.read_server_frame().await {
        ServerFrame::Gap {
            sequence,
            reason,
            reload_scopes,
            ..
        } => {
            assert_eq!(sequence.get(), 1);
            assert_eq!(reason, GapReason::BufferOverflow);
            assert_eq!(
                reload_scopes,
                vec![
                    ReloadScope::Workspace,
                    ReloadScope::Documents,
                    ReloadScope::Settings,
                    ReloadScope::AppConfig,
                    ReloadScope::SyncConfig,
                    ReloadScope::SyncStatus,
                ]
            );
        }
        other => panic!("expected one terminal buffer-overflow gap, got {other:?}"),
    }
    assert_eq!(
        socket.read_message().await,
        WsMessage::Close(WS_RELOAD_CLOSE)
    );
}

#[test]
fn sequence_exhaustion_reserves_one_safe_sequence_for_a_single_terminal_gap() {
    let mut sequence =
        ConnectionSequence::with_limit(EventSequence::new(2).expect("small test limit is safe"));

    assert_eq!(
        sequence.next(),
        Some(ConnectionSequenceStep::Event(
            EventSequence::new(1).unwrap()
        ))
    );
    assert_eq!(
        sequence.next(),
        Some(ConnectionSequenceStep::ExhaustedGap(
            EventSequence::new(2).unwrap()
        ))
    );
    assert_eq!(sequence.next(), None, "only one terminal gap is emitted");
}

async fn assert_safe_error_then_close(
    socket: &mut RawWebSocket,
    expected_code: FrameErrorCode,
    expected_close: u16,
) {
    match socket.read_server_frame().await {
        ServerFrame::Error {
            protocol_version,
            code,
            message,
        } => {
            assert_eq!(protocol_version, ProtocolVersion::new(1).unwrap());
            assert_eq!(code, expected_code);
            assert!(!message.is_empty());
            assert!(!message.contains("credential"));
            assert!(!message.contains("not-the-launch-credential"));
        }
        other => panic!("expected a safe authentication error, got {other:?}"),
    }
    assert_eq!(
        socket.read_message().await,
        WsMessage::Close(expected_close)
    );
}

fn workspace_publication(revision: &str) -> EventPublication {
    let workspace_id = WorkspaceId::new(uuid::Uuid::from_u128(41));
    let revision = Revision::parse(revision).unwrap();
    EventPublication {
        resource: ResourceRefDto::Workspace { id: workspace_id },
        revision: revision.clone(),
        event: DomainEvent::WorkspaceChanged {
            workspace: WorkspaceDto {
                id: workspace_id,
                generation: WorkspaceGeneration::parse("generation-events").unwrap(),
                display_name: "Events fixture".to_owned(),
                readiness: WorkspaceReadiness::Ready,
                revision,
            },
        },
    }
}

#[derive(Debug, Eq, PartialEq)]
enum WsMessage {
    Text(String),
    Close(u16),
}

struct RawWebSocket {
    stream: TcpStream,
}

impl RawWebSocket {
    async fn connect(address: SocketAddr, origin: &str) -> Self {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!(
            "GET /api/v1/events HTTP/1.1\r\n\
             Host: {address}\r\n\
             Origin: {origin}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: AAECAwQFBgcICQoLDA0ODw==\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();

        let mut response = Vec::new();
        let mut byte = [0_u8; 1];
        while !response.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            response.push(byte[0]);
            assert!(response.len() < 16 * 1024, "upgrade response is bounded");
        }
        let response = String::from_utf8(response).unwrap();
        assert!(
            response.starts_with("HTTP/1.1 101 "),
            "websocket upgrade failed: {response}"
        );
        Self { stream }
    }

    async fn send_json(&mut self, value: &Value) {
        self.send_text(&serde_json::to_string(value).unwrap()).await;
    }

    async fn send_text(&mut self, text: &str) {
        let payload = text.as_bytes();
        let mut frame = vec![0x81];
        match payload.len() {
            length if length < 126 => frame.push(0x80 | length as u8),
            length if u16::try_from(length).is_ok() => {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&(length as u16).to_be_bytes());
            }
            length => {
                frame.push(0x80 | 127);
                frame.extend_from_slice(&(length as u64).to_be_bytes());
            }
        }
        let mask = [0x19, 0x7a, 0xc3, 0x4d];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % mask.len()]),
        );
        self.stream.write_all(&frame).await.unwrap();
    }

    async fn read_server_frame(&mut self) -> ServerFrame {
        match self.read_message().await {
            WsMessage::Text(text) => serde_json::from_str(&text).unwrap(),
            WsMessage::Close(code) => panic!("expected a server frame before close {code}"),
        }
    }

    async fn read_message(&mut self) -> WsMessage {
        loop {
            let mut header = [0_u8; 2];
            self.stream.read_exact(&mut header).await.unwrap();
            assert_ne!(header[0] & 0x80, 0, "test server frames must not fragment");
            assert_eq!(header[1] & 0x80, 0, "server frames must not be masked");
            let length = match header[1] & 0x7f {
                length @ 0..=125 => u64::from(length),
                126 => {
                    let mut bytes = [0_u8; 2];
                    self.stream.read_exact(&mut bytes).await.unwrap();
                    u64::from(u16::from_be_bytes(bytes))
                }
                127 => {
                    let mut bytes = [0_u8; 8];
                    self.stream.read_exact(&mut bytes).await.unwrap();
                    u64::from_be_bytes(bytes)
                }
                _ => unreachable!(),
            };
            let length = usize::try_from(length).expect("test frame length fits usize");
            assert!(length <= 20 * 1024 * 1024, "test frame is bounded");
            let mut payload = vec![0_u8; length];
            self.stream.read_exact(&mut payload).await.unwrap();
            match header[0] & 0x0f {
                0x1 => return WsMessage::Text(String::from_utf8(payload).unwrap()),
                0x8 => {
                    let code = payload
                        .get(..2)
                        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
                        .unwrap_or(1005);
                    return WsMessage::Close(code);
                }
                0x9 => self.send_control(0xA, &payload).await,
                opcode => panic!("unexpected server websocket opcode {opcode:#x}"),
            }
        }
    }

    async fn send_control(&mut self, opcode: u8, payload: &[u8]) {
        assert!(payload.len() <= 125);
        let mask = [0x52, 0x0b, 0xa6, 0xd1];
        let mut frame = vec![0x80 | opcode, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % mask.len()]),
        );
        self.stream.write_all(&frame).await.unwrap();
    }
}
