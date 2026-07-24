use std::{future::Future, pin::Pin, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConfirmationOutcome {
    Allowed,
    Rejected,
    TimedOut,
}

#[derive(Clone, Debug)]
pub(crate) struct ConfirmationRequest {
    pub(crate) tool: String,
    pub(crate) workspace_display_name: Option<String>,
    pub(crate) logical_target: Option<String>,
    pub(crate) expected_revision: Option<String>,
    pub(crate) effect: String,
}

pub(crate) trait ConfirmationPresenter: Send + Sync {
    fn present<'a>(
        &'a self,
        request: ConfirmationRequest,
    ) -> Pin<Box<dyn Future<Output = ConfirmationOutcome> + Send + 'a>>;
}

pub(crate) struct TauriConfirmationPresenter<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriConfirmationPresenter<R> {
    pub(crate) fn new(app: tauri::AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: tauri::Runtime> ConfirmationPresenter for TauriConfirmationPresenter<R> {
    fn present<'a>(
        &'a self,
        request: ConfirmationRequest,
    ) -> Pin<Box<dyn Future<Output = ConfirmationOutcome> + Send + 'a>> {
        Box::pin(async move {
            focus_or_create_qingyu_window(&self.app);
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.app
                .dialog()
                .message(confirmation_message(&request))
                .title("QingYu MCP")
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Allow".to_string(),
                    "Cancel".to_string(),
                ))
                .show(move |allowed| {
                    let _ = sender.send(allowed);
                });

            match tokio::time::timeout(Duration::from_secs(120), receiver).await {
                Ok(Ok(true)) => ConfirmationOutcome::Allowed,
                Ok(Ok(false)) | Ok(Err(_)) => ConfirmationOutcome::Rejected,
                Err(_) => ConfirmationOutcome::TimedOut,
            }
        })
    }
}

fn confirmation_message(request: &ConfirmationRequest) -> String {
    let workspace = request
        .workspace_display_name
        .as_deref()
        .unwrap_or("QingYu");
    let target = request
        .logical_target
        .as_deref()
        .unwrap_or("application settings");
    let revision = request
        .expected_revision
        .as_deref()
        .unwrap_or("not applicable");
    format!(
        "Tool: {}\nWorkspace: {workspace}\nTarget: {target}\nRevision: {revision}\nEffect: {}",
        request.tool, request.effect
    )
}

fn focus_or_create_qingyu_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.webview_windows().values().find(|window| {
        crate::windows::is_editor_window_label(window.label())
            || crate::windows::is_settings_window_label(window.label())
    }) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    crate::windows::spawn_restorable_editor_window(app.clone());
}
