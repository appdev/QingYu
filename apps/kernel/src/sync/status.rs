//! Instance-owned, public-safe synchronization status.

use std::{fmt, sync::Mutex};

use crate::contract::{
    Nullable, Rfc3339Utc, RunId, SyncCompletionState, SyncConfigViewDto, SyncSafeErrorCategory,
    SyncSafeErrorCode, SyncSafeErrorDto, SyncSafeErrorOperation, SyncStatusDto, SyncSummaryDto,
    SyncTrigger,
};

pub(crate) enum SyncRunCompletion {
    Succeeded(SyncSummaryDto),
    Failed {
        error: Box<SyncSafeErrorDto>,
        partial_summary: Option<SyncSummaryDto>,
    },
    UnknownFailure,
}

pub struct SyncStatusState {
    current: Mutex<Option<SyncStatusDto>>,
}

impl SyncStatusState {
    pub const fn new() -> Self {
        Self {
            current: Mutex::new(None),
        }
    }

    pub fn snapshot_for(
        &self,
        config: &SyncConfigViewDto,
    ) -> Result<SyncStatusDto, SyncStatusStateError> {
        let current = self.current.lock().map_err(|_| SyncStatusStateError)?;
        let Some(status) = current.as_ref() else {
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
        self.current
            .lock()
            .map(|status| {
                status.as_ref().is_some_and(|status| {
                    status.completion_state == SyncCompletionState::Attempting
                })
            })
            .map_err(|_| SyncStatusStateError)
    }

    pub fn install_config(
        &self,
        config: &SyncConfigViewDto,
    ) -> Result<SyncStatusDto, SyncStatusStateError> {
        let mut current = self.current.lock().map_err(|_| SyncStatusStateError)?;
        if current
            .as_ref()
            .is_some_and(|status| status.completion_state == SyncCompletionState::Attempting)
        {
            return Err(SyncStatusStateError);
        }
        let installed = idle_status(config);
        *current = Some(installed.clone());
        Ok(installed)
    }

    pub fn begin_run(
        &self,
        config: &SyncConfigViewDto,
        run_id: RunId,
        accepted_at: Rfc3339Utc,
        trigger: SyncTrigger,
    ) -> Result<SyncStatusDto, SyncStatusStateError> {
        let mut current = self.current.lock().map_err(|_| SyncStatusStateError)?;
        let status = current.get_or_insert_with(|| idle_status(config));
        if status.completion_state == SyncCompletionState::Attempting {
            return Err(SyncStatusStateError);
        }
        status.completion_state = SyncCompletionState::Attempting;
        status.provider = config.provider;
        status.config_revision = Nullable::value(config.revision.clone());
        status.active_run_id = Nullable::value(run_id);
        status.last_attempt_at = Nullable::value(accepted_at);
        status.last_trigger = Nullable::value(trigger);
        status.summary = Nullable::null();
        status.error = Nullable::null();
        Ok(status.clone())
    }

    pub(crate) fn complete_run(
        &self,
        run_id: RunId,
        completed_at: Rfc3339Utc,
        completion: SyncRunCompletion,
    ) -> Result<SyncStatusDto, SyncStatusStateError> {
        let mut current = self.current.lock().map_err(|_| SyncStatusStateError)?;
        let status = current.as_mut().ok_or(SyncStatusStateError)?;
        if status.active_run_id.as_ref() != Some(&run_id) {
            return Err(SyncStatusStateError);
        }
        status.active_run_id = Nullable::null();
        match completion {
            SyncRunCompletion::Succeeded(summary) => {
                status.completion_state = SyncCompletionState::Succeeded;
                status.last_successful_sync_at = Nullable::value(completed_at);
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
        Ok(status.clone())
    }

    pub fn complete_cancelled(&self, run_id: RunId) -> Result<SyncStatusDto, SyncStatusStateError> {
        let mut current = self.current.lock().map_err(|_| SyncStatusStateError)?;
        let status = current.as_mut().ok_or(SyncStatusStateError)?;
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
        Ok(status.clone())
    }

    #[doc(hidden)]
    pub fn poison_for_test(&self) {
        let _caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.current.lock().expect("status lock before poison");
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
