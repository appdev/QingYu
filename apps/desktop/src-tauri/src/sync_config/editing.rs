use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use super::status::SyncRunResult;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncEditingSession {
    pub(crate) revision: Option<String>,
    pub(crate) session_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncEditingSnapshot {
    pub(crate) counter: u64,
    pub(crate) pending_apply: Option<SyncPendingApply>,
    pub(crate) state: Option<SyncEditingSession>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SyncApplyExitReason {
    CategoryLeave,
    WindowClose,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SyncApplySource {
    SettingsExit,
    #[cfg(test)]
    TestAlternative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SyncApplyState {
    Claimed,
    Completed,
    Pending,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncPendingApply {
    pub(crate) counter: u64,
    pub(crate) exit_reason: SyncApplyExitReason,
    pub(crate) revision: String,
    pub(crate) session_id: String,
    pub(crate) source: SyncApplySource,
    pub(crate) state: SyncApplyState,
    pub(crate) token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RequestSyncConfigApply {
    pub(crate) exit_reason: SyncApplyExitReason,
    pub(crate) revision: String,
    pub(crate) session_id: String,
    pub(crate) source: SyncApplySource,
    pub(crate) token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct CancelSyncConfigApplyRequest {
    pub(crate) revision: String,
    pub(crate) session_id: String,
    pub(crate) token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SetSyncConfigEditingRequest {
    pub(crate) active: bool,
    pub(crate) revision: Option<String>,
    pub(crate) session_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncEditingEvent {
    pub(crate) active: bool,
    pub(crate) counter: u64,
    pub(crate) revision: Option<String>,
    pub(crate) session_id: String,
}

#[derive(Clone, Default)]
struct SyncEditingRegistry {
    apply: Option<SyncApplyEntry>,
    counter: u64,
    state: Option<SyncEditingSession>,
}

#[derive(Clone, Debug)]
struct SyncApplyEntry {
    completion: tokio::sync::watch::Sender<Option<Result<SyncRunResult, String>>>,
    outcome: Option<Result<SyncRunResult, String>>,
    public: SyncPendingApply,
}

pub(crate) enum SyncApplyDisposition {
    Completed(Result<SyncRunResult, String>),
    Execute,
    Wait,
}

static SYNC_EDITING_REGISTRY: OnceLock<Mutex<SyncEditingRegistry>> = OnceLock::new();

fn registry() -> &'static Mutex<SyncEditingRegistry> {
    SYNC_EDITING_REGISTRY.get_or_init(|| Mutex::new(SyncEditingRegistry::default()))
}

fn state_unavailable() -> String {
    "sync-editing-state-unavailable: The sync editing state is unavailable.".to_string()
}

fn apply_cancelled() -> String {
    "sync-apply-cancelled: The sync settings apply was cancelled.".to_string()
}

fn advance_counter(registry: &mut SyncEditingRegistry) -> Result<u64, String> {
    registry.counter = registry
        .counter
        .checked_add(1)
        .ok_or_else(state_unavailable)?;
    Ok(registry.counter)
}

fn snapshot(registry: &SyncEditingRegistry) -> SyncEditingSnapshot {
    SyncEditingSnapshot {
        counter: registry.counter,
        pending_apply: registry.apply.as_ref().map(|entry| entry.public.clone()),
        state: registry.state.clone(),
    }
}

pub(crate) fn load_sync_editing_state() -> Result<SyncEditingSnapshot, String> {
    registry()
        .lock()
        .map(|registry| snapshot(&registry))
        .map_err(|_| state_unavailable())
}

fn apply_editing_transition(
    registry: &mut SyncEditingRegistry,
    active: bool,
    session_id: &str,
    revision: Option<&str>,
) -> Result<SyncEditingSnapshot, String> {
    advance_counter(registry)?;
    if active {
        if registry
            .apply
            .as_ref()
            .is_some_and(|entry| entry.public.state == SyncApplyState::Completed)
        {
            registry.apply = None;
        }
        registry.state = Some(SyncEditingSession {
            revision: revision.map(str::to_string),
            session_id: session_id.to_string(),
        });
    } else if registry
        .state
        .as_ref()
        .is_some_and(|state| state.session_id == session_id)
    {
        registry.state = None;
    }
    Ok(snapshot(registry))
}

fn editing_event(
    snapshot: SyncEditingSnapshot,
    request: &SetSyncConfigEditingRequest,
) -> SyncEditingEvent {
    if let Some(state) = snapshot.state {
        SyncEditingEvent {
            active: true,
            counter: snapshot.counter,
            revision: state.revision,
            session_id: state.session_id,
        }
    } else {
        SyncEditingEvent {
            active: false,
            counter: snapshot.counter,
            revision: request.revision.clone(),
            session_id: request.session_id.clone(),
        }
    }
}

pub(crate) fn set_sync_editing_with_notify<Notify>(
    request: SetSyncConfigEditingRequest,
    notify: Notify,
) -> Result<SyncEditingEvent, String>
where
    Notify: FnOnce(&SyncEditingEvent) -> Result<(), String>,
{
    let mut registry = registry().lock().map_err(|_| state_unavailable())?;
    set_sync_editing_in_registry_with_notify(&mut registry, request, notify)
}

fn set_sync_editing_in_registry_with_notify<Notify>(
    registry: &mut SyncEditingRegistry,
    request: SetSyncConfigEditingRequest,
    notify: Notify,
) -> Result<SyncEditingEvent, String>
where
    Notify: FnOnce(&SyncEditingEvent) -> Result<(), String>,
{
    let previous = registry.clone();
    let snapshot = apply_editing_transition(
        registry,
        request.active,
        &request.session_id,
        request.revision.as_deref(),
    )?;
    let event = editing_event(snapshot, &request);
    if notify(&event).is_err() {
        *registry = previous;
        return Err(
            "sync-editing-event-unavailable: The sync editing state could not be announced."
                .to_string(),
        );
    }
    Ok(event)
}

fn request_apply(
    registry: &mut SyncEditingRegistry,
    request: &RequestSyncConfigApply,
) -> Result<SyncPendingApply, String> {
    if request.token.trim().is_empty() {
        return Err(
            "sync-apply-session-mismatch: The sync settings session is unavailable.".to_string(),
        );
    }
    if let Some(entry) = registry
        .apply
        .as_ref()
        .filter(|entry| entry.public.token == request.token)
    {
        if entry.public.revision != request.revision
            || entry.public.session_id != request.session_id
            || entry.public.source != request.source
            || entry.public.exit_reason != request.exit_reason
        {
            return Err(
                "sync-apply-mismatch: The sync settings apply identity changed.".to_string(),
            );
        }
        let counter = advance_counter(registry)?;
        let entry = registry.apply.as_mut().ok_or_else(state_unavailable)?;
        entry.public.counter = counter;
        return Ok(entry.public.clone());
    }
    if request.revision.trim().is_empty()
        || !registry
            .state
            .as_ref()
            .is_some_and(|state| state.session_id == request.session_id)
    {
        return Err(
            "sync-apply-session-mismatch: The sync settings session is unavailable.".to_string(),
        );
    }
    if registry.apply.as_ref().is_some_and(|entry| {
        entry.public.token != request.token && entry.public.state != SyncApplyState::Completed
    }) {
        return Err("sync-apply-pending: Another sync settings apply is pending.".to_string());
    }
    let counter = advance_counter(registry)?;
    let pending = SyncPendingApply {
        counter,
        exit_reason: request.exit_reason,
        revision: request.revision.clone(),
        session_id: request.session_id.clone(),
        source: request.source,
        state: SyncApplyState::Pending,
        token: request.token.clone(),
    };
    let (completion, _) = tokio::sync::watch::channel(None);
    registry.apply = Some(SyncApplyEntry {
        completion,
        outcome: None,
        public: pending.clone(),
    });
    Ok(pending)
}

pub(crate) fn request_sync_apply_with_notify<Notify>(
    request: RequestSyncConfigApply,
    notify: Notify,
) -> Result<SyncPendingApply, String>
where
    Notify: FnOnce(&SyncPendingApply) -> Result<(), String>,
{
    let mut registry = registry().lock().map_err(|_| state_unavailable())?;
    request_sync_apply_in_registry_with_notify(&mut registry, request, notify)
}

fn request_sync_apply_in_registry_with_notify<Notify>(
    registry: &mut SyncEditingRegistry,
    request: RequestSyncConfigApply,
    notify: Notify,
) -> Result<SyncPendingApply, String>
where
    Notify: FnOnce(&SyncPendingApply) -> Result<(), String>,
{
    let previous = registry.clone();
    let pending = request_apply(registry, &request)?;
    if notify(&pending).is_err() {
        *registry = previous;
        return Err(
            "sync-apply-event-unavailable: The sync settings apply could not be announced."
                .to_string(),
        );
    }
    Ok(pending)
}

pub(crate) fn sync_editing_active() -> Result<bool, String> {
    registry()
        .lock()
        .map(|registry| registry.state.is_some())
        .map_err(|_| state_unavailable())
}

pub(crate) fn begin_sync_apply(
    revision: &str,
    token: &str,
) -> Result<SyncApplyDisposition, String> {
    let mut registry = registry().lock().map_err(|_| state_unavailable())?;
    begin_apply_in_registry(&mut registry, revision, token)
}

fn begin_apply_in_registry(
    registry: &mut SyncEditingRegistry,
    revision: &str,
    token: &str,
) -> Result<SyncApplyDisposition, String> {
    let entry = registry.apply.clone().ok_or_else(|| {
        "sync-apply-unavailable: The sync settings apply is unavailable.".to_string()
    })?;
    if entry.public.token != token || entry.public.revision != revision {
        return Err("sync-apply-mismatch: The sync settings apply identity changed.".to_string());
    }
    match entry.public.state {
        SyncApplyState::Completed => entry
            .outcome
            .map(SyncApplyDisposition::Completed)
            .ok_or_else(|| {
                "sync-apply-unavailable: The sync settings apply is unavailable.".into()
            }),
        SyncApplyState::Claimed => Ok(SyncApplyDisposition::Wait),
        SyncApplyState::Pending => {
            let counter = advance_counter(registry)?;
            if let Some(entry) = registry.apply.as_mut() {
                entry.public.counter = counter;
                entry.public.state = SyncApplyState::Claimed;
            }
            Ok(SyncApplyDisposition::Execute)
        }
    }
}

pub(crate) async fn wait_sync_apply(revision: &str, token: &str) -> Result<SyncRunResult, String> {
    let mut completion = {
        let registry = registry().lock().map_err(|_| state_unavailable())?;
        let entry = registry.apply.as_ref().ok_or_else(|| {
            "sync-apply-unavailable: The sync settings apply is unavailable.".to_string()
        })?;
        if entry.public.token != token || entry.public.revision != revision {
            return Err(
                "sync-apply-mismatch: The sync settings apply identity changed.".to_string(),
            );
        }
        if entry.public.state == SyncApplyState::Completed {
            return entry.outcome.clone().ok_or_else(|| {
                "sync-apply-unavailable: The sync settings apply is unavailable.".to_string()
            })?;
        }
        entry.completion.subscribe()
    };
    loop {
        if let Some(outcome) = completion.borrow_and_update().clone() {
            return outcome;
        }
        completion.changed().await.map_err(|_| {
            "sync-apply-unavailable: The sync settings apply is unavailable.".to_string()
        })?;
    }
}

pub(crate) fn complete_sync_apply(
    revision: &str,
    token: &str,
    outcome: Result<SyncRunResult, String>,
) -> Result<(), String> {
    let mut registry = registry().lock().map_err(|_| state_unavailable())?;
    complete_apply_in_registry(&mut registry, revision, token, outcome)
}

pub(crate) fn cancel_sync_apply(
    request: CancelSyncConfigApplyRequest,
) -> Result<SyncPendingApply, String> {
    let mut registry = registry().lock().map_err(|_| state_unavailable())?;
    cancel_apply_in_registry(
        &mut registry,
        &request.session_id,
        &request.revision,
        &request.token,
    )
}

fn cancel_apply_in_registry(
    registry: &mut SyncEditingRegistry,
    session_id: &str,
    revision: &str,
    token: &str,
) -> Result<SyncPendingApply, String> {
    let entry = registry.apply.clone().ok_or_else(|| {
        "sync-apply-unavailable: The sync settings apply is unavailable.".to_string()
    })?;
    if entry.public.session_id != session_id
        || entry.public.revision != revision
        || entry.public.token != token
    {
        return Err("sync-apply-mismatch: The sync settings apply identity changed.".to_string());
    }
    if entry.public.state == SyncApplyState::Completed {
        return Ok(entry.public);
    }
    let counter = advance_counter(registry)?;
    let outcome = Err(apply_cancelled());
    let entry = registry.apply.as_mut().ok_or_else(state_unavailable)?;
    entry.outcome = Some(outcome.clone());
    entry.public.counter = counter;
    entry.public.state = SyncApplyState::Completed;
    entry.completion.send_replace(Some(outcome));
    Ok(entry.public.clone())
}

fn complete_apply_in_registry(
    registry: &mut SyncEditingRegistry,
    revision: &str,
    token: &str,
    outcome: Result<SyncRunResult, String>,
) -> Result<(), String> {
    let entry = registry.apply.clone().ok_or_else(|| {
        "sync-apply-unavailable: The sync settings apply is unavailable.".to_string()
    })?;
    if entry.public.token != token || entry.public.revision != revision {
        return Err("sync-apply-mismatch: The sync settings apply identity changed.".to_string());
    }
    if entry.public.state == SyncApplyState::Completed {
        return Ok(());
    }
    let counter = advance_counter(registry)?;
    if let Some(entry) = registry.apply.as_mut() {
        entry.outcome = Some(outcome.clone());
        entry.public.counter = counter;
        entry.public.state = SyncApplyState::Completed;
        entry.completion.send_replace(Some(outcome));
    }
    Ok(())
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct SyncEditingTestRegistry {
    registry: SyncEditingRegistry,
}

#[cfg(test)]
impl SyncEditingTestRegistry {
    pub(crate) fn load(&self) -> SyncEditingSnapshot {
        snapshot(&self.registry)
    }

    pub(crate) fn set(
        &mut self,
        active: bool,
        session_id: &str,
        revision: Option<&str>,
    ) -> Result<SyncEditingSnapshot, String> {
        apply_editing_transition(&mut self.registry, active, session_id, revision)
    }

    pub(crate) fn set_with_notify<Notify>(
        &mut self,
        request: SetSyncConfigEditingRequest,
        notify: Notify,
    ) -> Result<SyncEditingEvent, String>
    where
        Notify: FnOnce(&SyncEditingEvent) -> Result<(), String>,
    {
        set_sync_editing_in_registry_with_notify(&mut self.registry, request, notify)
    }

    pub(crate) fn request_apply(
        &mut self,
        session_id: &str,
        revision: &str,
        token: &str,
    ) -> Result<SyncPendingApply, String> {
        request_apply(
            &mut self.registry,
            &RequestSyncConfigApply {
                exit_reason: SyncApplyExitReason::CategoryLeave,
                revision: revision.into(),
                session_id: session_id.into(),
                source: SyncApplySource::SettingsExit,
                token: token.into(),
            },
        )
    }

    pub(crate) fn request_with_notify<Notify>(
        &mut self,
        request: RequestSyncConfigApply,
        notify: Notify,
    ) -> Result<SyncPendingApply, String>
    where
        Notify: FnOnce(&SyncPendingApply) -> Result<(), String>,
    {
        request_sync_apply_in_registry_with_notify(&mut self.registry, request, notify)
    }

    pub(crate) fn pending_apply_count(&self) -> usize {
        usize::from(self.registry.apply.is_some())
    }

    pub(crate) fn begin_apply(
        &mut self,
        revision: &str,
        token: &str,
    ) -> Result<SyncApplyDisposition, String> {
        begin_apply_in_registry(&mut self.registry, revision, token)
    }

    pub(crate) fn complete_apply(
        &mut self,
        revision: &str,
        token: &str,
        outcome: Result<SyncRunResult, String>,
    ) -> Result<(), String> {
        complete_apply_in_registry(&mut self.registry, revision, token, outcome)
    }

    pub(crate) fn cancel_apply(
        &mut self,
        session_id: &str,
        revision: &str,
        token: &str,
    ) -> Result<SyncPendingApply, String> {
        cancel_apply_in_registry(&mut self.registry, session_id, revision, token)
    }

    pub(crate) fn subscribe_apply(
        &self,
        revision: &str,
        token: &str,
    ) -> Result<tokio::sync::watch::Receiver<Option<Result<SyncRunResult, String>>>, String> {
        let entry = self.registry.apply.as_ref().ok_or_else(|| {
            "sync-apply-unavailable: The sync settings apply is unavailable.".to_string()
        })?;
        if entry.public.token != token || entry.public.revision != revision {
            return Err(
                "sync-apply-mismatch: The sync settings apply identity changed.".to_string(),
            );
        }
        Ok(entry.completion.subscribe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_event_failure_rolls_back_a_test_owned_registry() {
        let mut registry = SyncEditingTestRegistry::default();
        let request = SetSyncConfigEditingRequest {
            active: true,
            revision: Some("rev-event".into()),
            session_id: "event-session".into(),
        };
        let before = registry.load();
        let error = registry
            .set_with_notify(request, |_| Err("event unavailable".into()))
            .err()
            .unwrap();
        assert!(error.starts_with("sync-editing-event-unavailable:"));
        assert_eq!(registry.load(), before);
    }

    #[test]
    fn apply_event_failure_rolls_back_a_test_owned_registry() {
        let mut registry = SyncEditingTestRegistry::default();
        registry.set(true, "session", Some("rev")).unwrap();
        let before = registry.load();
        let request = RequestSyncConfigApply {
            exit_reason: SyncApplyExitReason::CategoryLeave,
            revision: "rev".into(),
            session_id: "session".into(),
            source: SyncApplySource::SettingsExit,
            token: "token".into(),
        };

        let error = registry
            .request_with_notify(request, |_| Err("event unavailable".into()))
            .err()
            .unwrap();

        assert!(error.starts_with("sync-apply-event-unavailable:"));
        assert_eq!(registry.load(), before);
    }

    #[test]
    fn duplicate_apply_token_requires_an_exact_request_identity() {
        let base = RequestSyncConfigApply {
            exit_reason: SyncApplyExitReason::CategoryLeave,
            revision: "rev".into(),
            session_id: "session".into(),
            source: SyncApplySource::SettingsExit,
            token: "token".into(),
        };
        let mismatches = [
            RequestSyncConfigApply {
                revision: "other-rev".into(),
                ..base.clone()
            },
            RequestSyncConfigApply {
                session_id: "other-session".into(),
                ..base.clone()
            },
            RequestSyncConfigApply {
                source: SyncApplySource::TestAlternative,
                ..base.clone()
            },
            RequestSyncConfigApply {
                exit_reason: SyncApplyExitReason::WindowClose,
                ..base.clone()
            },
        ];

        for mismatch in mismatches {
            let mut registry = SyncEditingRegistry::default();
            apply_editing_transition(&mut registry, true, "session", Some("rev")).unwrap();
            request_apply(&mut registry, &base).unwrap();
            let before = snapshot(&registry);

            let error = request_apply(&mut registry, &mismatch).err().unwrap();

            assert!(error.starts_with("sync-apply-mismatch:"));
            assert_eq!(snapshot(&registry), before);
        }
    }

    #[test]
    fn cancelling_an_exact_pending_apply_settles_waiters_and_allows_a_new_session() {
        let mut registry = SyncEditingTestRegistry::default();
        registry
            .set(true, "old-session", Some("old-revision"))
            .unwrap();
        registry
            .request_apply("old-session", "old-revision", "old-token")
            .unwrap();
        let mut waiter = registry
            .subscribe_apply("old-revision", "old-token")
            .unwrap();
        assert!(matches!(
            registry.begin_apply("old-revision", "old-token").unwrap(),
            SyncApplyDisposition::Execute
        ));

        let cancelled = registry
            .cancel_apply("old-session", "old-revision", "old-token")
            .unwrap();

        assert_eq!(cancelled.state, SyncApplyState::Completed);
        let outcome = tauri::async_runtime::block_on(async {
            waiter.changed().await.unwrap();
            waiter.borrow_and_update().clone().unwrap()
        });
        let error = outcome.err().unwrap();
        assert!(error.starts_with("sync-apply-cancelled:"));

        let completed = registry.load();
        let replay = registry
            .cancel_apply("old-session", "old-revision", "old-token")
            .unwrap();
        assert_eq!(replay, cancelled);
        assert_eq!(registry.load(), completed);

        registry
            .set(true, "new-session", Some("new-revision"))
            .unwrap();
        let next = registry
            .request_apply("new-session", "new-revision", "new-token")
            .unwrap();
        assert_eq!(next.state, SyncApplyState::Pending);
        assert_eq!(next.token, "new-token");
    }

    #[test]
    fn cancelling_a_mismatched_identity_never_mutates_the_current_apply() {
        let mut registry = SyncEditingTestRegistry::default();
        registry
            .set(true, "old-session", Some("old-revision"))
            .unwrap();
        registry
            .request_apply("old-session", "old-revision", "old-token")
            .unwrap();
        let old_pending = registry.load();

        let error = registry
            .cancel_apply("other-session", "old-revision", "old-token")
            .err()
            .unwrap();

        assert!(error.starts_with("sync-apply-mismatch:"));
        assert_eq!(registry.load(), old_pending);

        registry
            .cancel_apply("old-session", "old-revision", "old-token")
            .unwrap();
        registry
            .set(true, "new-session", Some("new-revision"))
            .unwrap();
        registry
            .request_apply("new-session", "new-revision", "new-token")
            .unwrap();
        let new_pending = registry.load();

        let error = registry
            .cancel_apply("old-session", "old-revision", "old-token")
            .err()
            .unwrap();

        assert!(error.starts_with("sync-apply-mismatch:"));
        assert_eq!(registry.load(), new_pending);
    }
}
