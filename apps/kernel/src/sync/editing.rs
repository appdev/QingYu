//! Instance-owned sync configuration editing and apply state.

use std::{borrow::Borrow, collections::HashSet, fmt, ops::Deref, sync::Mutex};

use zeroize::Zeroize as _;

use crate::contract::Revision;

#[cfg(test)]
thread_local! {
    static SENSITIVE_IDENTITY_DROP_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_sensitive_identity_drop_count() {
    SENSITIVE_IDENTITY_DROP_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn record_sensitive_identity_drop() {
    SENSITIVE_IDENTITY_DROP_COUNT.with(|count| count.set(count.get().saturating_add(1)));
}

#[cfg(test)]
fn sensitive_identity_drop_count() -> usize {
    SENSITIVE_IDENTITY_DROP_COUNT.with(std::cell::Cell::get)
}

pub struct SyncEditingRegistry {
    state: Mutex<SyncEditingState>,
}

impl SyncEditingRegistry {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SyncEditingState {
                apply: None,
                counter: 0,
                retired_apply_tokens: HashSet::new(),
                revision: None,
                session_id: None,
            }),
        }
    }

    pub fn snapshot(&self) -> Result<SyncEditingSnapshot, SyncEditingRegistryError> {
        self.state
            .lock()
            .map(|state| state.snapshot())
            .map_err(|_| SyncEditingRegistryError::unavailable())
    }

    pub fn set_active(
        &self,
        session_id: String,
        revision: Option<Revision>,
    ) -> Result<SyncEditingSnapshot, SyncEditingRegistryError> {
        let mut state = self.lock_state()?;
        Self::set_active_locked(&mut state, session_id, revision)
    }

    pub fn set_active_with_notify<Notify, NotifyError>(
        &self,
        session_id: String,
        revision: Option<Revision>,
        notify: Notify,
    ) -> Result<SyncEditingSnapshot, SyncEditingRegistryError>
    where
        Notify: FnOnce(&SyncEditingSnapshot) -> Result<(), NotifyError>,
    {
        let mut state = self.lock_state()?;
        let previous = state.clone();
        let snapshot = Self::set_active_locked(&mut state, session_id, revision)?;
        if notify(&snapshot).is_err() {
            *state = previous;
            return Err(SyncEditingRegistryError::notification_unavailable());
        }
        Ok(snapshot)
    }

    fn set_active_locked(
        state: &mut SyncEditingState,
        mut session_id: String,
        revision: Option<Revision>,
    ) -> Result<SyncEditingSnapshot, SyncEditingRegistryError> {
        if session_id.trim().is_empty() {
            session_id.zeroize();
            return Err(SyncEditingRegistryError::invalid_session());
        }
        state.advance()?;
        if state
            .apply
            .as_ref()
            .is_some_and(|entry| entry.public.state == SyncApplyState::Completed)
        {
            state.apply = None;
        }
        state.session_id = Some(SensitiveSyncIdentity::new(session_id));
        state.revision = revision;
        Ok(state.snapshot())
    }

    pub fn clear(&self, session_id: &str) -> Result<SyncEditingSnapshot, SyncEditingRegistryError> {
        let mut state = self.lock_state()?;
        state.advance()?;
        if state.session_id.as_deref() == Some(session_id) {
            state.session_id = None;
            state.revision = None;
        }
        Ok(state.snapshot())
    }

    pub fn request_apply(
        &self,
        request: SyncApplyRequest,
    ) -> Result<SyncPendingApply, SyncEditingRegistryError> {
        let mut state = self.lock_state()?;
        Self::request_apply_locked(&mut state, request)
    }

    pub fn request_apply_with_notify<Notify, NotifyError>(
        &self,
        request: SyncApplyRequest,
        notify: Notify,
    ) -> Result<SyncPendingApply, SyncEditingRegistryError>
    where
        Notify: FnOnce(&SyncPendingApply) -> Result<(), NotifyError>,
    {
        let mut state = self.lock_state()?;
        let previous = state.clone();
        let pending = Self::request_apply_locked(&mut state, request)?;
        if notify(&pending).is_err() {
            *state = previous;
            return Err(SyncEditingRegistryError::notification_unavailable());
        }
        Ok(pending)
    }

    fn request_apply_locked(
        state: &mut SyncEditingState,
        mut request: SyncApplyRequest,
    ) -> Result<SyncPendingApply, SyncEditingRegistryError> {
        if request.token.trim().is_empty() || request.session_id.trim().is_empty() {
            return Err(SyncEditingRegistryError::invalid_session());
        }
        if let Some(entry) = state
            .apply
            .as_ref()
            .filter(|entry| entry.public.token.as_str() == request.token)
        {
            if entry.public.revision != request.revision
                || entry.public.session_id.as_str() != request.session_id
                || entry.public.source != request.source
                || entry.public.exit_reason != request.exit_reason
            {
                return Err(SyncEditingRegistryError::apply_mismatch());
            }
            let counter = state.advance()?;
            let entry = state
                .apply
                .as_mut()
                .ok_or_else(SyncEditingRegistryError::unavailable)?;
            entry.public.counter = counter;
            return Ok(entry.public.clone());
        }
        if state.retired_apply_tokens.contains(request.token.as_str()) {
            return Err(SyncEditingRegistryError::apply_mismatch());
        }
        if state.session_id.as_deref() != Some(request.session_id.as_str()) {
            return Err(SyncEditingRegistryError::invalid_session());
        }
        if state.apply.as_ref().is_some_and(|entry| {
            entry.public.token.as_str() != request.token
                && entry.public.state != SyncApplyState::Completed
        }) {
            return Err(SyncEditingRegistryError::apply_pending());
        }
        let counter = state.advance()?;
        let pending = SyncPendingApply {
            counter,
            exit_reason: request.exit_reason,
            revision: request.revision.clone(),
            session_id: SensitiveSyncIdentity::new(std::mem::take(&mut request.session_id)),
            source: request.source,
            state: SyncApplyState::Pending,
            token: SensitiveSyncIdentity::new(std::mem::take(&mut request.token)),
        };
        let (completion, _) = tokio::sync::watch::channel(None);
        state.apply = Some(SyncApplyEntry {
            completion,
            outcome: None,
            public: pending.clone(),
        });
        Ok(pending)
    }

    pub fn begin_apply(
        &self,
        revision: &Revision,
        token: &str,
    ) -> Result<SyncApplyDisposition, SyncEditingRegistryError> {
        let mut state = self.lock_state()?;
        let entry = state
            .apply
            .as_ref()
            .ok_or_else(SyncEditingRegistryError::apply_unavailable)?;
        if &entry.public.revision != revision || entry.public.token.as_str() != token {
            return Err(SyncEditingRegistryError::apply_mismatch());
        }
        match entry.public.state {
            SyncApplyState::Completed => entry
                .outcome
                .clone()
                .map(SyncApplyDisposition::Completed)
                .ok_or_else(SyncEditingRegistryError::apply_unavailable),
            SyncApplyState::Claimed => Ok(SyncApplyDisposition::Wait),
            SyncApplyState::Pending => {
                let counter = state.advance()?;
                let entry = state
                    .apply
                    .as_mut()
                    .ok_or_else(SyncEditingRegistryError::unavailable)?;
                entry.public.counter = counter;
                entry.public.state = SyncApplyState::Claimed;
                Ok(SyncApplyDisposition::Execute)
            }
        }
    }

    pub async fn wait_apply(
        &self,
        revision: &Revision,
        token: &str,
    ) -> Result<SyncApplyOutcome, SyncEditingRegistryError> {
        let mut completion = {
            let state = self.lock_state()?;
            let entry = state
                .apply
                .as_ref()
                .ok_or_else(SyncEditingRegistryError::apply_unavailable)?;
            if &entry.public.revision != revision || entry.public.token.as_str() != token {
                return Err(SyncEditingRegistryError::apply_mismatch());
            }
            if entry.public.state == SyncApplyState::Completed {
                return entry
                    .outcome
                    .clone()
                    .ok_or_else(SyncEditingRegistryError::apply_unavailable);
            }
            entry.completion.subscribe()
        };
        loop {
            if let Some(outcome) = completion.borrow_and_update().clone() {
                return Ok(outcome);
            }
            completion
                .changed()
                .await
                .map_err(|_| SyncEditingRegistryError::apply_unavailable())?;
        }
    }

    pub fn complete_apply(
        &self,
        revision: &Revision,
        token: &str,
        outcome: SyncApplyOutcome,
    ) -> Result<(), SyncEditingRegistryError> {
        let mut state = self.lock_state()?;
        let entry = state
            .apply
            .as_ref()
            .ok_or_else(SyncEditingRegistryError::apply_unavailable)?;
        if &entry.public.revision != revision || entry.public.token.as_str() != token {
            return Err(SyncEditingRegistryError::apply_mismatch());
        }
        if entry.public.state == SyncApplyState::Completed {
            return Ok(());
        }
        let retired_token = entry.public.token.clone();
        let counter = state.advance()?;
        state.retired_apply_tokens.insert(retired_token);
        let entry = state
            .apply
            .as_mut()
            .ok_or_else(SyncEditingRegistryError::unavailable)?;
        entry.outcome = Some(outcome.clone());
        entry.public.counter = counter;
        entry.public.state = SyncApplyState::Completed;
        entry.completion.send_replace(Some(outcome));
        Ok(())
    }

    pub fn cancel_apply(
        &self,
        session_id: &str,
        revision: &Revision,
        token: &str,
    ) -> Result<SyncPendingApply, SyncEditingRegistryError> {
        let mut state = self.lock_state()?;
        let entry = state
            .apply
            .as_ref()
            .ok_or_else(SyncEditingRegistryError::apply_unavailable)?;
        if entry.public.session_id.as_str() != session_id
            || &entry.public.revision != revision
            || entry.public.token.as_str() != token
        {
            return Err(SyncEditingRegistryError::apply_mismatch());
        }
        if entry.public.state == SyncApplyState::Completed {
            return Ok(entry.public.clone());
        }
        let retired_token = entry.public.token.clone();
        let counter = state.advance()?;
        let outcome = Err(SyncApplyFailure::Cancelled);
        state.retired_apply_tokens.insert(retired_token);
        let entry = state
            .apply
            .as_mut()
            .ok_or_else(SyncEditingRegistryError::unavailable)?;
        entry.outcome = Some(outcome.clone());
        entry.public.counter = counter;
        entry.public.state = SyncApplyState::Completed;
        entry.completion.send_replace(Some(outcome));
        Ok(entry.public.clone())
    }

    fn lock_state(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, SyncEditingState>, SyncEditingRegistryError> {
        self.state
            .lock()
            .map_err(|_| SyncEditingRegistryError::unavailable())
    }
}

impl Default for SyncEditingRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SyncEditingRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncEditingRegistry(..)")
    }
}

#[derive(Clone)]
struct SyncEditingState {
    apply: Option<SyncApplyEntry>,
    counter: u64,
    retired_apply_tokens: HashSet<SensitiveSyncIdentity>,
    revision: Option<Revision>,
    session_id: Option<SensitiveSyncIdentity>,
}

impl SyncEditingState {
    fn advance(&mut self) -> Result<u64, SyncEditingRegistryError> {
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or_else(SyncEditingRegistryError::unavailable)?;
        Ok(self.counter)
    }

    fn snapshot(&self) -> SyncEditingSnapshot {
        SyncEditingSnapshot {
            counter: self.counter,
            pending_apply: self.apply.as_ref().map(|entry| entry.public.clone()),
            revision: self.revision.clone(),
            session_id: self.session_id.clone(),
        }
    }
}

#[derive(Clone)]
struct SyncApplyEntry {
    completion: tokio::sync::watch::Sender<Option<SyncApplyOutcome>>,
    outcome: Option<SyncApplyOutcome>,
    public: SyncPendingApply,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncApplyExitReason {
    CategoryLeave,
    WindowClose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncApplySource {
    SettingsExit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncApplyState {
    Claimed,
    Completed,
    Pending,
}

#[derive(Clone, Eq, PartialEq)]
pub struct SyncApplyRequest {
    pub exit_reason: SyncApplyExitReason,
    pub revision: Revision,
    pub session_id: String,
    pub source: SyncApplySource,
    pub token: String,
}

impl Drop for SyncApplyRequest {
    fn drop(&mut self) {
        self.session_id.zeroize();
        self.token.zeroize();
    }
}

impl fmt::Debug for SyncApplyRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncApplyRequest")
            .field("exit_reason", &self.exit_reason)
            .field("revision", &self.revision)
            .field("session_id", &"[REDACTED]")
            .field("source", &self.source)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SyncPendingApply {
    pub counter: u64,
    pub exit_reason: SyncApplyExitReason,
    pub revision: Revision,
    pub session_id: SensitiveSyncIdentity,
    pub source: SyncApplySource,
    pub state: SyncApplyState,
    pub token: SensitiveSyncIdentity,
}

impl fmt::Debug for SyncPendingApply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncPendingApply")
            .field("counter", &self.counter)
            .field("exit_reason", &self.exit_reason)
            .field("revision", &self.revision)
            .field("session_id", &"[REDACTED]")
            .field("source", &self.source)
            .field("state", &self.state)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncApplySuccess {
    pub revision: Revision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncApplyFailure {
    Cancelled,
    ExecutionUnavailable,
}

pub type SyncApplyOutcome = Result<SyncApplySuccess, SyncApplyFailure>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncApplyDisposition {
    Completed(SyncApplyOutcome),
    Execute,
    Wait,
}

#[derive(Clone, Eq, PartialEq)]
pub struct SyncEditingSnapshot {
    pub counter: u64,
    pub pending_apply: Option<SyncPendingApply>,
    pub revision: Option<Revision>,
    pub session_id: Option<SensitiveSyncIdentity>,
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct SensitiveSyncIdentity(String);

impl SensitiveSyncIdentity {
    fn new(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for SensitiveSyncIdentity {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Deref for SensitiveSyncIdentity {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Debug for SensitiveSyncIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SensitiveSyncIdentity([REDACTED])")
    }
}

impl Drop for SensitiveSyncIdentity {
    fn drop(&mut self) {
        self.0.zeroize();
        #[cfg(test)]
        record_sensitive_identity_drop();
    }
}

impl fmt::Debug for SyncEditingSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncEditingSnapshot")
            .field("counter", &self.counter)
            .field("has_pending_apply", &self.pending_apply.is_some())
            .field("revision", &self.revision)
            .field("has_session", &self.session_id.is_some())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncEditingRegistryErrorKind {
    ApplyMismatch,
    ApplyPending,
    ApplyUnavailable,
    InvalidSession,
    NotificationUnavailable,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncEditingRegistryError {
    kind: SyncEditingRegistryErrorKind,
}

impl SyncEditingRegistryError {
    const fn new(kind: SyncEditingRegistryErrorKind) -> Self {
        Self { kind }
    }

    const fn apply_mismatch() -> Self {
        Self::new(SyncEditingRegistryErrorKind::ApplyMismatch)
    }

    const fn apply_pending() -> Self {
        Self::new(SyncEditingRegistryErrorKind::ApplyPending)
    }

    const fn apply_unavailable() -> Self {
        Self::new(SyncEditingRegistryErrorKind::ApplyUnavailable)
    }

    const fn invalid_session() -> Self {
        Self::new(SyncEditingRegistryErrorKind::InvalidSession)
    }

    const fn notification_unavailable() -> Self {
        Self::new(SyncEditingRegistryErrorKind::NotificationUnavailable)
    }

    const fn unavailable() -> Self {
        Self::new(SyncEditingRegistryErrorKind::Unavailable)
    }

    pub const fn kind(self) -> SyncEditingRegistryErrorKind {
        self.kind
    }
}

impl fmt::Display for SyncEditingRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync editing state is unavailable")
    }
}

impl std::error::Error for SyncEditingRegistryError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_identity_copies_are_zeroized_when_their_owners_drop() {
        reset_sensitive_identity_drop_count();
        {
            let registry = SyncEditingRegistry::new();
            let revision = Revision::parse("8".repeat(64)).unwrap();
            registry
                .set_active("private-session".to_string(), Some(revision.clone()))
                .unwrap();
            let pending = registry
                .request_apply(SyncApplyRequest {
                    exit_reason: SyncApplyExitReason::WindowClose,
                    revision,
                    session_id: "private-session".to_string(),
                    source: SyncApplySource::SettingsExit,
                    token: "private-token".to_string(),
                })
                .unwrap();
            let pending_copy = pending.clone();
            let snapshot = registry.snapshot().unwrap();
            let snapshot_copy = snapshot.clone();
            for safe_debug in [
                format!("{pending:?}"),
                format!("{pending_copy:?}"),
                format!("{snapshot:?}"),
                format!("{snapshot_copy:?}"),
            ] {
                assert!(!safe_debug.contains("private-session"));
                assert!(!safe_debug.contains("private-token"));
            }
        }

        assert!(sensitive_identity_drop_count() >= 8);
    }
}
