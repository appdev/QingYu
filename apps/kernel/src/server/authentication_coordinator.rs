use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use zeroize::Zeroizing;

use super::{
    AuthenticationAttemptPermit, AuthenticationFlow, AuthenticationRateLimiter,
    InvalidAuthenticationAttempt, IssuedSession, OwnerPasswordVerification, RateLimitDecision,
    RequestIntent, ServerAuthenticationStore, SessionAuthorization, SessionIssueError,
    SessionStore,
};

pub struct ServerLogin {
    session: IssuedSession,
    needs_rehash: bool,
}

impl ServerLogin {
    pub fn session(&self) -> &IssuedSession {
        &self.session
    }

    pub fn needs_rehash(&self) -> bool {
        self.needs_rehash
    }

    pub fn into_session(self) -> IssuedSession {
        self.session
    }
}

impl fmt::Debug for ServerLogin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerLogin")
            .field("session", &"[REDACTED]")
            .field("needs_rehash", &self.needs_rehash)
            .finish()
    }
}

pub struct ServerAuthenticationCoordinator {
    authentication: Arc<ServerAuthenticationStore>,
    rate_limiter: Mutex<AuthenticationRateLimiter>,
    sessions: Mutex<SessionStore>,
}

impl ServerAuthenticationCoordinator {
    pub fn new(
        authentication: Arc<ServerAuthenticationStore>,
        rate_limiter: AuthenticationRateLimiter,
        sessions: SessionStore,
    ) -> Self {
        Self {
            authentication,
            rate_limiter: Mutex::new(rate_limiter),
            sessions: Mutex::new(sessions),
        }
    }

    /// Authenticates the fixed server owner and issues one browser session.
    ///
    /// `client_id` must already be normalized by the host from its direct peer
    /// or a configured trusted proxy. This layer never parses or trusts a
    /// `Forwarded` header.
    ///
    /// Argon2 verification and permit settlement are intentionally synchronous
    /// so an async request cancellation cannot abandon an admitted failure.
    /// The server host must execute this complete method in a bounded blocking
    /// pool and must not split its admission, verification, or settlement steps.
    pub fn login(
        &self,
        client_id: u64,
        now: Duration,
        password: String,
    ) -> Result<ServerLogin, ServerAuthenticationCoordinatorError> {
        let permit = {
            let mut limiter = self.lock_rate_limiter()?;
            limiter
                .begin_attempt(AuthenticationFlow::Login, client_id, now)
                .map_err(map_rate_limit_decision)?
        };
        let password = Zeroizing::new(password);
        let verification = self.authentication.verify_owner_password(password.as_str());
        drop(password);

        match verification {
            Ok(OwnerPasswordVerification::Rejected) => {
                self.record_failure(permit, now)?;
                Err(ServerAuthenticationCoordinatorError::InvalidCredentials)
            }
            Ok(OwnerPasswordVerification::Authorized { needs_rehash }) => {
                self.record_success(permit)?;
                let session = self
                    .lock_sessions()?
                    .issue(now)
                    .map_err(map_session_issue_error)?;
                Ok(ServerLogin {
                    session,
                    needs_rehash,
                })
            }
            Err(_unavailable) => {
                match self.record_failure(permit, now) {
                    Ok(()) | Err(ServerAuthenticationCoordinatorError::RateLimited { .. }) => {}
                    Err(error) => return Err(error),
                }
                Err(ServerAuthenticationCoordinatorError::StateUnavailable)
            }
        }
    }

    pub fn authorize(
        &self,
        credential: &str,
        csrf_token: Option<&str>,
        intent: RequestIntent,
        now: Duration,
    ) -> Result<SessionAuthorization, ServerAuthenticationCoordinatorError> {
        Ok(self
            .lock_sessions()?
            .authorize(credential, csrf_token, intent, now))
    }

    pub fn logout(&self, credential: &str) -> Result<bool, ServerAuthenticationCoordinatorError> {
        Ok(self.lock_sessions()?.revoke(credential))
    }

    pub fn logout_all(&self) -> Result<usize, ServerAuthenticationCoordinatorError> {
        Ok(self.lock_sessions()?.revoke_all())
    }

    fn record_failure(
        &self,
        permit: AuthenticationAttemptPermit,
        now: Duration,
    ) -> Result<(), ServerAuthenticationCoordinatorError> {
        let decision = self
            .lock_rate_limiter()?
            .record_failure(permit, now)
            .map_err(map_invalid_attempt)?;
        match decision {
            RateLimitDecision::Allowed => Ok(()),
            RateLimitDecision::Limited { retry_after } => {
                Err(ServerAuthenticationCoordinatorError::RateLimited { retry_after })
            }
            RateLimitDecision::AtCapacity => {
                Err(ServerAuthenticationCoordinatorError::StateUnavailable)
            }
        }
    }

    fn record_success(
        &self,
        permit: AuthenticationAttemptPermit,
    ) -> Result<(), ServerAuthenticationCoordinatorError> {
        self.lock_rate_limiter()?
            .record_success(permit)
            .map_err(map_invalid_attempt)
    }

    fn lock_rate_limiter(
        &self,
    ) -> Result<MutexGuard<'_, AuthenticationRateLimiter>, ServerAuthenticationCoordinatorError>
    {
        self.rate_limiter
            .lock()
            .map_err(|_poisoned| ServerAuthenticationCoordinatorError::StateUnavailable)
    }

    fn lock_sessions(
        &self,
    ) -> Result<MutexGuard<'_, SessionStore>, ServerAuthenticationCoordinatorError> {
        self.sessions
            .lock()
            .map_err(|_poisoned| ServerAuthenticationCoordinatorError::StateUnavailable)
    }
}

impl fmt::Debug for ServerAuthenticationCoordinator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerAuthenticationCoordinator")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerAuthenticationCoordinatorError {
    InvalidCredentials,
    RateLimited { retry_after: Duration },
    AtCapacity,
    StateUnavailable,
}

impl fmt::Display for ServerAuthenticationCoordinatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCredentials => "server authentication is not authorized",
            Self::RateLimited { .. } => "server authentication is temporarily limited",
            Self::AtCapacity => "server authentication capacity is exhausted",
            Self::StateUnavailable => "server authentication state is unavailable",
        })
    }
}

impl std::error::Error for ServerAuthenticationCoordinatorError {}

fn map_rate_limit_decision(decision: RateLimitDecision) -> ServerAuthenticationCoordinatorError {
    match decision {
        RateLimitDecision::Limited { retry_after } => {
            ServerAuthenticationCoordinatorError::RateLimited { retry_after }
        }
        RateLimitDecision::AtCapacity => ServerAuthenticationCoordinatorError::AtCapacity,
        RateLimitDecision::Allowed => ServerAuthenticationCoordinatorError::StateUnavailable,
    }
}

fn map_invalid_attempt(_: InvalidAuthenticationAttempt) -> ServerAuthenticationCoordinatorError {
    ServerAuthenticationCoordinatorError::StateUnavailable
}

fn map_session_issue_error(_: SessionIssueError) -> ServerAuthenticationCoordinatorError {
    ServerAuthenticationCoordinatorError::StateUnavailable
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        panic::{catch_unwind, AssertUnwindSafe},
        path::Path,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use tempfile::tempdir;

    use super::*;
    use crate::{
        paths::KernelPaths,
        server::{RateLimitPolicy, SessionPolicy},
    };

    const OWNER_PASSWORD: &str = "correct horse battery staple";

    fn fixture_paths(root: &Path) -> KernelPaths {
        let workspace = root.join("workspace");
        let config = root.join("config");
        let cache = root.join("cache");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&cache).unwrap();
        KernelPaths::desktop(&workspace, &config, &cache).unwrap()
    }

    fn coordinator(root: &Path) -> ServerAuthenticationCoordinator {
        let paths = fixture_paths(root);
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        authentication
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let policy =
            RateLimitPolicy::new(2, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        ServerAuthenticationCoordinator::new(
            authentication,
            AuthenticationRateLimiter::new(policy, policy),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        )
    }

    fn poison<T>(state: &Mutex<T>) {
        let poisoned = catch_unwind(AssertUnwindSafe(|| {
            let _guard = state.lock().unwrap();
            panic!("poison authentication coordinator state for fail-closed test");
        }));
        assert!(poisoned.is_err());
    }

    #[test]
    fn poisoned_rate_limiter_fails_closed_before_password_verification() {
        let temporary = tempdir().unwrap();
        let coordinator = coordinator(temporary.path());
        poison(&coordinator.rate_limiter);

        assert_eq!(
            coordinator
                .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
    }

    #[test]
    fn poisoned_session_store_fails_closed_for_every_session_operation() {
        let temporary = tempdir().unwrap();
        let coordinator = coordinator(temporary.path());
        poison(&coordinator.sessions);

        assert_eq!(
            coordinator
                .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
        assert_eq!(
            coordinator.authorize(
                "session-credential",
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(1),
            ),
            Err(ServerAuthenticationCoordinatorError::StateUnavailable)
        );
        assert_eq!(
            coordinator.logout("session-credential"),
            Err(ServerAuthenticationCoordinatorError::StateUnavailable)
        );
        assert_eq!(
            coordinator.logout_all(),
            Err(ServerAuthenticationCoordinatorError::StateUnavailable)
        );
    }
}
