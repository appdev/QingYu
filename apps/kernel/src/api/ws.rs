use std::{sync::Arc, time::Duration};

use axum::{
    extract::{
        ws::{
            rejection::WebSocketUpgradeRejection, CloseFrame, Message, WebSocket, WebSocketUpgrade,
        },
        Extension, State,
    },
    response::Response,
};
use serde::Deserialize;
use tokio::time::{timeout, Instant};
use uuid::Uuid;

use crate::{
    contract::{
        AuthenticateFrame, ConnectionId, ErrorCode, FrameErrorCode, GapReason, ProtocolVersion,
        ReadySequence, ReloadScope, ServerFrame, SnapshotRequired,
    },
    events::{ConnectionSequence, ConnectionSequenceStep, EventReceiveError},
    runtime::KernelRuntime,
};

use super::{api_error, auth::AuthenticatedBrowserSession, runtime, ApiState, ServerApiHost};

const AUTHENTICATION_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_AUTHENTICATION_FRAME_BYTES: usize = 64 * 1024;
const AUTHENTICATION_CLOSE_CODE: u16 = 4001;
const RELOAD_CLOSE_CODE: u16 = 4009;

pub(crate) async fn upgrade(
    State(state): State<ApiState>,
    browser_session: Option<Extension<AuthenticatedBrowserSession>>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Ok(upgrade) = upgrade else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let runtime = runtime(&state).clone();
    let server = state.server.clone();
    let browser_session = browser_session.map(|Extension(session)| session);
    upgrade
        .max_message_size(MAX_AUTHENTICATION_FRAME_BYTES)
        .max_frame_size(MAX_AUTHENTICATION_FRAME_BYTES)
        .on_upgrade(move |socket| serve_connection(socket, runtime, server, browser_session))
}

async fn serve_connection(
    mut socket: WebSocket,
    runtime: Arc<KernelRuntime>,
    server: Option<ServerApiHost>,
    mut browser_session: Option<AuthenticatedBrowserSession>,
) {
    if browser_session.is_none() {
        if let Err(code) = authenticate(&mut socket, &runtime).await {
            send_authentication_error_and_close(&mut socket, code).await;
            return;
        }
    }

    let mut subscription = runtime.event_broker().subscribe();
    let connection_id = ConnectionId::new(Uuid::new_v4());
    let ready = ServerFrame::Ready {
        protocol_version: ProtocolVersion::new(1).expect("v1 is supported"),
        connection_id,
        instance_id: runtime.instance_id(),
        sequence: ReadySequence::new(0).expect("ready starts at zero"),
        snapshot_required: SnapshotRequired::required(),
    };
    if !send_frame(&mut socket, &ready).await {
        return;
    }

    let mut sequence = ConnectionSequence::new();
    let initial_validation_delay = browser_session
        .as_ref()
        .zip(server.as_ref())
        .map_or(Duration::from_secs(3600), |(session, host)| {
            browser_validation_delay(session, host)
        });
    let browser_validation = tokio::time::sleep(initial_validation_delay);
    tokio::pin!(browser_validation);
    loop {
        tokio::select! {
            () = &mut browser_validation, if browser_session.is_some() => {
                let (Some(host), Some(session)) = (server.as_ref(), browser_session.take()) else {
                    return;
                };
                match host
                    .authorize_browser_session(
                        session.credential.clone(),
                        None,
                        crate::server::RequestIntent::ReadOnly,
                    )
                    .await
                {
                    Ok(updated) => {
                        let delay = browser_validation_delay(&updated, host);
                        browser_session = Some(updated);
                        browser_validation.as_mut().reset(Instant::now() + delay);
                    }
                    Err(_error) => {
                        send_authentication_error_and_close(
                            &mut socket,
                            FrameErrorCode::Unauthorized,
                        )
                        .await;
                        return;
                    }
                }
            }
            publication = subscription.recv() => {
                match publication {
                    Ok(publication) => match sequence.next() {
                        Some(ConnectionSequenceStep::Event(event_sequence)) => {
                            let frame = ServerFrame::Event {
                                protocol_version: ProtocolVersion::new(1).expect("v1 is supported"),
                                connection_id,
                                sequence: event_sequence,
                                resource: publication.resource,
                                revision: publication.revision,
                                event: Box::new(publication.event),
                            };
                            if !send_frame(&mut socket, &frame).await {
                                return;
                            }
                        }
                        Some(ConnectionSequenceStep::ExhaustedGap(gap_sequence)) => {
                            send_gap_and_close(
                                &mut socket,
                                connection_id,
                                gap_sequence,
                                GapReason::SequenceExhausted,
                            )
                            .await;
                            return;
                        }
                        None => return,
                    },
                    Err(EventReceiveError::Lagged) => {
                        if let Some(gap_sequence) = sequence.terminal_gap() {
                            send_gap_and_close(
                                &mut socket,
                                connection_id,
                                gap_sequence,
                                GapReason::BufferOverflow,
                            )
                            .await;
                        }
                        return;
                    }
                    Err(EventReceiveError::Closed) => return,
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            return;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Text(_) | Message::Binary(_))) => {
                        send_authentication_error_and_close(
                            &mut socket,
                            FrameErrorCode::InvalidFrame,
                        )
                        .await;
                        return;
                    }
                }
            }
        }
    }
}

fn browser_validation_delay(
    session: &AuthenticatedBrowserSession,
    host: &ServerApiHost,
) -> Duration {
    session
        .expires_at
        .saturating_sub(host.now())
        .min(Duration::from_secs(1))
        .max(Duration::from_millis(1))
}

async fn authenticate(
    socket: &mut WebSocket,
    runtime: &KernelRuntime,
) -> Result<(), FrameErrorCode> {
    let message = timeout(AUTHENTICATION_TIMEOUT, socket.recv())
        .await
        .map_err(|_| FrameErrorCode::InvalidFrame)?
        .ok_or(FrameErrorCode::InvalidFrame)?
        .map_err(|_| FrameErrorCode::InvalidFrame)?;
    let Message::Text(text) = message else {
        return Err(FrameErrorCode::InvalidFrame);
    };
    if text.len() > MAX_AUTHENTICATION_FRAME_BYTES {
        return Err(FrameErrorCode::InvalidFrame);
    }

    let probe: AuthenticationProbe =
        serde_json::from_str(text.as_str()).map_err(|_| FrameErrorCode::InvalidFrame)?;
    if probe.frame_type != "authenticate" {
        return Err(FrameErrorCode::InvalidFrame);
    }
    if probe.protocol_version != 1 {
        return Err(FrameErrorCode::UnsupportedVersion);
    }
    let frame: AuthenticateFrame =
        serde_json::from_str(text.as_str()).map_err(|_| FrameErrorCode::InvalidFrame)?;
    if !runtime.matches_native_launch_credential(&frame.credential) {
        return Err(FrameErrorCode::Unauthorized);
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticationProbe {
    #[serde(rename = "type")]
    frame_type: String,
    protocol_version: u64,
}

async fn send_authentication_error_and_close(socket: &mut WebSocket, code: FrameErrorCode) {
    let message = match code {
        FrameErrorCode::Unauthorized => "Authentication failed.",
        FrameErrorCode::InvalidFrame => "The first frame is invalid.",
        FrameErrorCode::UnsupportedVersion => "The protocol version is unsupported.",
    };
    let frame = ServerFrame::Error {
        protocol_version: ProtocolVersion::new(1).expect("v1 is supported"),
        code,
        message: message.to_owned(),
    };
    let _sent = send_frame(socket, &frame).await;
    close(socket, AUTHENTICATION_CLOSE_CODE, "authentication failed").await;
}

async fn send_gap_and_close(
    socket: &mut WebSocket,
    connection_id: ConnectionId,
    sequence: crate::contract::EventSequence,
    reason: GapReason,
) {
    let frame = ServerFrame::Gap {
        protocol_version: ProtocolVersion::new(1).expect("v1 is supported"),
        connection_id,
        sequence,
        reason,
        reload_scopes: vec![
            ReloadScope::Workspace,
            ReloadScope::Documents,
            ReloadScope::Settings,
            ReloadScope::SyncConfig,
            ReloadScope::SyncStatus,
        ],
    };
    let _sent = send_frame(socket, &frame).await;
    close(socket, RELOAD_CLOSE_CODE, "snapshot reload required").await;
}

async fn send_frame(socket: &mut WebSocket, frame: &ServerFrame) -> bool {
    let Ok(serialized) = serde_json::to_string(frame) else {
        return false;
    };
    socket.send(Message::Text(serialized.into())).await.is_ok()
}

async fn close(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let _closed = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })))
        .await;
}
