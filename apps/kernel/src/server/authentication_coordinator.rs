use std::{
    fmt,
    sync::{Arc, MutexGuard},
    time::Duration,
};

use zeroize::Zeroizing;

use super::secret::SecretDigest;
use super::security::AuthenticationSecurityState;
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
    security: Arc<AuthenticationSecurityState>,
}

impl ServerAuthenticationCoordinator {
    #[cfg(test)]
    pub(crate) fn new(
        authentication: Arc<ServerAuthenticationStore>,
        rate_limiter: AuthenticationRateLimiter,
        sessions: SessionStore,
    ) -> Self {
        Self::from_security(
            authentication,
            Arc::new(AuthenticationSecurityState::new(rate_limiter, sessions)),
        )
    }

    pub(crate) fn from_security(
        authentication: Arc<ServerAuthenticationStore>,
        security: Arc<AuthenticationSecurityState>,
    ) -> Self {
        Self {
            authentication,
            security,
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
        self.ensure_security_available()?;
        let now = self.observe_time(now)?;
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
                                let settlement = self.record_success(permit);
                                self.security.fail_closed();
                                settlement?;
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
                let _session_mutation = self.lock_session_mutation_lifecycle()?;
                let session_now = self.fresh_time()?;
                let session = self
                    .lock_sessions()?
                    .issue(session_now)
                    .map_err(map_session_issue_error)?;
                Ok(ServerLogin {
                    session,
                    needs_rehash: false,
                })
            }
            Err(_unavailable) => {
                drop(password);
                let uncertain = self.authentication.is_state_uncertain();
                let settlement = self.settle_unavailable_attempt(permit, now);
                if uncertain {
                    self.security.fail_closed();
                }
                settlement?;
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
        self.ensure_security_available()?;
        let now = self.observe_time(now)?;
        let current_password = Zeroizing::new(current_password);
        let new_password = Zeroizing::new(new_password);
        {
            let _session_mutation = self.lock_session_mutation_lifecycle()?;
            let mut sessions = self.lock_sessions()?;
            authorize_state_change(&mut sessions, credential, csrf_token, now)?;
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
        let prepared = match self.authentication.prepare_owner_password_change(
            current_password.as_str().to_owned(),
            new_password.as_str().to_owned(),
        ) {
            Ok(prepared) => prepared,
            Err(OwnerPasswordUpdateError::InvalidCurrentPassword) => {
                self.record_failure(permit, now)?;
                return Err(ServerAuthenticationCoordinatorError::InvalidCredentials);
            }
            Err(OwnerPasswordUpdateError::InvalidNewPassword) => {
                self.record_success(permit)?;
                return Err(ServerAuthenticationCoordinatorError::InvalidPassword);
            }
            Err(
                error @ (OwnerPasswordUpdateError::StateUnavailable
                | OwnerPasswordUpdateError::StateUncertain),
            ) => {
                let revocation = {
                    let _session_mutation = self.lock_session_mutation_lifecycle()?;
                    self.lock_sessions()
                        .map(|mut sessions| sessions.revoke_all())
                };
                let settlement = self.settle_unavailable_attempt(permit, now);
                if error == OwnerPasswordUpdateError::StateUncertain {
                    self.security.fail_closed();
                }
                revocation?;
                settlement?;
                return Err(map_password_update_error(error));
            }
        };
        let _password_lifecycle = self.lock_password_lifecycle()?;
        let _session_mutation = self.lock_session_mutation_lifecycle()?;
        let authorization_now = self.fresh_time()?;
        {
            let mut sessions = self.lock_sessions()?;
            if let Err(error) =
                authorize_state_change(&mut sessions, credential, csrf_token, authorization_now)
            {
                self.record_success(permit)?;
                return Err(error);
            }
        }
        let update = self
            .authentication
            .commit_prepared_owner_password_change(prepared);
        match update {
            Ok(()) => {
                let revoked = self.lock_sessions()?.revoke_all();
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
                let revocation = self
                    .lock_sessions()
                    .map(|mut sessions| sessions.revoke_all());
                let settlement = self.settle_unavailable_attempt(permit, now);
                if error == OwnerPasswordUpdateError::StateUncertain {
                    self.security.fail_closed();
                }
                revocation?;
                settlement?;
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
        self.ensure_security_available()?;
        self.observe_time(now)?;
        let _session_mutation = self.lock_session_mutation_lifecycle()?;
        let now = self.fresh_time()?;
        Ok(self
            .lock_sessions()?
            .authorize(credential, csrf_token, intent, now))
    }

    pub fn logout(
        &self,
        credential: &str,
        csrf_token: Option<&str>,
        now: Duration,
    ) -> Result<bool, ServerAuthenticationCoordinatorError> {
        self.ensure_security_available()?;
        self.observe_time(now)?;
        let _session_mutation = self.lock_session_mutation_lifecycle()?;
        let now = self.fresh_time()?;
        let mut sessions = self.lock_sessions()?;
        authorize_state_change(&mut sessions, credential, csrf_token, now)?;
        Ok(sessions.revoke(credential))
    }

    pub fn logout_all(
        &self,
        credential: &str,
        csrf_token: Option<&str>,
        now: Duration,
    ) -> Result<usize, ServerAuthenticationCoordinatorError> {
        self.ensure_security_available()?;
        self.observe_time(now)?;
        let _session_mutation = self.lock_session_mutation_lifecycle()?;
        let now = self.fresh_time()?;
        let mut sessions = self.lock_sessions()?;
        authorize_state_change(&mut sessions, credential, csrf_token, now)?;
        Ok(sessions.revoke_all())
    }

    fn ensure_security_available(&self) -> Result<(), ServerAuthenticationCoordinatorError> {
        if self.authentication.is_state_uncertain() {
            self.security.fail_closed();
        }
        if self.security.is_available() {
            Ok(())
        } else {
            Err(ServerAuthenticationCoordinatorError::StateUnavailable)
        }
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
        if !self.security.is_available() {
            return Err(ServerAuthenticationCoordinatorError::StateUnavailable);
        }
        self.security.rate_limiter.lock().map_err(|_poisoned| {
            self.security.fail_closed();
            ServerAuthenticationCoordinatorError::StateUnavailable
        })
    }

    fn lock_sessions(
        &self,
    ) -> Result<MutexGuard<'_, SessionStore>, ServerAuthenticationCoordinatorError> {
        if !self.security.is_available() {
            return Err(ServerAuthenticationCoordinatorError::StateUnavailable);
        }
        self.security.sessions.lock().map_err(|_poisoned| {
            self.security.fail_closed();
            ServerAuthenticationCoordinatorError::StateUnavailable
        })
    }

    fn lock_password_lifecycle(
        &self,
    ) -> Result<MutexGuard<'_, ()>, ServerAuthenticationCoordinatorError> {
        if !self.security.is_available() {
            return Err(ServerAuthenticationCoordinatorError::StateUnavailable);
        }
        self.security
            .password_lifecycle
            .lock()
            .map_err(|_poisoned| {
                self.security.fail_closed();
                ServerAuthenticationCoordinatorError::StateUnavailable
            })
    }

    fn lock_session_mutation_lifecycle(
        &self,
    ) -> Result<MutexGuard<'_, ()>, ServerAuthenticationCoordinatorError> {
        if !self.security.is_available() {
            return Err(ServerAuthenticationCoordinatorError::StateUnavailable);
        }
        self.security
            .session_mutation_lifecycle
            .lock()
            .map_err(|_poisoned| {
                self.security.fail_closed();
                ServerAuthenticationCoordinatorError::StateUnavailable
            })
    }

    fn observe_time(
        &self,
        candidate: Duration,
    ) -> Result<Duration, ServerAuthenticationCoordinatorError> {
        self.security.observe_time(candidate).map_err(|()| {
            self.security.fail_closed();
            ServerAuthenticationCoordinatorError::StateUnavailable
        })
    }

    fn fresh_time(&self) -> Result<Duration, ServerAuthenticationCoordinatorError> {
        self.security.fresh_time().map_err(|()| {
            self.security.fail_closed();
            ServerAuthenticationCoordinatorError::StateUnavailable
        })
    }
}

fn authorize_state_change(
    sessions: &mut SessionStore,
    credential: &str,
    csrf_token: Option<&str>,
    now: Duration,
) -> Result<(), ServerAuthenticationCoordinatorError> {
    match sessions.authorize(credential, csrf_token, RequestIntent::StateChanging, now) {
        SessionAuthorization::Authorized { .. } => Ok(()),
        SessionAuthorization::InvalidSession => {
            Err(ServerAuthenticationCoordinatorError::InvalidSession)
        }
        SessionAuthorization::CsrfRejected => {
            Err(ServerAuthenticationCoordinatorError::CsrfRejected)
        }
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
        sync::{
            atomic::{AtomicU64, Ordering},
            mpsc, Arc, Mutex,
        },
        thread,
        time::{Duration, Instant},
    };

    use tempfile::tempdir;

    use super::*;
    use crate::{
        paths::KernelPaths,
        server::{RateLimitPolicy, SessionPolicy},
        storage::DurableFileTestFault,
    };

    const OWNER_PASSWORD: &str = "correct horse battery staple";

    struct ManualAuthenticationTimeSource {
        milliseconds: AtomicU64,
    }

    impl ManualAuthenticationTimeSource {
        fn new() -> Self {
            Self {
                milliseconds: AtomicU64::new(0),
            }
        }

        fn advance(&self, duration: Duration) {
            let milliseconds = u64::try_from(duration.as_millis()).unwrap();
            self.milliseconds.fetch_add(milliseconds, Ordering::AcqRel);
        }
    }

    impl crate::server::security::AuthenticationTimeSource for ManualAuthenticationTimeSource {
        fn elapsed(&self) -> Duration {
            Duration::from_millis(self.milliseconds.load(Ordering::Acquire))
        }
    }

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

    fn wait_for_full_admission(coordinator: &ServerAuthenticationCoordinator) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let decision = coordinator
                .security
                .rate_limiter
                .lock()
                .unwrap()
                .begin_attempt(AuthenticationFlow::Login, 99, Duration::from_secs(1));
            match decision {
                Err(RateLimitDecision::AtCapacity) => return,
                Ok(probe) => drop(probe),
                Err(other) => panic!("unexpected admission decision: {other:?}"),
            }
            thread::yield_now();
        }
        panic!("authentication attempt never acquired the only admission permit");
    }

    #[test]
    fn password_change_holds_the_shared_admission_permit_while_waiting_for_lifecycle() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        authentication
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let policy =
            RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let coordinator = Arc::new(ServerAuthenticationCoordinator::new(
            authentication,
            AuthenticationRateLimiter::with_capacity(policy, policy, 8, 1).unwrap(),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        ));
        let login = coordinator
            .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
            .unwrap();
        let credential = login.session().credential().to_owned();
        let csrf = login.session().csrf_token().to_owned();
        let lifecycle = coordinator.security.password_lifecycle.lock().unwrap();
        let worker_coordinator = Arc::clone(&coordinator);
        let worker = thread::spawn(move || {
            worker_coordinator.change_password(
                &credential,
                Some(&csrf),
                Duration::from_secs(1),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
        });

        wait_for_full_admission(&coordinator);
        drop(lifecycle);
        let result = worker.join().unwrap();

        assert_eq!(result.unwrap(), 1);
    }

    #[test]
    fn secondary_session_rejection_settles_the_password_change_admission_permit() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        authentication
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let policy =
            RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let coordinator = Arc::new(ServerAuthenticationCoordinator::new(
            authentication,
            AuthenticationRateLimiter::with_capacity(policy, policy, 8, 1).unwrap(),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        ));
        let login = coordinator
            .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
            .unwrap();
        let credential = login.session().credential().to_owned();
        let csrf = login.session().csrf_token().to_owned();
        let lifecycle = coordinator.security.password_lifecycle.lock().unwrap();
        let worker_coordinator = Arc::clone(&coordinator);
        let worker_credential = credential.clone();
        let worker_csrf = csrf.clone();
        let worker = thread::spawn(move || {
            worker_coordinator.change_password(
                &worker_credential,
                Some(&worker_csrf),
                Duration::from_secs(1),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
        });
        wait_for_full_admission(&coordinator);
        coordinator
            .logout(&credential, Some(&csrf), Duration::from_secs(2))
            .unwrap();
        drop(lifecycle);

        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            ServerAuthenticationCoordinatorError::InvalidSession
        );
        coordinator
            .login(7, Duration::from_secs(3), OWNER_PASSWORD.to_owned())
            .unwrap();
    }

    #[test]
    fn poisoned_secondary_session_lock_settles_the_password_change_admission_permit() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        authentication
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let policy =
            RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let coordinator = Arc::new(ServerAuthenticationCoordinator::new(
            authentication,
            AuthenticationRateLimiter::with_capacity(policy, policy, 8, 1).unwrap(),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        ));
        let login = coordinator
            .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
            .unwrap();
        let credential = login.session().credential().to_owned();
        let csrf = login.session().csrf_token().to_owned();
        let lifecycle = coordinator.security.password_lifecycle.lock().unwrap();
        let worker_coordinator = Arc::clone(&coordinator);
        let worker = thread::spawn(move || {
            worker_coordinator.change_password(
                &credential,
                Some(&csrf),
                Duration::from_secs(1),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
        });
        wait_for_full_admission(&coordinator);
        poison(&coordinator.security.sessions);
        drop(lifecycle);

        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
        let probe = coordinator
            .security
            .rate_limiter
            .lock()
            .unwrap()
            .begin_attempt(AuthenticationFlow::Login, 100, Duration::from_secs(2))
            .expect("failed secondary lock must release the admission permit");
        drop(probe);
    }

    #[test]
    fn poisoned_lifecycle_wait_settles_the_password_change_admission_permit() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        authentication
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let policy =
            RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let coordinator = Arc::new(ServerAuthenticationCoordinator::new(
            authentication,
            AuthenticationRateLimiter::with_capacity(policy, policy, 8, 1).unwrap(),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        ));
        let login = coordinator
            .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
            .unwrap();
        let credential = login.session().credential().to_owned();
        let csrf = login.session().csrf_token().to_owned();
        let lifecycle = coordinator.security.password_lifecycle.lock().unwrap();
        let worker_coordinator = Arc::clone(&coordinator);
        let worker = thread::spawn(move || {
            worker_coordinator.change_password(
                &credential,
                Some(&csrf),
                Duration::from_secs(1),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
        });
        wait_for_full_admission(&coordinator);
        let poisoned = catch_unwind(AssertUnwindSafe(move || {
            let _lifecycle = lifecycle;
            panic!("poison lifecycle while password change owns admission");
        }));
        assert!(poisoned.is_err());

        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
        let probe = coordinator
            .security
            .rate_limiter
            .lock()
            .unwrap()
            .begin_attempt(AuthenticationFlow::Login, 100, Duration::from_secs(2))
            .expect("failed lifecycle wait must release the admission permit");
        drop(probe);
    }

    #[test]
    fn logout_that_finishes_while_password_change_is_preparing_prevents_the_commit() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        authentication
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let (hash_entered_sender, hash_entered_receiver) = mpsc::sync_channel(1);
        let (hash_release_sender, hash_release_receiver) = mpsc::sync_channel(1);
        let hash_release_receiver = Mutex::new(hash_release_receiver);
        authentication.set_password_update_test_hook(Arc::new(move || {
            hash_entered_sender.send(()).unwrap();
            hash_release_receiver.lock().unwrap().recv().unwrap();
        }));
        let policy =
            RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let coordinator = Arc::new(ServerAuthenticationCoordinator::new(
            authentication,
            AuthenticationRateLimiter::new(policy, policy),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        ));
        let login = coordinator
            .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
            .unwrap();
        let credential = login.session().credential().to_owned();
        let csrf = login.session().csrf_token().to_owned();

        let change_coordinator = Arc::clone(&coordinator);
        let change_credential = credential.clone();
        let change_csrf = csrf.clone();
        let change_worker = thread::spawn(move || {
            change_coordinator.change_password(
                &change_credential,
                Some(&change_csrf),
                Duration::from_secs(1),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
        });
        hash_entered_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("password update never reached the hashing boundary");

        let (authorize_sender, authorize_receiver) = mpsc::sync_channel(1);
        let authorize_coordinator = Arc::clone(&coordinator);
        let authorize_credential = credential.clone();
        let authorize_worker = thread::spawn(move || {
            authorize_sender
                .send(authorize_coordinator.authorize(
                    &authorize_credential,
                    None,
                    RequestIntent::ReadOnly,
                    Duration::from_secs(2),
                ))
                .unwrap();
        });
        let (logout_sender, logout_receiver) = mpsc::sync_channel(1);
        let logout_coordinator = Arc::clone(&coordinator);
        let logout_csrf = csrf;
        let logout_worker = thread::spawn(move || {
            logout_sender
                .send(logout_coordinator.logout(
                    &credential,
                    Some(&logout_csrf),
                    Duration::from_secs(2),
                ))
                .unwrap();
        });

        let authorize_early = authorize_receiver.recv_timeout(Duration::from_secs(1));
        let logout_early = logout_receiver.recv_timeout(Duration::from_secs(1));
        let authorize_early = authorize_early.expect("read-only authorize waited on Argon2");
        let logout_early = logout_early.expect("logout waited on Argon2");
        hash_release_sender.send(()).unwrap();
        let change_result = change_worker.join().unwrap();
        authorize_worker.join().unwrap();
        logout_worker.join().unwrap();

        assert!(matches!(
            authorize_early.unwrap(),
            SessionAuthorization::Authorized { .. } | SessionAuthorization::InvalidSession
        ));
        assert!(logout_early.unwrap());
        assert_eq!(
            change_result.unwrap_err(),
            ServerAuthenticationCoordinatorError::InvalidSession
        );
        coordinator
            .login(7, Duration::from_secs(3), OWNER_PASSWORD.to_owned())
            .expect("a revoked prepared change must not replace the old password");
    }

    #[test]
    fn logout_waits_for_an_entered_password_commit_and_observes_revocation() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        authentication
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let (commit_entered_sender, commit_entered_receiver) = mpsc::sync_channel(1);
        let (commit_release_sender, commit_release_receiver) = mpsc::sync_channel(1);
        let commit_release_receiver = Mutex::new(commit_release_receiver);
        authentication.set_password_commit_test_hook(Arc::new(move || {
            commit_entered_sender.send(()).unwrap();
            commit_release_receiver.lock().unwrap().recv().unwrap();
        }));
        let policy =
            RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let coordinator = Arc::new(ServerAuthenticationCoordinator::new(
            authentication,
            AuthenticationRateLimiter::new(policy, policy),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        ));
        let login = coordinator
            .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
            .unwrap();
        let credential = login.session().credential().to_owned();
        let csrf = login.session().csrf_token().to_owned();
        let change_coordinator = Arc::clone(&coordinator);
        let change_credential = credential.clone();
        let change_csrf = csrf.clone();
        let change_worker = thread::spawn(move || {
            change_coordinator.change_password(
                &change_credential,
                Some(&change_csrf),
                Duration::from_secs(1),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
        });
        commit_entered_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("password change never entered its commit boundary");

        let (logout_sender, logout_receiver) = mpsc::sync_channel(1);
        let logout_coordinator = Arc::clone(&coordinator);
        let logout_worker = thread::spawn(move || {
            logout_sender
                .send(logout_coordinator.logout(&credential, Some(&csrf), Duration::from_secs(2)))
                .unwrap();
        });
        assert!(
            logout_receiver
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "logout crossed an active password commit boundary"
        );
        commit_release_sender.send(()).unwrap();

        assert_eq!(change_worker.join().unwrap().unwrap(), 1);
        assert_eq!(
            logout_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::InvalidSession
        );
        logout_worker.join().unwrap();
    }

    #[test]
    fn session_expiry_during_password_preparation_prevents_the_commit() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        authentication
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let (hash_entered_sender, hash_entered_receiver) = mpsc::sync_channel(1);
        let (hash_release_sender, hash_release_receiver) = mpsc::sync_channel(1);
        let hash_release_receiver = Mutex::new(hash_release_receiver);
        authentication.set_password_update_test_hook(Arc::new(move || {
            hash_entered_sender.send(()).unwrap();
            hash_release_receiver.lock().unwrap().recv().unwrap();
        }));
        let policy =
            RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let time_source = Arc::new(ManualAuthenticationTimeSource::new());
        let security = Arc::new(AuthenticationSecurityState::new_with_time_source(
            AuthenticationRateLimiter::new(policy, policy),
            SessionStore::new(SessionPolicy::new(Duration::from_millis(10)).unwrap()),
            time_source.clone(),
        ));
        let coordinator = Arc::new(ServerAuthenticationCoordinator::from_security(
            authentication,
            security,
        ));
        let login = coordinator
            .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
            .unwrap();
        let credential = login.session().credential().to_owned();
        let csrf = login.session().csrf_token().to_owned();
        let worker_coordinator = Arc::clone(&coordinator);
        let worker = thread::spawn(move || {
            worker_coordinator.change_password(
                &credential,
                Some(&csrf),
                Duration::from_secs(0),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
        });

        hash_entered_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("password update never reached the preparation boundary");
        time_source.advance(Duration::from_millis(30));
        hash_release_sender.send(()).unwrap();

        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            ServerAuthenticationCoordinatorError::InvalidSession
        );
        coordinator
            .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
            .expect("an expired prepared change must not replace the old password");
    }

    #[test]
    fn poisoned_rate_limiter_fails_closed_before_password_verification() {
        let temporary = tempdir().unwrap();
        let coordinator = coordinator(temporary.path());
        poison(&coordinator.security.rate_limiter);

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
        poison(&coordinator.security.sessions);

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
            coordinator.logout(
                "session-credential",
                Some("csrf-token"),
                Duration::from_secs(1),
            ),
            Err(ServerAuthenticationCoordinatorError::StateUnavailable)
        );
        assert_eq!(
            coordinator.logout_all(
                "session-credential",
                Some("csrf-token"),
                Duration::from_secs(1),
            ),
            Err(ServerAuthenticationCoordinatorError::StateUnavailable)
        );
    }

    #[test]
    fn poisoned_password_lifecycle_latches_every_later_login_closed() {
        let temporary = tempdir().unwrap();
        let coordinator = coordinator(temporary.path());
        poison(&coordinator.security.password_lifecycle);

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
            ServerAuthenticationCoordinatorError::StateUnavailable
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
                .security
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
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
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
                .security
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
        assert_eq!(
            coordinator
                .logout_all(
                    "session-credential",
                    Some("csrf-token"),
                    Duration::from_secs(2),
                )
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
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
