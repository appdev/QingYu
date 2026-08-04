use std::{future::Future, pin::Pin};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationOutcome {
    Allowed,
    Rejected,
    TimedOut,
}

#[derive(Clone, Debug)]
pub struct ConfirmationRequest {
    pub tool: String,
    pub workspace_display_name: Option<String>,
    pub logical_target: Option<String>,
    pub expected_revision: Option<String>,
    pub effect: String,
}

pub trait ConfirmationPresenter: Send + Sync {
    fn present<'a>(
        &'a self,
        request: ConfirmationRequest,
    ) -> Pin<Box<dyn Future<Output = ConfirmationOutcome> + Send + 'a>>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoUiConfirmationPresenter;

impl ConfirmationPresenter for NoUiConfirmationPresenter {
    fn present<'a>(
        &'a self,
        _request: ConfirmationRequest,
    ) -> Pin<Box<dyn Future<Output = ConfirmationOutcome> + Send + 'a>> {
        Box::pin(async { ConfirmationOutcome::Rejected })
    }
}
