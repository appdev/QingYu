pub mod audit;
pub mod config;
pub mod confirmation;
pub mod error;
pub mod handles;
pub mod headless;
pub mod http;
pub mod kernel_port;
pub mod local_settings;
pub mod policy;
pub mod tools;
pub mod workspaces;

pub use audit::{AuditEntry, AuditEvent, AuditOutcome, AuditSink};
pub use config::{
    AuditPolicy, ConfirmationPolicy, DeletionPolicy, DryRunPolicy, McpConfig,
    McpClientConnection, McpConfigDocument, McpConfigError, McpHttpTransportConfig, McpLocalIpcTransportConfig, McpTransportConfig, McpConfigManager, McpPermissions,
    SyncAfterWritePolicy, SyncExecutionPolicy, ToolCapability,
};
pub use confirmation::{
    ConfirmationOutcome, ConfirmationPresenter, ConfirmationRequest, NoUiConfirmationPresenter,
};
pub use error::McpToolFailure;
pub use handles::HandleSigner;
pub use kernel_port::{McpKernelFailure, McpKernelFuture, McpKernelPort};
pub use kernel_port::DirectKernelMcpPort;
pub use local_settings::{McpLocalSettingsService, McpPolicyEventSink, NoopMcpPolicyEventSink, MCP_POLICY_CHANGED_EVENT};
pub use policy::PolicyEngine;
pub use tools::{McpServices, QingYuMcpHandler};
pub use workspaces::{SafeWorkspace, WorkspaceRegistry};

#[cfg(test)]
mod dependency_boundary_tests {
    #[test]
    fn mcp_core_has_no_tauri_dependency_names() {
        let sources = [
            include_str!("config.rs"),
            include_str!("local_settings.rs"),
            include_str!("audit.rs"),
            include_str!("policy.rs"),
            include_str!("handles.rs"),
            include_str!("workspaces.rs"),
            include_str!("confirmation.rs"),
        ];
        for source in sources {
            assert!(!source.contains("tauri"));
            assert!(!source.contains("desktop_runtime"));
            assert!(!source.contains("DesktopKernelRuntimeState"));
        }
    }
}
