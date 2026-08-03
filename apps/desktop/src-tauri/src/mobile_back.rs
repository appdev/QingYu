use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(mobile)]
use std::sync::Arc;

#[cfg(mobile)]
use crate::mobile_kernel_runtime::{validated_mobile_renderer_origin, MobileKernelRuntimeState};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MobileBackCompletion {
    Consumed,
    Exit,
    Ignored,
}

#[derive(Default)]
pub(crate) struct MobileBackState {
    pending: AtomicBool,
}

impl MobileBackState {
    pub(crate) fn begin_request(&self) -> bool {
        self.pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn complete_request(&self, consumed: bool) -> MobileBackCompletion {
        if !self.pending.swap(false, Ordering::AcqRel) {
            return MobileBackCompletion::Ignored;
        }

        if consumed {
            MobileBackCompletion::Consumed
        } else {
            MobileBackCompletion::Exit
        }
    }
}

#[cfg(mobile)]
fn mobile_back_renderer_is_authorized(
    window: &tauri::WebviewWindow,
    runtime: &MobileKernelRuntimeState,
) -> bool {
    let Ok(url) = window.url() else {
        return false;
    };

    validated_mobile_renderer_origin(window.label(), runtime.configured_origin(), &url).is_ok()
}

#[tauri::command]
#[cfg(mobile)]
pub(crate) fn begin_mobile_back(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, Arc<MobileKernelRuntimeState>>,
    state: tauri::State<'_, MobileBackState>,
) -> bool {
    mobile_back_renderer_is_authorized(&window, &runtime) && state.begin_request()
}

#[tauri::command]
#[cfg(mobile)]
pub(crate) fn complete_mobile_back(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, Arc<MobileKernelRuntimeState>>,
    state: tauri::State<'_, MobileBackState>,
    consumed: bool,
) {
    if !mobile_back_renderer_is_authorized(&window, &runtime) {
        return;
    }

    if state.complete_request(consumed) == MobileBackCompletion::Exit {
        app.exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::{MobileBackCompletion, MobileBackState};

    #[test]
    fn rapid_duplicate_exit_requests_coalesce_while_one_is_pending() {
        let state = MobileBackState::default();

        assert!(state.begin_request());
        assert!(!state.begin_request());
        assert!(!state.begin_request());
    }

    #[test]
    fn consumed_acknowledgement_clears_pending_without_exiting() {
        let state = MobileBackState::default();
        assert!(state.begin_request());

        assert_eq!(state.complete_request(true), MobileBackCompletion::Consumed);
        assert!(state.begin_request());
    }

    #[test]
    fn unconsumed_acknowledgement_clears_pending_and_requests_exit() {
        let state = MobileBackState::default();
        assert!(state.begin_request());

        assert_eq!(state.complete_request(false), MobileBackCompletion::Exit);
        assert!(state.begin_request());
    }

    #[test]
    fn acknowledgement_without_a_pending_request_is_ignored() {
        let state = MobileBackState::default();

        assert_eq!(state.complete_request(false), MobileBackCompletion::Ignored);
    }
}
