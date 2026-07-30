use std::{fmt, time::Duration};

use super::{
    csrf::RequestIntent,
    secret::{ExposedSecret, RandomSecretError, SecretDigest},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionPolicy {
    lifetime: Duration,
}

impl SessionPolicy {
    pub fn new(lifetime: Duration) -> Result<Self, InvalidSessionPolicy> {
        if lifetime.is_zero() {
            return Err(InvalidSessionPolicy);
        }
        Ok(Self { lifetime })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSessionPolicy;

impl fmt::Display for InvalidSessionPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("session policy is invalid")
    }
}

impl std::error::Error for InvalidSessionPolicy {}

pub struct IssuedSession {
    credential: ExposedSecret,
    csrf_token: ExposedSecret,
    expires_at: Duration,
}

impl IssuedSession {
    pub fn credential(&self) -> &str {
        self.credential.expose()
    }

    pub fn csrf_token(&self) -> &str {
        self.csrf_token.expose()
    }
}

impl fmt::Debug for IssuedSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedSession")
            .field("credential", &"[REDACTED]")
            .field("csrf_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

struct StoredSession {
    credential: SecretDigest,
    csrf_token: SecretDigest,
    expires_at: Duration,
}

pub struct SessionStore {
    policy: SessionPolicy,
    sessions: Vec<StoredSession>,
}

impl SessionStore {
    pub fn new(policy: SessionPolicy) -> Self {
        Self {
            policy,
            sessions: Vec::new(),
        }
    }

    pub fn issue(&mut self, now: Duration) -> Result<IssuedSession, SessionIssueError> {
        let expires_at = now
            .checked_add(self.policy.lifetime)
            .ok_or(SessionIssueError)?;
        let credential = ExposedSecret::generate().map_err(map_random_error)?;
        let csrf_token = ExposedSecret::generate().map_err(map_random_error)?;
        self.sessions.push(StoredSession {
            credential: credential.digest(),
            csrf_token: csrf_token.digest(),
            expires_at,
        });
        Ok(IssuedSession {
            credential,
            csrf_token,
            expires_at,
        })
    }

    pub fn authorize(
        &mut self,
        credential: &str,
        csrf_token: Option<&str>,
        intent: RequestIntent,
        now: Duration,
    ) -> SessionAuthorization {
        self.sessions.retain(|session| now < session.expires_at);
        let Some(session) = self
            .sessions
            .iter()
            .find(|session| session.credential.matches(credential))
        else {
            return SessionAuthorization::InvalidSession;
        };

        if intent == RequestIntent::StateChanging
            && !csrf_token.is_some_and(|candidate| session.csrf_token.matches(candidate))
        {
            return SessionAuthorization::CsrfRejected;
        }

        SessionAuthorization::Authorized {
            expires_at: session.expires_at,
        }
    }
}

impl fmt::Debug for SessionStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionStore")
            .field("policy", &self.policy)
            .field("active_sessions", &self.sessions.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionAuthorization {
    Authorized { expires_at: Duration },
    InvalidSession,
    CsrfRejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionIssueError;

impl fmt::Display for SessionIssueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("session could not be issued")
    }
}

impl std::error::Error for SessionIssueError {}

fn map_random_error(_: RandomSecretError) -> SessionIssueError {
    SessionIssueError
}
