use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    Router,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;

use crate::mcp::QingYuMcpHandler;

#[derive(Clone)]
pub struct HttpMcpServerConfig {
    pub bind: SocketAddr,
    pub path: String,
    pub bearer_token: Option<String>,
    pub allowed_hosts: Vec<String>,
    pub handler: QingYuMcpHandler,
    pub shutdown: CancellationToken,
}

pub struct HttpMcpServerHandle {
    local_addr: SocketAddr,
}

impl HttpMcpServerHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

#[derive(Debug)]
pub enum HttpMcpError {
    Bind(std::io::Error),
    LocalAddr(std::io::Error),
}

impl std::fmt::Display for HttpMcpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bind(error) => write!(f, "MCP HTTP bind error: {error}"),
            Self::LocalAddr(error) => write!(f, "MCP HTTP local addr error: {error}"),
        }
    }
}

#[derive(Clone)]
struct AuthState {
    bearer_token: Arc<str>,
}

pub async fn serve_http_mcp(
    config: HttpMcpServerConfig,
) -> Result<HttpMcpServerHandle, HttpMcpError> {
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(HttpMcpError::Bind)?;
    let actual = listener.local_addr().map_err(HttpMcpError::LocalAddr)?;
    let mut allowed_hosts = config.allowed_hosts.clone();
    allowed_hosts.push(actual.to_string());
    let service: StreamableHttpService<QingYuMcpHandler, LocalSessionManager> =
        StreamableHttpService::new(
            {
                let handler = config.handler.clone();
                move || Ok(handler.clone())
            },
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_cancellation_token(config.shutdown.clone())
                .with_allowed_hosts(allowed_hosts),
        );
    let app = Router::new().nest_service(&config.path, service);
    let app = match config.bearer_token {
        Some(token) => app.layer(middleware::from_fn_with_state(
            AuthState {
                bearer_token: Arc::from(token),
            },
            bearer_auth,
        )),
        None => app,
    };
    let shutdown = config.shutdown.clone();
    tokio::spawn(async move {
        let _serve_result = axum::serve(listener, app)
            .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
            .await;
    });
    Ok(HttpMcpServerHandle { local_addr: actual })
}

async fn bearer_auth(
    State(state): State<AuthState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = format!("Bearer {}", state.bearer_token);
    let authorized = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected);
    if !authorized {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}
