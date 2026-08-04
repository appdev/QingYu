use std::path::PathBuf;

use crate::mcp::{ConfirmationPolicy, McpConfig};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessMcpEnvironment {
    pub workspace: Option<PathBuf>,
    pub token: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessMcpRuntimeConfig {
    pub mcp: McpConfig,
    pub bind: std::net::SocketAddr,
    pub path: String,
    pub token: Option<String>,
    pub allowed_hosts: Vec<String>,
    pub runtime_root: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeadlessMcpError {
    pub code: &'static str,
    pub message: &'static str,
}

pub fn validate_headless_mcp(
    config: &McpConfig,
    env: &HeadlessMcpEnvironment,
) -> Result<(), HeadlessMcpError> {
    if !config.enabled {
        return Ok(());
    }
    if env.workspace.is_none() {
        return Err(HeadlessMcpError {
            code: "headless_workspace_missing",
            message: "QingYu MCP requires QINGYU_WORKSPACE in headless mode.",
        });
    }
    if config.confirmation != ConfirmationPolicy::Never {
        return Err(HeadlessMcpError {
            code: "headless_confirmation_unsupported",
            message: "Headless QingYu MCP requires confirmation=never.",
        });
    }
    if config.transports.http.enabled
        && !is_loopback_bind(&config.transports.http.host)
        && env.token.as_deref().unwrap_or("").is_empty()
    {
        return Err(HeadlessMcpError {
            code: "http_auth_missing",
            message: "QingYu MCP HTTP requires QINGYU_MCP_TOKEN for non-loopback binds.",
        });
    }
    Ok(())
}

fn is_loopback_bind(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

pub fn load_headless_mcp_config_from_env(
) -> Result<Option<HeadlessMcpRuntimeConfig>, HeadlessMcpError> {
    let enabled = std::env::var("QINGYU_MCP_ENABLED").ok().as_deref() == Some("1");
    if !enabled {
        return Ok(None);
    }
    let host = std::env::var("QINGYU_MCP_HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = std::env::var("QINGYU_MCP_HTTP_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3211);
    let path = std::env::var("QINGYU_MCP_HTTP_PATH").unwrap_or_else(|_| "/mcp".to_owned());
    let token = std::env::var("QINGYU_MCP_TOKEN").ok().filter(|value| !value.is_empty());
    let bind = format!("{host}:{port}")
        .parse::<std::net::SocketAddr>()
        .map_err(|_| HeadlessMcpError {
            code: "http_bind_invalid",
            message: "QingYu MCP HTTP bind address is invalid.",
        })?;
    let mut mcp = McpConfig::default();
    mcp.enabled = true;
    mcp.confirmation = ConfirmationPolicy::Never;
    mcp.transports.local_ipc.enabled = false;
    mcp.transports.http.enabled = true;
    mcp.transports.http.host = host;
    mcp.transports.http.port = port;
    mcp.transports.http.path = path.clone();
    let env = HeadlessMcpEnvironment {
        workspace: Some(PathBuf::from("/data")),
        token: token.clone(),
    };
    validate_headless_mcp(&mcp, &env)?;
    Ok(Some(HeadlessMcpRuntimeConfig {
        mcp,
        bind,
        path,
        token,
        allowed_hosts: vec![
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
        ],
        runtime_root: PathBuf::from("/data/mcp-runtime"),
    }))
}

pub fn build_headless_handler(
    composition: std::sync::Arc<crate::server::runtime_composition::ServerRuntimeComposition>,
    config: &HeadlessMcpRuntimeConfig,
) -> Result<crate::mcp::QingYuMcpHandler, HeadlessMcpError> {
    let mut signing_key = [0_u8; 32];
    getrandom::fill(&mut signing_key).map_err(|_| HeadlessMcpError {
        code: "signing_key_unavailable",
        message: "QingYu MCP could not generate a signing key.",
    })?;
    let signer = crate::mcp::HandleSigner::new(signing_key);
    let handles = std::sync::Arc::new(signer.clone());
    let policy = std::sync::Arc::new(crate::mcp::PolicyEngine::new(
        signer.derive_key(b"QingYu MCP operation previews v1"),
    ));
    let config_manager = std::sync::Arc::new(
        crate::mcp::McpConfigManager::from_static_config(config.mcp.clone()).map_err(|_| {
            HeadlessMcpError {
                code: "headless_config_invalid",
                message: "QingYu MCP headless configuration is invalid.",
            }
        })?,
    );
    let audit = std::sync::Arc::new(crate::mcp::AuditSink::new(
        &config.runtime_root,
        config.mcp.audit.clone(),
    ));
    let services = crate::mcp::McpServices {
        config: config_manager,
        workspaces: std::sync::Arc::new(crate::mcp::WorkspaceRegistry::new(Vec::new())),
        handles,
        kernel: std::sync::Arc::new(crate::mcp::kernel_port::DirectKernelMcpPort::new(composition)),
        policy,
        audit,
    };
    Ok(crate::mcp::QingYuMcpHandler::new(
        services,
        std::sync::Arc::new(crate::mcp::NoUiConfirmationPresenter),
    ))
}

#[cfg(test)]
mod tests {
    use super::{validate_headless_mcp, HeadlessMcpEnvironment};
    use crate::mcp::{ConfirmationPolicy, McpConfig};

    #[test]
    fn rejects_non_loopback_http_without_token() {
        let mut config = McpConfig::default();
        config.enabled = true;
        config.transports.http.enabled = true;
        config.transports.http.host = "0.0.0.0".to_owned();
        let env = HeadlessMcpEnvironment {
            workspace: Some("/data/workspace".into()),
            token: None,
        };
        let error = validate_headless_mcp(&config, &env).expect_err("missing token should fail");
        assert_eq!(error.code, "http_auth_missing");
    }

    #[test]
    fn rejects_ui_confirmation_policy() {
        let mut config = McpConfig::default();
        config.enabled = true;
        config.confirmation = ConfirmationPolicy::AllWrites;
        let env = HeadlessMcpEnvironment {
            workspace: Some("/data/workspace".into()),
            token: Some("secret".into()),
        };
        let error = validate_headless_mcp(&config, &env).expect_err("ui confirmation should fail");
        assert_eq!(error.code, "headless_confirmation_unsupported");
    }
}
