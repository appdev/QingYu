use std::fmt;

use crate::contract::{ApiErrorEnvelope, ErrorCode, ErrorDetails, RequestId};

pub const fn http_status_for_error_code(code: ErrorCode) -> u16 {
    match code {
        ErrorCode::InvalidRequest
        | ErrorCode::InvalidWorkspacePath
        | ErrorCode::InvalidDocumentName => 400,
        ErrorCode::Unauthorized | ErrorCode::InvalidCredentials => 401,
        ErrorCode::HostNotAllowed | ErrorCode::OriginNotAllowed | ErrorCode::CsrfRejected => 403,
        ErrorCode::DocumentNotFound | ErrorCode::ResourceNotFound | ErrorCode::SyncConfigAbsent => {
            404
        }
        ErrorCode::DocumentAlreadyExists
        | ErrorCode::InitializationRequired
        | ErrorCode::AlreadyInitialized
        | ErrorCode::RevisionConflict
        | ErrorCode::SettingsRevisionConflict
        | ErrorCode::SyncConfigRevisionConflict => 409,
        ErrorCode::DocumentTooLarge | ErrorCode::ResourceTooLarge => 413,
        ErrorCode::DocumentInvalidEncoding
        | ErrorCode::InvalidSettingsField
        | ErrorCode::SyncConfigInvalid => 422,
        ErrorCode::WorkspaceLocked => 423,
        ErrorCode::AuthenticationRateLimited => 429,
        ErrorCode::KernelNotReady
        | ErrorCode::AuthenticationUnavailable
        | ErrorCode::WorkspaceUnavailable
        | ErrorCode::SettingsUnavailable
        | ErrorCode::SyncNotReady
        | ErrorCode::SyncRunUnavailable => 503,
        ErrorCode::InternalError => 500,
    }
}

pub fn safe_error_envelope(
    code: ErrorCode,
    request_id: RequestId,
    details: Option<ErrorDetails>,
) -> Result<ApiErrorEnvelope, InvalidErrorDetails> {
    if matches!(code, ErrorCode::AuthenticationRateLimited)
        && !matches!(details.as_ref(), Some(ErrorDetails::RateLimit { .. }))
    {
        return Err(InvalidErrorDetails);
    }
    if details
        .as_ref()
        .is_some_and(|details| !error_details_are_allowed(code, details))
    {
        return Err(InvalidErrorDetails);
    }

    Ok(ApiErrorEnvelope {
        code,
        message: safe_message_for_error_code(code).to_string(),
        request_id,
        details,
    })
}

pub const fn safe_message_for_error_code(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidRequest => "The request is invalid.",
        ErrorCode::InvalidWorkspacePath => "The workspace path is invalid.",
        ErrorCode::InvalidDocumentName => "The document name is invalid.",
        ErrorCode::Unauthorized => "Authentication is required.",
        ErrorCode::InitializationRequired => "Server initialization is required.",
        ErrorCode::AlreadyInitialized => "Server initialization is already complete.",
        ErrorCode::InvalidCredentials => "The credentials are invalid.",
        ErrorCode::CsrfRejected => "The CSRF proof is invalid.",
        ErrorCode::AuthenticationRateLimited => "Authentication is temporarily limited.",
        ErrorCode::AuthenticationUnavailable => "Authentication is unavailable.",
        ErrorCode::HostNotAllowed => "The request host is not allowed.",
        ErrorCode::OriginNotAllowed => "The request origin is not allowed.",
        ErrorCode::KernelNotReady => "The Kernel is not ready.",
        ErrorCode::WorkspaceUnavailable => "The workspace is unavailable.",
        ErrorCode::WorkspaceLocked => "The workspace is locked.",
        ErrorCode::DocumentNotFound => "The document was not found.",
        ErrorCode::ResourceNotFound => "The resource was not found.",
        ErrorCode::DocumentAlreadyExists => "The document already exists.",
        ErrorCode::DocumentTooLarge => "The document exceeds the supported size.",
        ErrorCode::ResourceTooLarge => "The resource exceeds the supported size.",
        ErrorCode::DocumentInvalidEncoding => "The document encoding is invalid.",
        ErrorCode::RevisionConflict => "The document changed since it was loaded.",
        ErrorCode::SettingsRevisionConflict => "The settings changed since they were loaded.",
        ErrorCode::SyncConfigRevisionConflict => {
            "The sync configuration changed since it was loaded."
        }
        ErrorCode::InvalidSettingsField => "A settings field is invalid.",
        ErrorCode::SettingsUnavailable => "Settings are unavailable.",
        ErrorCode::SyncConfigAbsent => "Sync is not configured.",
        ErrorCode::SyncConfigInvalid => "The sync configuration is invalid.",
        ErrorCode::SyncNotReady => "Sync is not ready.",
        ErrorCode::SyncRunUnavailable => "A sync run cannot be started now.",
        ErrorCode::InternalError => "An unexpected error occurred.",
    }
}

const fn error_details_are_allowed(code: ErrorCode, details: &ErrorDetails) -> bool {
    match details {
        ErrorDetails::RevisionConflict { .. } => matches!(
            code,
            ErrorCode::RevisionConflict
                | ErrorCode::SettingsRevisionConflict
                | ErrorCode::SyncConfigRevisionConflict
        ),
        ErrorDetails::Validation { .. } => matches!(
            code,
            ErrorCode::InvalidRequest
                | ErrorCode::InvalidWorkspacePath
                | ErrorCode::InvalidDocumentName
                | ErrorCode::DocumentTooLarge
                | ErrorCode::ResourceTooLarge
                | ErrorCode::DocumentInvalidEncoding
                | ErrorCode::InvalidSettingsField
                | ErrorCode::SyncConfigInvalid
        ),
        ErrorDetails::Startup { .. } => matches!(
            code,
            ErrorCode::KernelNotReady
                | ErrorCode::WorkspaceUnavailable
                | ErrorCode::WorkspaceLocked
                | ErrorCode::SettingsUnavailable
                | ErrorCode::SyncNotReady
                | ErrorCode::SyncRunUnavailable
        ),
        ErrorDetails::RateLimit { .. } => {
            matches!(code, ErrorCode::AuthenticationRateLimited)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidErrorDetails;

impl fmt::Display for InvalidErrorDetails {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("error details do not apply to this error code")
    }
}

impl std::error::Error for InvalidErrorDetails {}
