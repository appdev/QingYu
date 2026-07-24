use std::{
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    config::{ConfirmationPolicy, DryRunPolicy, McpPermissions, ToolCapability},
    confirmation::{ConfirmationOutcome, ConfirmationPresenter, ConfirmationRequest},
};

const PREVIEW_VERSION: u8 = 1;
const PREVIEW_TTL_SECONDS: i64 = 5 * 60;
const MAX_ACTIVE_PREVIEWS: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationRisk {
    ReadOnly,
    Write,
    HighRisk,
    Destructive,
}

#[derive(Clone, Debug)]
pub(crate) struct OperationDescriptor {
    pub(crate) tool: String,
    pub(crate) workspace_id: Option<Uuid>,
    pub(crate) workspace_display_name: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) expected_revision: Option<String>,
    pub(crate) risk: OperationRisk,
    pub(crate) canonical_arguments: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OperationRequirements {
    pub(crate) confirmation_required: bool,
    pub(crate) preview_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OperationPreview {
    pub(crate) token: String,
    pub(crate) expires_at: i64,
    pub(crate) tool: String,
    pub(crate) target: Option<String>,
    pub(crate) expected_revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PreviewPayload {
    version: u8,
    tool: String,
    arguments_digest: String,
    expected_revision: Option<String>,
    policy_generation: u64,
    workspace_generation: u64,
    invalidation_generation: u64,
    issued_at: i64,
    expires_at: i64,
    nonce: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PolicyError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl PolicyError {
    fn permission_denied() -> Self {
        Self {
            code: "permission_denied",
            message: "The current QingYu MCP policy does not allow this operation.",
        }
    }

    fn invalid_preview() -> Self {
        Self {
            code: "preview_required",
            message: "A matching unexpired preview token is required.",
        }
    }

    fn preview_expired() -> Self {
        Self {
            code: "preview_expired",
            message: "The MCP preview token expired.",
        }
    }

    fn preview_capacity() -> Self {
        Self {
            code: "rate_limited",
            message: "Too many MCP previews are active.",
        }
    }

    fn random() -> Self {
        Self {
            code: "preview_generation_failed",
            message: "The MCP preview token could not be generated.",
        }
    }

    fn confirmation_rejected() -> Self {
        Self {
            code: "confirmation_rejected",
            message: "The operation was rejected in QingYu.",
        }
    }

    fn confirmation_timeout() -> Self {
        Self {
            code: "confirmation_timeout",
            message: "The QingYu confirmation timed out.",
        }
    }
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PolicyError {}

pub(crate) struct PolicyEngine {
    signing_key: [u8; 32],
    active_previews: Mutex<HashMap<String, i64>>,
    invalidation_generation: AtomicU64,
}

impl PolicyEngine {
    pub(crate) fn new(signing_key: [u8; 32]) -> Self {
        Self {
            signing_key,
            active_previews: Mutex::new(HashMap::new()),
            invalidation_generation: AtomicU64::new(1),
        }
    }

    pub(crate) fn authorize(
        permissions: &McpPermissions,
        capability: ToolCapability,
    ) -> Result<(), PolicyError> {
        if permissions.allows(capability) {
            return Ok(());
        }
        Err(PolicyError::permission_denied())
    }

    pub(crate) fn requirements(
        confirmation: ConfirmationPolicy,
        dry_run: DryRunPolicy,
        risk: OperationRisk,
    ) -> OperationRequirements {
        let is_write = !matches!(risk, OperationRisk::ReadOnly);
        let is_high_risk = matches!(risk, OperationRisk::HighRisk | OperationRisk::Destructive);
        OperationRequirements {
            confirmation_required: is_write
                && match confirmation {
                    ConfirmationPolicy::Never => false,
                    ConfirmationPolicy::DestructiveOnly => is_high_risk,
                    ConfirmationPolicy::AllWrites => true,
                },
            preview_required: is_write
                && match dry_run {
                    DryRunPolicy::Never => false,
                    DryRunPolicy::HighRisk => is_high_risk,
                    DryRunPolicy::AllWrites => true,
                },
        }
    }

    pub(crate) fn preview(
        &self,
        descriptor: &OperationDescriptor,
        policy_generation: u64,
        workspace_generation: u64,
    ) -> Result<OperationPreview, PolicyError> {
        self.preview_at(
            descriptor,
            policy_generation,
            workspace_generation,
            unix_time_seconds()?,
        )
    }

    pub(crate) fn preview_at(
        &self,
        descriptor: &OperationDescriptor,
        policy_generation: u64,
        workspace_generation: u64,
        now: i64,
    ) -> Result<OperationPreview, PolicyError> {
        let nonce = random_nonce()?;
        let expires_at = now.saturating_add(PREVIEW_TTL_SECONDS);
        let invalidation_generation = self.invalidation_generation.load(Ordering::Acquire);
        let payload = PreviewPayload {
            version: PREVIEW_VERSION,
            tool: descriptor.tool.clone(),
            arguments_digest: arguments_digest(&descriptor.canonical_arguments),
            expected_revision: descriptor.expected_revision.clone(),
            policy_generation,
            workspace_generation,
            invalidation_generation,
            issued_at: now,
            expires_at,
            nonce: nonce.clone(),
        };
        let bytes = serde_json::to_vec(&payload).map_err(|_| PolicyError::invalid_preview())?;
        let signature = self.sign(&bytes)?;

        let mut active = self
            .active_previews
            .lock()
            .map_err(|_| PolicyError::invalid_preview())?;
        active.retain(|_, expiry| *expiry >= now);
        if active.len() >= MAX_ACTIVE_PREVIEWS {
            return Err(PolicyError::preview_capacity());
        }
        active.insert(nonce, expires_at);

        Ok(OperationPreview {
            token: format!(
                "{}.{}",
                URL_SAFE_NO_PAD.encode(bytes),
                URL_SAFE_NO_PAD.encode(signature)
            ),
            expires_at,
            tool: descriptor.tool.clone(),
            target: descriptor.target.clone(),
            expected_revision: descriptor.expected_revision.clone(),
        })
    }

    pub(crate) fn consume_preview(
        &self,
        token: &str,
        descriptor: &OperationDescriptor,
        policy_generation: u64,
        workspace_generation: u64,
    ) -> Result<(), PolicyError> {
        self.consume_preview_at(
            token,
            descriptor,
            policy_generation,
            workspace_generation,
            unix_time_seconds()?,
        )
    }

    pub(crate) fn consume_preview_at(
        &self,
        token: &str,
        descriptor: &OperationDescriptor,
        policy_generation: u64,
        workspace_generation: u64,
        now: i64,
    ) -> Result<(), PolicyError> {
        let payload = self.decode(token)?;
        if now > payload.expires_at {
            if let Ok(mut active) = self.active_previews.lock() {
                active.remove(&payload.nonce);
            }
            return Err(PolicyError::preview_expired());
        }
        let current_invalidation = self.invalidation_generation.load(Ordering::Acquire);
        if payload.version != PREVIEW_VERSION
            || payload.issued_at > now
            || payload.tool != descriptor.tool
            || payload.arguments_digest != arguments_digest(&descriptor.canonical_arguments)
            || payload.expected_revision != descriptor.expected_revision
            || payload.policy_generation != policy_generation
            || payload.workspace_generation != workspace_generation
            || payload.invalidation_generation != current_invalidation
        {
            return Err(PolicyError::invalid_preview());
        }

        let mut active = self
            .active_previews
            .lock()
            .map_err(|_| PolicyError::invalid_preview())?;
        match active.remove(&payload.nonce) {
            Some(expiry) if expiry == payload.expires_at => Ok(()),
            _ => Err(PolicyError::invalid_preview()),
        }
    }

    pub(crate) fn invalidate_previews(&self) {
        self.invalidation_generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut active) = self.active_previews.lock() {
            active.clear();
        }
    }

    pub(crate) async fn confirm_if_required(
        &self,
        confirmation: ConfirmationPolicy,
        descriptor: &OperationDescriptor,
        presenter: &dyn ConfirmationPresenter,
    ) -> Result<ConfirmationOutcome, PolicyError> {
        let required = Self::requirements(confirmation, DryRunPolicy::Never, descriptor.risk)
            .confirmation_required;
        if !required {
            return Ok(ConfirmationOutcome::Allowed);
        }
        let request = ConfirmationRequest {
            tool: descriptor.tool.clone(),
            workspace_display_name: descriptor.workspace_display_name.clone(),
            logical_target: descriptor.target.clone(),
            expected_revision: descriptor.expected_revision.clone(),
            effect: operation_effect(descriptor.risk).to_string(),
        };
        match presenter.present(request).await {
            ConfirmationOutcome::Allowed => Ok(ConfirmationOutcome::Allowed),
            ConfirmationOutcome::Rejected => Err(PolicyError::confirmation_rejected()),
            ConfirmationOutcome::TimedOut => Err(PolicyError::confirmation_timeout()),
        }
    }

    fn decode(&self, token: &str) -> Result<PreviewPayload, PolicyError> {
        let mut parts = token.split('.');
        let payload = parts.next().ok_or_else(PolicyError::invalid_preview)?;
        let signature = parts.next().ok_or_else(PolicyError::invalid_preview)?;
        if parts.next().is_some() || payload.is_empty() || signature.is_empty() {
            return Err(PolicyError::invalid_preview());
        }
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| PolicyError::invalid_preview())?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| PolicyError::invalid_preview())?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.signing_key)
            .map_err(|_| PolicyError::invalid_preview())?;
        mac.update(&payload);
        mac.verify_slice(&signature)
            .map_err(|_| PolicyError::invalid_preview())?;
        serde_json::from_slice(&payload).map_err(|_| PolicyError::invalid_preview())
    }

    fn sign(&self, payload: &[u8]) -> Result<Vec<u8>, PolicyError> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.signing_key)
            .map_err(|_| PolicyError::invalid_preview())?;
        mac.update(payload);
        Ok(mac.finalize().into_bytes().to_vec())
    }
}

fn arguments_digest(arguments: &str) -> String {
    format!("{:x}", Sha256::digest(arguments.as_bytes()))
}

fn random_nonce() -> Result<String, PolicyError> {
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| PolicyError::random())?;
    Ok(URL_SAFE_NO_PAD.encode(nonce))
}

fn unix_time_seconds() -> Result<i64, PolicyError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| duration.as_secs().try_into().ok())
        .ok_or_else(PolicyError::invalid_preview)
}

fn operation_effect(risk: OperationRisk) -> &'static str {
    match risk {
        OperationRisk::ReadOnly => "Read data",
        OperationRisk::Write => "Change QingYu data",
        OperationRisk::HighRisk => "Change sensitive QingYu configuration or remote data",
        OperationRisk::Destructive => "Delete or propagate changes",
    }
}
