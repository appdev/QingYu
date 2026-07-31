use std::{future::Future, pin::Pin, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(120);
const CONFIRMATION_PARENT_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const CONFIRMATION_PARENT_POLL_INTERVAL: Duration = Duration::from_millis(25);

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
    presentation_lock: tokio::sync::Mutex<()>,
}

impl<R: tauri::Runtime> TauriConfirmationPresenter<R> {
    pub(crate) fn new(app: tauri::AppHandle<R>) -> Self {
        Self {
            app,
            presentation_lock: tokio::sync::Mutex::new(()),
        }
    }
}

impl<R: tauri::Runtime> ConfirmationPresenter for TauriConfirmationPresenter<R> {
    fn present<'a>(
        &'a self,
        request: ConfirmationRequest,
    ) -> Pin<Box<dyn Future<Output = ConfirmationOutcome> + Send + 'a>> {
        Box::pin(async move {
            let deadline = tokio::time::Instant::now() + CONFIRMATION_TIMEOUT;
            let Ok(_presentation_guard) =
                tokio::time::timeout_at(deadline, self.presentation_lock.lock()).await
            else {
                return ConfirmationOutcome::TimedOut;
            };
            let app = self.app.clone();
            let dialog_app = self.app.clone();
            let message = confirmation_message(&request);

            present_with_parent(
                deadline,
                move || async move { acquire_confirmation_parent(&app).await },
                move |parent, sender| {
                    let parent_guard = parent.clone();
                    dialog_app
                        .dialog()
                        .message(message)
                        .title("QingYu MCP")
                        .buttons(MessageDialogButtons::OkCancelCustom(
                            "Allow".to_string(),
                            "Cancel".to_string(),
                        ))
                        .parent(&parent)
                        .show(move |allowed| {
                            let _parent_guard = parent_guard;
                            let _ = sender.send(allowed);
                        });
                },
            )
            .await
        })
    }
}

async fn present_with_parent<P, Acquire, AcquireFuture, Show>(
    deadline: tokio::time::Instant,
    acquire_parent: Acquire,
    show: Show,
) -> ConfirmationOutcome
where
    P: Send + 'static,
    Acquire: FnOnce() -> AcquireFuture,
    AcquireFuture: Future<Output = Option<P>> + Send,
    Show: FnOnce(P, tokio::sync::oneshot::Sender<bool>),
{
    let parent = match tokio::time::timeout_at(deadline, acquire_parent()).await {
        Ok(Some(parent)) => parent,
        Ok(None) | Err(_) => return ConfirmationOutcome::TimedOut,
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    show(parent, sender);

    match tokio::time::timeout_at(deadline, receiver).await {
        Ok(Ok(true)) => ConfirmationOutcome::Allowed,
        Ok(Ok(false)) | Ok(Err(_)) => ConfirmationOutcome::Rejected,
        Err(_) => ConfirmationOutcome::TimedOut,
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

fn confirmation_parent_window_label<'a>(
    labels: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    labels
        .into_iter()
        .filter(|label| {
            crate::windows::is_editor_window_label(label)
                || crate::windows::is_settings_window_label(label)
        })
        .min_by_key(|label| {
            let priority = if *label == "main" {
                0
            } else if crate::windows::is_editor_window_label(label) {
                1
            } else {
                2
            };
            (priority, *label)
        })
}

fn confirmation_parent_window<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Option<tauri::WebviewWindow<R>> {
    let windows = app.webview_windows();
    let label = confirmation_parent_window_label(windows.keys().map(String::as_str))?;
    windows.get(label).cloned()
}

fn confirmation_parent_has_native_handle<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> bool {
    #[cfg(target_os = "macos")]
    return window.ns_window().is_ok_and(|handle| !handle.is_null());
    #[cfg(target_os = "windows")]
    return window.hwnd().is_ok();
    #[cfg(target_os = "linux")]
    return window.gtk_window().is_ok();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    false
}

async fn acquire_confirmation_parent<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Option<tauri::WebviewWindow<R>> {
    if crate::desktop_runtime::promote_normal_ui_for_confirmation(app).is_err() {
        return None;
    }
    let deadline = tokio::time::Instant::now() + CONFIRMATION_PARENT_WAIT_TIMEOUT;

    loop {
        if let Some(window) = confirmation_parent_window(app) {
            if confirmation_parent_has_native_handle(&window) && window.show().is_ok() {
                let _ = window.set_focus();
                return Some(window);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(CONFIRMATION_PARENT_POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{confirmation_parent_window_label, present_with_parent, ConfirmationOutcome};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    use std::time::Duration;

    #[test]
    fn confirmation_parent_prefers_main_then_editor_then_settings() {
        assert_eq!(
            confirmation_parent_window_label([
                "markra-settings",
                "markra-editor-20",
                "main",
                "markra-editor-3",
            ]),
            Some("main")
        );
        assert_eq!(
            confirmation_parent_window_label([
                "markra-settings",
                "markra-editor-20",
                "markra-editor-3",
            ]),
            Some("markra-editor-20")
        );
        assert_eq!(
            confirmation_parent_window_label(["markra-settings", "unrelated"]),
            Some("markra-settings")
        );
    }

    #[test]
    fn confirmation_parent_rejects_unrelated_windows() {
        assert_eq!(
            confirmation_parent_window_label(["diagnostics", "about"]),
            None
        );
    }

    #[tokio::test]
    async fn selected_parent_is_forwarded_to_the_dialog_launcher() {
        let presented_parent = Arc::new(Mutex::new(None));
        let captured_parent = Arc::clone(&presented_parent);
        let outcome = present_with_parent(
            tokio::time::Instant::now() + Duration::from_secs(1),
            || async { Some("main") },
            move |parent, sender| {
                *captured_parent.lock().expect("parent capture") = Some(parent);
                sender.send(true).expect("confirmation receiver");
            },
        )
        .await;

        assert_eq!(outcome, ConfirmationOutcome::Allowed);
        assert_eq!(
            *presented_parent.lock().expect("presented parent"),
            Some("main")
        );
    }

    #[tokio::test]
    async fn missing_parent_fails_closed_without_launching_a_dialog() {
        let launched = Arc::new(AtomicBool::new(false));
        let captured_launch = Arc::clone(&launched);
        let outcome = present_with_parent(
            tokio::time::Instant::now() + Duration::from_secs(1),
            || async { None::<&'static str> },
            move |_parent, _sender| {
                captured_launch.store(true, Ordering::SeqCst);
            },
        )
        .await;

        assert_eq!(outcome, ConfirmationOutcome::TimedOut);
        assert!(!launched.load(Ordering::SeqCst));
    }
}
