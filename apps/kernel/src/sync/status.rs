//! Instance-owned, public-safe synchronization status.

use std::{collections::VecDeque, fmt, sync::Mutex};

use crate::contract::{
    Nullable, Rfc3339Utc, RunId, SyncCompletionState, SyncConfigViewDto, SyncRunCompletionState,
    SyncRunStatusDto, SyncSafeErrorCategory, SyncSafeErrorCode, SyncSafeErrorDto,
    SyncSafeErrorOperation, SyncStatusDto, SyncSummaryDto, SyncTrigger,
};

const RETAINED_SYNC_RUNS: usize = 64;

pub(crate) enum SyncRunCompletion {
    Succeeded(SyncSummaryDto),
    Failed {
        error: Box<SyncSafeErrorDto>,
        partial_summary: Option<SyncSummaryDto>,
    },
    UnknownFailure,
}

pub struct SyncStatusState {
    state: Mutex<SyncStatusData>,
}

struct SyncStatusData {
    current: Option<SyncStatusDto>,
    runs: VecDeque<SyncRunStatusDto>,
}

impl SyncStatusState {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(SyncStatusData {
                current: None,
                runs: VecDeque::new(),
            }),
        }
    }

    pub fn snapshot_for(
        &self,
        config: &SyncConfigViewDto,
    ) -> Result<SyncStatusDto, SyncStatusStateError> {
        let state = self.state.lock().map_err(|_| SyncStatusStateError)?;
        let Some(status) = state.current.as_ref() else {
            return Ok(idle_status(config));
        };
        if status.provider != config.provider
            || status.config_revision.as_ref() != Some(&config.revision)
        {
            return Err(SyncStatusStateError);
        }
        Ok(status.clone())
    }

    pub fn is_attempting(&self) -> Result<bool, SyncStatusStateError> {
        self.state
            .lock()
            .map(|state| {
                state.current.as_ref().is_some_and(|status| {
                    status.completion_state == SyncCompletionState::Attempting
                })
            })
            .map_err(|_| SyncStatusStateError)
    }

    pub fn install_config(
        &self,
        config: &SyncConfigViewDto,
    ) -> Result<SyncStatusDto, SyncStatusStateError> {
        let mut state = self.state.lock().map_err(|_| SyncStatusStateError)?;
        if state
            .current
            .as_ref()
            .is_some_and(|status| status.completion_state == SyncCompletionState::Attempting)
        {
            return Err(SyncStatusStateError);
        }
        let installed = idle_status(config);
        state.current = Some(installed.clone());
        Ok(installed)
    }

    pub fn begin_run(
        &self,
        config: &SyncConfigViewDto,
        run_id: RunId,
        accepted_at: Rfc3339Utc,
        trigger: SyncTrigger,
    ) -> Result<SyncStatusDto, SyncStatusStateError> {
        let mut state = self.state.lock().map_err(|_| SyncStatusStateError)?;
        let status = state.current.get_or_insert_with(|| idle_status(config));
        if status.completion_state == SyncCompletionState::Attempting {
            return Err(SyncStatusStateError);
        }
        status.completion_state = SyncCompletionState::Attempting;
        status.provider = config.provider;
        status.config_revision = Nullable::value(config.revision.clone());
        status.active_run_id = Nullable::value(run_id);
        status.last_attempt_at = Nullable::value(accepted_at.clone());
        status.last_trigger = Nullable::value(trigger);
        status.summary = Nullable::null();
        status.error = Nullable::null();
        let current = status.clone();
        state.runs.push_back(SyncRunStatusDto {
            run_id,
            provider: config.provider,
            config_revision: config.revision.clone(),
            completion_state: SyncRunCompletionState::Attempting,
            accepted_at,
            finished_at: Nullable::null(),
            summary: Nullable::null(),
            error: Nullable::null(),
        });
        while state.runs.len() > RETAINED_SYNC_RUNS {
            state.runs.pop_front();
        }
        Ok(current)
    }

    pub(crate) fn complete_run(
        &self,
        run_id: RunId,
        completed_at: Rfc3339Utc,
        completion: SyncRunCompletion,
    ) -> Result<SyncStatusDto, SyncStatusStateError> {
        let mut state = self.state.lock().map_err(|_| SyncStatusStateError)?;
        let run_index = state
            .runs
            .iter()
            .position(|candidate| candidate.run_id == run_id)
            .ok_or(SyncStatusStateError)?;
        let status = state.current.as_mut().ok_or(SyncStatusStateError)?;
        if status.active_run_id.as_ref() != Some(&run_id) {
            return Err(SyncStatusStateError);
        }
        status.active_run_id = Nullable::null();
        match completion {
            SyncRunCompletion::Succeeded(summary) => {
                status.completion_state = SyncCompletionState::Succeeded;
                status.last_successful_sync_at = Nullable::value(completed_at.clone());
                status.summary = Nullable::value(summary);
                status.error = Nullable::null();
            }
            SyncRunCompletion::Failed {
                error,
                partial_summary,
            } => {
                status.completion_state = SyncCompletionState::Failed;
                status.summary = match partial_summary {
                    Some(summary) => Nullable::value(summary),
                    None => Nullable::null(),
                };
                status.error = Nullable::value(*error);
            }
            SyncRunCompletion::UnknownFailure => {
                status.completion_state = SyncCompletionState::Failed;
                status.summary = Nullable::null();
                status.error = Nullable::value(
                    SyncSafeErrorDto::new(
                        status.provider,
                        SyncSafeErrorOperation::SyncRun,
                        SyncSafeErrorCode::Unknown,
                    )
                    .with_category(SyncSafeErrorCategory::Provider)
                    .with_run_id(run_id),
                );
            }
        }
        let current = status.clone();
        let run = &mut state.runs[run_index];
        run.completion_state = match current.completion_state {
            SyncCompletionState::Failed => SyncRunCompletionState::Failed,
            SyncCompletionState::Succeeded => SyncRunCompletionState::Succeeded,
            SyncCompletionState::Idle | SyncCompletionState::Attempting => {
                return Err(SyncStatusStateError);
            }
        };
        run.finished_at = Nullable::value(completed_at);
        run.summary = current.summary.clone();
        run.error = current.error.clone();
        Ok(current)
    }

    pub fn complete_cancelled(
        &self,
        run_id: RunId,
        completed_at: Rfc3339Utc,
    ) -> Result<SyncStatusDto, SyncStatusStateError> {
        let mut state = self.state.lock().map_err(|_| SyncStatusStateError)?;
        let run_index = state
            .runs
            .iter()
            .position(|candidate| candidate.run_id == run_id)
            .ok_or(SyncStatusStateError)?;
        let status = state.current.as_mut().ok_or(SyncStatusStateError)?;
        if status.active_run_id.as_ref() != Some(&run_id) {
            return Err(SyncStatusStateError);
        }
        status.active_run_id = Nullable::null();
        status.summary = Nullable::null();
        status.completion_state = SyncCompletionState::Failed;
        status.error = Nullable::value(
            SyncSafeErrorDto::new(
                status.provider,
                SyncSafeErrorOperation::SyncRun,
                SyncSafeErrorCode::Cancelled,
            )
            .with_run_id(run_id),
        );
        let current = status.clone();
        let run = &mut state.runs[run_index];
        run.completion_state = SyncRunCompletionState::Failed;
        run.finished_at = Nullable::value(completed_at);
        run.summary = current.summary.clone();
        run.error = current.error.clone();
        Ok(current)
    }

    pub fn snapshot_run(
        &self,
        run_id: RunId,
    ) -> Result<Option<SyncRunStatusDto>, SyncStatusStateError> {
        self.state
            .lock()
            .map(|state| {
                state
                    .runs
                    .iter()
                    .find(|candidate| candidate.run_id == run_id)
                    .cloned()
            })
            .map_err(|_| SyncStatusStateError)
    }

    #[doc(hidden)]
    pub fn poison_for_test(&self) {
        let _caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.state.lock().expect("status lock before poison");
            panic!("deterministic status poison");
        }));
    }
}

impl Default for SyncStatusState {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SyncStatusState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncStatusState(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncStatusStateError;

impl fmt::Display for SyncStatusStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync status is unavailable")
    }
}

impl std::error::Error for SyncStatusStateError {}

fn idle_status(config: &SyncConfigViewDto) -> SyncStatusDto {
    SyncStatusDto {
        completion_state: SyncCompletionState::Idle,
        provider: config.provider,
        config_revision: Nullable::value(config.revision.clone()),
        active_run_id: Nullable::null(),
        last_attempt_at: Nullable::null(),
        last_successful_sync_at: Nullable::null(),
        last_trigger: Nullable::null(),
        summary: Nullable::null(),
        error: Nullable::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{
        SafeUnsignedInteger, SyncProvider, SyncSafeErrorCategory, SyncSafeErrorCode,
        SyncSafeErrorOperation, SyncSummaryDto,
    };

    #[test]
    fn terminal_status_preserves_success_summary_and_safe_failure_details() {
        let state = SyncStatusState::new();
        let config = config_view(SyncProvider::S3, "a");
        let first_run = RunId::new(uuid::Uuid::new_v4());
        let now = Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap();
        let expected_summary = summary(3);
        state
            .begin_run(&config, first_run, now.clone(), SyncTrigger::Manual)
            .unwrap();

        let succeeded = state
            .complete_run(
                first_run,
                now.clone(),
                SyncRunCompletion::Succeeded(expected_summary.clone()),
            )
            .unwrap();

        assert_eq!(succeeded.summary.as_ref(), Some(&expected_summary));
        assert!(succeeded.error.as_ref().is_none());

        let second_run = RunId::new(uuid::Uuid::new_v4());
        let partial = summary(1);
        let error = SyncSafeErrorDto::new(
            SyncProvider::S3,
            SyncSafeErrorOperation::UploadObject,
            SyncSafeErrorCode::RemoteUnavailable,
        )
        .with_category(SyncSafeErrorCategory::Network)
        .with_run_id(second_run);
        state
            .begin_run(&config, second_run, now.clone(), SyncTrigger::Manual)
            .unwrap();

        let failed = state
            .complete_run(
                second_run,
                now,
                SyncRunCompletion::Failed {
                    error: Box::new(error.clone()),
                    partial_summary: Some(partial.clone()),
                },
            )
            .unwrap();

        assert_eq!(failed.summary.as_ref(), Some(&partial));
        assert_eq!(failed.error.as_ref(), Some(&error));
    }

    #[test]
    fn per_run_status_remains_queryable_after_a_later_run_replaces_global_status() {
        let state = SyncStatusState::new();
        let config = config_view(SyncProvider::S3, "run-history");
        let first_run = RunId::new(uuid::Uuid::new_v4());
        let second_run = RunId::new(uuid::Uuid::new_v4());
        let accepted = Rfc3339Utc::parse("2026-07-31T00:00:00Z").unwrap();
        let completed = Rfc3339Utc::parse("2026-07-31T00:00:01Z").unwrap();
        let first_summary = summary(2);

        state
            .begin_run(&config, first_run, accepted.clone(), SyncTrigger::Manual)
            .unwrap();
        let attempting = state.snapshot_run(first_run).unwrap().unwrap();
        assert_eq!(attempting.run_id, first_run);
        assert_eq!(attempting.config_revision, config.revision);
        assert_eq!(
            attempting.completion_state,
            SyncRunCompletionState::Attempting
        );
        assert_eq!(attempting.accepted_at, accepted);
        assert!(attempting.finished_at.as_ref().is_none());

        state
            .complete_run(
                first_run,
                completed.clone(),
                SyncRunCompletion::Succeeded(first_summary.clone()),
            )
            .unwrap();
        state
            .begin_run(&config, second_run, completed.clone(), SyncTrigger::Manual)
            .unwrap();

        let retained = state.snapshot_run(first_run).unwrap().unwrap();
        assert_eq!(retained.completion_state, SyncRunCompletionState::Succeeded);
        assert_eq!(retained.finished_at.as_ref(), Some(&completed));
        assert_eq!(retained.summary.as_ref(), Some(&first_summary));
        assert!(retained.error.as_ref().is_none());
    }

    #[test]
    fn per_run_status_retention_is_bounded_and_cancelled_runs_are_terminal() {
        let state = SyncStatusState::new();
        let config = config_view(SyncProvider::S3, "bounded-history");
        let accepted = Rfc3339Utc::parse("2026-07-31T00:00:00Z").unwrap();
        let completed = Rfc3339Utc::parse("2026-07-31T00:00:01Z").unwrap();
        let mut runs = Vec::new();

        for index in 0..=RETAINED_SYNC_RUNS {
            let run_id = RunId::new(uuid::Uuid::from_u128(index as u128 + 1));
            runs.push(run_id);
            state
                .begin_run(&config, run_id, accepted.clone(), SyncTrigger::Manual)
                .unwrap();
            if index == RETAINED_SYNC_RUNS {
                state.complete_cancelled(run_id, completed.clone()).unwrap();
            } else {
                state
                    .complete_run(
                        run_id,
                        completed.clone(),
                        SyncRunCompletion::Succeeded(summary(0)),
                    )
                    .unwrap();
            }
        }

        assert!(state.snapshot_run(runs[0]).unwrap().is_none());
        assert!(state.snapshot_run(runs[1]).unwrap().is_some());
        let cancelled = state.snapshot_run(*runs.last().unwrap()).unwrap().unwrap();
        assert_eq!(cancelled.completion_state, SyncRunCompletionState::Failed);
        assert_eq!(cancelled.finished_at.as_ref(), Some(&completed));
        assert_eq!(
            cancelled.error.as_ref().map(SyncSafeErrorDto::code),
            Some("cancelled")
        );
    }

    #[test]
    fn snapshot_rejects_stale_completed_identity_without_mutating_it() {
        let state = SyncStatusState::new();
        let first = config_view(SyncProvider::S3, "a");
        let second = config_view(SyncProvider::Webdav, "b");
        let run_id = RunId::new(uuid::Uuid::new_v4());
        let now = Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap();
        state
            .begin_run(&first, run_id, now.clone(), SyncTrigger::Manual)
            .unwrap();
        state
            .complete_run(run_id, now, SyncRunCompletion::Succeeded(summary(0)))
            .unwrap();

        let error = state.snapshot_for(&second).unwrap_err();
        let original = state.snapshot_for(&first).unwrap();

        assert_eq!(error, SyncStatusStateError);
        assert_eq!(original.completion_state, SyncCompletionState::Succeeded);
        assert_eq!(original.provider, SyncProvider::S3);
        assert_eq!(original.config_revision.as_ref(), Some(&first.revision));
    }

    fn config_view(provider: SyncProvider, revision_character: &str) -> SyncConfigViewDto {
        serde_json::from_value(serde_json::json!({
            "revision": revision_character.repeat(64),
            "enabled": false,
            "provider": provider,
            "remoteRoot": "qingyu",
            "mode": "automatic",
            "intervalSeconds": 30,
            "generateConflictDocument": false,
            "configured": true,
            "readiness": "disabled",
            "issues": [],
            "webdav": {
                "serverUrl": { "value": null, "redacted": false },
                "username": "",
                "password": { "present": false }
            },
            "s3": {
                "endpointUrl": { "value": null, "redacted": false },
                "region": "",
                "bucket": "",
                "accessKeyId": { "present": false },
                "secretAccessKey": { "present": false },
                "requestTimeoutSeconds": 60,
                "addressingStyle": "auto",
                "tlsVerification": "verify"
            }
        }))
        .unwrap()
    }

    fn summary(value: u64) -> SyncSummaryDto {
        let value = SafeUnsignedInteger::new(value).unwrap();
        SyncSummaryDto {
            bytes_downloaded: value,
            bytes_uploaded: value,
            conflict_files: value,
            downloaded_files: value,
            scanned_files: value,
            skipped_files: value,
            uploaded_files: value,
        }
    }
}
