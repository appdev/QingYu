use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(120);
const CONFIRMATION_PARENT_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const CONFIRMATION_PARENT_POLL_INTERVAL: Duration = Duration::from_millis(25);
const CONFIRMATION_NATIVE_HANDLE_CHECK_TIMEOUT: Duration = Duration::from_secs(1);

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
    presentation_lock: Arc<tokio::sync::Mutex<()>>,
}

impl<R: tauri::Runtime> TauriConfirmationPresenter<R> {
    pub(crate) fn new(app: tauri::AppHandle<R>) -> Self {
        Self {
            app,
            presentation_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

impl<R: tauri::Runtime> ConfirmationPresenter for TauriConfirmationPresenter<R> {
    fn present<'a>(
        &'a self,
        request: ConfirmationRequest,
    ) -> Pin<Box<dyn Future<Output = ConfirmationOutcome> + Send + 'a>> {
        let app = self.app.clone();
        let presentation_lock = Arc::clone(&self.presentation_lock);
        Box::pin(async move {
            await_confirmation_request(async move {
                let deadline = tokio::time::Instant::now() + CONFIRMATION_TIMEOUT;
                let Ok(presentation_guard) =
                    tokio::time::timeout_at(deadline, presentation_lock.lock_owned()).await
                else {
                    return ConfirmationOutcome::TimedOut;
                };
                let parent_app = app.clone();
                let dialog_app = app;
                let message = confirmation_message(&request);

                present_with_parent(
                    deadline,
                    move || async move { acquire_confirmation_parent(&parent_app).await },
                    presentation_guard,
                    move |parent, sender, presentation_guard| {
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
                                let _presentation_guard = presentation_guard;
                                let _ = sender.send(allowed);
                            });
                    },
                )
                .await
            })
            .await
        })
    }
}

async fn await_confirmation_request<Presentation>(presentation: Presentation) -> ConfirmationOutcome
where
    Presentation: Future<Output = ConfirmationOutcome>,
{
    presentation.await
}

async fn present_with_parent<P, Lease, Acquire, AcquireFuture, Show>(
    deadline: tokio::time::Instant,
    acquire_parent: Acquire,
    presentation_lease: Lease,
    show: Show,
) -> ConfirmationOutcome
where
    P: Send + 'static,
    Lease: Send + 'static,
    Acquire: FnOnce() -> AcquireFuture,
    AcquireFuture: Future<Output = Option<P>> + Send,
    Show: FnOnce(P, tokio::sync::oneshot::Sender<bool>, Lease),
{
    let parent = match tokio::time::timeout_at(deadline, acquire_parent()).await {
        Ok(Some(parent)) => parent,
        Ok(None) | Err(_) => return ConfirmationOutcome::TimedOut,
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    show(parent, sender, presentation_lease);

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

fn confirmation_parent_window_labels<'a>(
    labels: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    let mut candidates = labels
        .into_iter()
        .filter(|label| {
            crate::windows::is_editor_window_label(label)
                || crate::windows::is_settings_window_label(label)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|label| {
        let priority = if *label == "main" {
            0
        } else if crate::windows::is_editor_window_label(label) {
            1
        } else {
            2
        };
        (priority, *label)
    });
    candidates
}

fn confirmation_parent_window_label<'a>(
    labels: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    confirmation_parent_window_labels(labels).into_iter().next()
}

fn confirmation_parent_windows<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Vec<tauri::WebviewWindow<R>> {
    let windows = app.webview_windows();
    confirmation_parent_window_labels(windows.keys().map(String::as_str))
        .into_iter()
        .filter_map(|label| windows.get(label).cloned())
        .collect()
}

async fn confirmation_parent_has_native_handle<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> bool {
    let app = window.app_handle().clone();
    let window = window.clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    if app
        .run_on_main_thread(move || {
            let available = {
                #[cfg(target_os = "macos")]
                {
                    window.ns_window().is_ok_and(|handle| !handle.is_null())
                }
                #[cfg(target_os = "windows")]
                {
                    window.hwnd().is_ok()
                }
                #[cfg(target_os = "linux")]
                {
                    window.gtk_window().is_ok()
                }
                #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
                {
                    false
                }
            };
            let _ = sender.send(available);
        })
        .is_err()
    {
        return false;
    }
    matches!(
        tokio::time::timeout(CONFIRMATION_NATIVE_HANDLE_CHECK_TIMEOUT, receiver).await,
        Ok(Ok(true))
    )
}

async fn acquire_confirmation_parent<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Option<tauri::WebviewWindow<R>> {
    if crate::desktop_runtime::promote_normal_ui_for_confirmation(app).is_err() {
        return None;
    }
    let deadline = tokio::time::Instant::now() + CONFIRMATION_PARENT_WAIT_TIMEOUT;

    loop {
        for window in confirmation_parent_windows(app) {
            if confirmation_parent_has_native_handle(&window).await && window.show().is_ok() {
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
    use super::{
        await_confirmation_request, confirmation_parent_window_label,
        confirmation_parent_window_labels, present_with_parent, ConfirmationOutcome,
    };
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

    #[test]
    fn confirmation_parent_candidates_preserve_fallback_order() {
        assert_eq!(
            confirmation_parent_window_labels([
                "markra-settings",
                "markra-editor-20",
                "main",
                "markra-editor-3",
                "diagnostics",
            ]),
            vec![
                "main",
                "markra-editor-20",
                "markra-editor-3",
                "markra-settings",
            ]
        );
    }

    #[tokio::test]
    async fn selected_parent_is_forwarded_to_the_dialog_launcher() {
        let presented_parent = Arc::new(Mutex::new(None));
        let captured_parent = Arc::clone(&presented_parent);
        let outcome = present_with_parent(
            tokio::time::Instant::now() + Duration::from_secs(1),
            || async { Some("main") },
            (),
            move |parent, sender, ()| {
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
            (),
            move |_parent, _sender, ()| {
                captured_launch.store(true, Ordering::SeqCst);
            },
        )
        .await;

        assert_eq!(outcome, ConfirmationOutcome::TimedOut);
        assert!(!launched.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn timed_out_waiter_keeps_the_gate_until_the_native_callback_settles() {
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let lease = Arc::clone(&gate).lock_owned().await;
        let callback_state = Arc::new(Mutex::new(None));
        let captured_callback_state = Arc::clone(&callback_state);
        let outcome = present_with_parent(
            tokio::time::Instant::now() + Duration::from_millis(10),
            || async { Some("main") },
            lease,
            move |_parent, sender, lease| {
                *captured_callback_state.lock().expect("callback state") = Some((sender, lease));
            },
        )
        .await;

        assert_eq!(outcome, ConfirmationOutcome::TimedOut);
        assert!(gate.try_lock().is_err());
        let (_sender, lease) = callback_state
            .lock()
            .expect("callback state")
            .take()
            .expect("native callback should retain the presentation lease");
        drop(lease);
        let _released = tokio::time::timeout(Duration::from_secs(1), gate.lock())
            .await
            .expect("callback settlement should release the presentation gate");
    }

    #[tokio::test]
    async fn cancelled_queued_waiter_never_presents_after_the_active_dialog_settles() {
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let active_lease = Arc::clone(&gate).lock_owned().await;
        let launched = Arc::new(AtomicBool::new(false));
        let inner_gate = Arc::clone(&gate);
        let captured_launch = Arc::clone(&launched);
        let waiter = tokio::spawn(await_confirmation_request(async move {
            let _guard = inner_gate.lock_owned().await;
            captured_launch.store(true, Ordering::SeqCst);
            ConfirmationOutcome::Rejected
        }));

        tokio::task::yield_now().await;
        waiter.abort();
        assert!(waiter.await.is_err());
        drop(active_lease);
        let _released = tokio::time::timeout(Duration::from_secs(1), gate.lock())
            .await
            .expect("cancelled queued waiter must not retain or reacquire the gate");
        assert!(!launched.load(Ordering::SeqCst));
    }
}
