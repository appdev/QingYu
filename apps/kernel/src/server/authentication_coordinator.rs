use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use zeroize::Zeroizing;

use super::secret::SecretDigest;
use super::{
    AuthenticationAttemptPermit, AuthenticationFlow, AuthenticationRateLimiter,
    InvalidAuthenticationAttempt, IssuedSession, OwnerPasswordUpdateError,
    OwnerPasswordVerification, RateLimitDecision, RequestIntent, ServerAuthenticationStore,
    SessionAuthorization, SessionIssueError, SessionStore,
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
    password_lifecycle: Mutex<()>,
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
            password_lifecycle: Mutex::new(()),
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
        let _password_lifecycle = match self.lock_password_lifecycle() {
            Ok(lifecycle) => lifecycle,
            Err(_unavailable) => {
                drop(password);
                self.settle_unavailable_attempt(permit, now)?;
                return Err(ServerAuthenticationCoordinatorError::StateUnavailable);
            }
        };
        let verification = self.authentication.verify_owner_password(password.as_str());

        match verification {
            Ok(OwnerPasswordVerification::Rejected) => {
                drop(password);
                self.record_failure(permit, now)?;
                Err(ServerAuthenticationCoordinatorError::InvalidCredentials)
            }
            Ok(OwnerPasswordVerification::Authorized { needs_rehash }) => {
                if needs_rehash {
                    let rehash = self
                        .authentication
                        .rehash_owner_password(password.as_str().to_owned());
                    if let Err(error) = rehash {
                        drop(password);
                        return match error {
                            OwnerPasswordUpdateError::InvalidCurrentPassword => {
                                self.record_failure(permit, now)?;
                                Err(ServerAuthenticationCoordinatorError::InvalidCredentials)
                            }
                            OwnerPasswordUpdateError::StateUncertain => {
                                self.record_success(permit)?;
                                Err(ServerAuthenticationCoordinatorError::StateUncertain)
                            }
                            OwnerPasswordUpdateError::InvalidNewPassword
                            | OwnerPasswordUpdateError::StateUnavailable => {
                                self.record_success(permit)?;
                                Err(ServerAuthenticationCoordinatorError::StateUnavailable)
                            }
                        };
                    }
                }
                drop(password);
                self.record_success(permit)?;
                let session = self
                    .lock_sessions()?
                    .issue(now)
                    .map_err(map_session_issue_error)?;
                Ok(ServerLogin {
                    session,
                    needs_rehash: false,
                })
            }
            Err(_unavailable) => {
                drop(password);
                self.settle_unavailable_attempt(permit, now)?;
                Err(ServerAuthenticationCoordinatorError::StateUnavailable)
            }
        }
    }

    /// Replaces the fixed owner's password and revokes every browser session.
    ///
    /// The caller must provide a currently authorized session and its CSRF
    /// token. Current-password Argon2 verification is admitted through a
    /// password-change bucket keyed by the high-entropy session identity. This
    /// complete synchronous method belongs in the host's bounded blocking pool
    /// for the same reason as [`Self::login`]. The host must not retry an
    /// unavailable or uncertain mutation automatically.
    pub fn change_password(
        &self,
        credential: &str,
        csrf_token: Option<&str>,
        now: Duration,
        current_password: String,
        new_password: String,
    ) -> Result<usize, ServerAuthenticationCoordinatorError> {
        let current_password = Zeroizing::new(current_password);
        let new_password = Zeroizing::new(new_password);
        let _password_lifecycle = self.lock_password_lifecycle()?;
        let mut sessions = self.lock_sessions()?;
        match sessions.authorize(credential, csrf_token, RequestIntent::StateChanging, now) {
            SessionAuthorization::Authorized { .. } => {}
            SessionAuthorization::InvalidSession => {
                return Err(ServerAuthenticationCoordinatorError::InvalidSession);
            }
            SessionAuthorization::CsrfRejected => {
                return Err(ServerAuthenticationCoordinatorError::CsrfRejected);
            }
        }
        let permit = {
            let mut limiter = self.lock_rate_limiter()?;
            limiter
                .begin_attempt(
                    AuthenticationFlow::PasswordChange,
                    SecretDigest::rate_limit_client_id(credential),
                    now,
                )
                .map_err(map_rate_limit_decision)?
        };
        let update = self.authentication.change_owner_password(
            current_password.as_str().to_owned(),
            new_password.as_str().to_owned(),
        );
        match update {
            Ok(()) => {
                let revoked = sessions.revoke_all();
                self.record_success(permit)?;
                Ok(revoked)
            }
            Err(OwnerPasswordUpdateError::InvalidCurrentPassword) => {
                self.record_failure(permit, now)?;
                Err(ServerAuthenticationCoordinatorError::InvalidCredentials)
            }
            Err(OwnerPasswordUpdateError::InvalidNewPassword) => {
                self.record_success(permit)?;
                Err(ServerAuthenticationCoordinatorError::InvalidPassword)
            }
            Err(
                error @ (OwnerPasswordUpdateError::StateUnavailable
                | OwnerPasswordUpdateError::StateUncertain),
            ) => {
                sessions.revoke_all();
                self.settle_unavailable_attempt(permit, now)?;
                Err(map_password_update_error(error))
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

    fn settle_unavailable_attempt(
        &self,
        permit: AuthenticationAttemptPermit,
        now: Duration,
    ) -> Result<(), ServerAuthenticationCoordinatorError> {
        match self.record_failure(permit, now) {
            Ok(()) | Err(ServerAuthenticationCoordinatorError::RateLimited { .. }) => Ok(()),
            Err(error) => Err(error),
        }
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

    fn lock_password_lifecycle(
        &self,
    ) -> Result<MutexGuard<'_, ()>, ServerAuthenticationCoordinatorError> {
        self.password_lifecycle
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
    InvalidSession,
    CsrfRejected,
    InvalidPassword,
    RateLimited { retry_after: Duration },
    AtCapacity,
    StateUnavailable,
    StateUncertain,
}

impl fmt::Display for ServerAuthenticationCoordinatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCredentials => "server authentication is not authorized",
            Self::InvalidSession => "server session is not authorized",
            Self::CsrfRejected => "server request CSRF proof is not authorized",
            Self::InvalidPassword => "new owner password is invalid",
            Self::RateLimited { .. } => "server authentication is temporarily limited",
            Self::AtCapacity => "server authentication capacity is exhausted",
            Self::StateUnavailable => "server authentication state is unavailable",
            Self::StateUncertain => "server authentication publication is uncertain",
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

fn map_password_update_error(
    error: OwnerPasswordUpdateError,
) -> ServerAuthenticationCoordinatorError {
    match error {
        OwnerPasswordUpdateError::InvalidCurrentPassword => {
            ServerAuthenticationCoordinatorError::InvalidCredentials
        }
        OwnerPasswordUpdateError::InvalidNewPassword => {
            ServerAuthenticationCoordinatorError::InvalidPassword
        }
        OwnerPasswordUpdateError::StateUnavailable => {
            ServerAuthenticationCoordinatorError::StateUnavailable
        }
        OwnerPasswordUpdateError::StateUncertain => {
            ServerAuthenticationCoordinatorError::StateUncertain
        }
    }
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
        storage::DurableFileTestFault,
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

    #[test]
    fn poisoned_password_lifecycle_fails_closed_and_settles_the_login_attempt() {
        let temporary = tempdir().unwrap();
        let coordinator = coordinator(temporary.path());
        poison(&coordinator.password_lifecycle);

        assert_eq!(
            coordinator
                .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
        assert_eq!(
            coordinator
                .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
        assert_eq!(
            coordinator
                .login(7, Duration::from_secs(2), OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::RateLimited {
                retry_after: Duration::from_secs(29),
            }
        );
    }

    #[test]
    fn uncertain_password_change_revokes_every_session_before_returning_failure() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        ServerAuthenticationStore::open(paths.config_root())
            .unwrap()
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let authentication = Arc::new(
            ServerAuthenticationStore::open_with_test_fault(
                paths.config_root(),
                DurableFileTestFault::ParentSyncFailure,
            )
            .unwrap(),
        );
        let rate_policy =
            RateLimitPolicy::new(2, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let coordinator = ServerAuthenticationCoordinator::new(
            authentication,
            AuthenticationRateLimiter::new(rate_policy, rate_policy),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        );
        let login = coordinator
            .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
            .unwrap();

        assert_eq!(
            coordinator
                .change_password(
                    login.session().credential(),
                    Some(login.session().csrf_token()),
                    Duration::from_secs(1),
                    "incorrect owner password material".to_owned(),
                    "new owner password material".to_owned(),
                )
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::InvalidCredentials
        );

        assert_eq!(
            coordinator
                .change_password(
                    login.session().credential(),
                    Some(login.session().csrf_token()),
                    Duration::from_secs(2),
                    OWNER_PASSWORD.to_owned(),
                    "new owner password material".to_owned(),
                )
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUncertain
        );
        assert_eq!(
            coordinator
                .rate_limiter
                .lock()
                .unwrap()
                .begin_attempt(
                    AuthenticationFlow::PasswordChange,
                    SecretDigest::rate_limit_client_id(login.session().credential()),
                    Duration::from_secs(3),
                )
                .unwrap_err(),
            RateLimitDecision::Limited {
                retry_after: Duration::from_secs(29),
            }
        );
        assert_eq!(
            coordinator
                .authorize(
                    login.session().credential(),
                    None,
                    RequestIntent::ReadOnly,
                    Duration::from_secs(3),
                )
                .unwrap(),
            SessionAuthorization::InvalidSession
        );
        assert_eq!(
            coordinator
                .login(
                    7,
                    Duration::from_secs(4),
                    "new owner password material".to_owned(),
                )
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
    }

    #[test]
    fn unavailable_password_change_revokes_sessions_and_settles_the_attempt_as_a_failure() {
        let temporary = tempdir().unwrap();
        let coordinator = coordinator(temporary.path());
        let login = coordinator
            .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
            .unwrap();

        assert_eq!(
            coordinator
                .change_password(
                    login.session().credential(),
                    Some(login.session().csrf_token()),
                    Duration::from_secs(1),
                    "incorrect owner password material".to_owned(),
                    "new owner password material".to_owned(),
                )
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::InvalidCredentials
        );
        fs::rename(
            temporary.path().join("config"),
            temporary.path().join("displaced-config"),
        )
        .unwrap();
        fs::create_dir(temporary.path().join("config")).unwrap();

        assert_eq!(
            coordinator
                .change_password(
                    login.session().credential(),
                    Some(login.session().csrf_token()),
                    Duration::from_secs(2),
                    OWNER_PASSWORD.to_owned(),
                    "new owner password material".to_owned(),
                )
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
        assert_eq!(
            coordinator
                .rate_limiter
                .lock()
                .unwrap()
                .begin_attempt(
                    AuthenticationFlow::PasswordChange,
                    SecretDigest::rate_limit_client_id(login.session().credential()),
                    Duration::from_secs(3),
                )
                .unwrap_err(),
            RateLimitDecision::Limited {
                retry_after: Duration::from_secs(29),
            }
        );
        assert_eq!(
            coordinator
                .authorize(
                    login.session().credential(),
                    None,
                    RequestIntent::ReadOnly,
                    Duration::from_secs(3),
                )
                .unwrap(),
            SessionAuthorization::InvalidSession
        );
    }

    #[test]
    fn uncertain_rehash_issues_no_session_and_settles_a_valid_attempt_as_success() {
        use argon2::{
            password_hash::{PasswordHasher as _, SaltString},
            Algorithm, Argon2, Params, Version,
        };

        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let weak = Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(4096, 1, 1, Some(32)).unwrap(),
        );
        let salt = SaltString::encode_b64(&[7_u8; 16]).unwrap();
        let hash = weak
            .hash_password(OWNER_PASSWORD.as_bytes(), &salt)
            .unwrap()
            .to_string();
        fs::write(
            temporary.path().join("config/owner-auth-v1.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "passwordHash": hash,
            }))
            .unwrap(),
        )
        .unwrap();
        let authentication = Arc::new(
            ServerAuthenticationStore::open_with_test_fault(
                paths.config_root(),
                DurableFileTestFault::ParentSyncFailure,
            )
            .unwrap(),
        );
        let rate_policy =
            RateLimitPolicy::new(2, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let coordinator = ServerAuthenticationCoordinator::new(
            authentication,
            AuthenticationRateLimiter::new(rate_policy, rate_policy),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        );
        assert_eq!(
            coordinator
                .login(
                    7,
                    Duration::from_secs(0),
                    "incorrect owner password material".to_owned(),
                )
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::InvalidCredentials
        );
        assert_eq!(
            coordinator
                .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUncertain
        );
        assert_eq!(coordinator.logout_all().unwrap(), 0);
        assert_eq!(
            coordinator
                .login(
                    7,
                    Duration::from_secs(2),
                    "incorrect owner password material".to_owned(),
                )
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
    }
}
