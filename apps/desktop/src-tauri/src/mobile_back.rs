use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(mobile)]
use tauri::{Emitter, Manager};

#[cfg(mobile)]
const MOBILE_BACK_REQUESTED_EVENT: &str = "qingyu://mobile-back-requested";

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

    #[cfg(mobile)]
    pub(crate) fn cancel_request(&self) {
        self.pending.store(false, Ordering::Release);
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
pub(crate) fn emit_mobile_back_requested<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let state = app.state::<MobileBackState>();
    if !state.begin_request() {
        return false;
    }

    let emitted = app
        .get_webview_window("main")
        .is_some_and(|window| window.emit(MOBILE_BACK_REQUESTED_EVENT, ()).is_ok());
    if !emitted {
        state.cancel_request();
    }
    emitted
}

#[tauri::command]
#[cfg(mobile)]
pub(crate) fn complete_mobile_back(
    app: tauri::AppHandle,
    state: tauri::State<'_, MobileBackState>,
    consumed: bool,
) {
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
