use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use super::{
    AuthenticationRateLimiter, InitializationToken, ServerAuthenticationCoordinator,
    ServerAuthenticationError, ServerAuthenticationStore, ServerInitializationCoordinator,
    ServerInitializationCoordinatorError, SessionStore,
};

pub(crate) struct AuthenticationSecurityState {
    pub(crate) rate_limiter: Mutex<AuthenticationRateLimiter>,
    pub(crate) sessions: Mutex<SessionStore>,
    pub(crate) password_lifecycle: Mutex<()>,
    failed_closed: AtomicBool,
}

impl AuthenticationSecurityState {
    pub(crate) fn new(rate_limiter: AuthenticationRateLimiter, sessions: SessionStore) -> Self {
        Self {
            rate_limiter: Mutex::new(rate_limiter),
            sessions: Mutex::new(sessions),
            password_lifecycle: Mutex::new(()),
            failed_closed: AtomicBool::new(false),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        if self.failed_closed.load(Ordering::Acquire) {
            return false;
        }
        let available = !self.rate_limiter.is_poisoned()
            && !self.sessions.is_poisoned()
            && !self.password_lifecycle.is_poisoned();
        if !available {
            self.fail_closed();
        }
        available
    }

    pub(crate) fn fail_closed(&self) {
        self.failed_closed.store(true, Ordering::Release);
    }
}

/// Process-lifetime owner for transport-neutral server authentication state.
///
/// A server host constructs exactly one owner and derives both initialization
/// and authenticated-session coordinators from it. Every derived coordinator
/// then shares one authentication concurrency budget, rate limiter, session
/// store, and password lifecycle lock.
pub struct ServerAuthenticationSecurity {
    authentication: Arc<ServerAuthenticationStore>,
    state: Arc<AuthenticationSecurityState>,
    _owner_claim: Arc<()>,
}

impl ServerAuthenticationSecurity {
    pub fn claim(
        authentication: Arc<ServerAuthenticationStore>,
        rate_limiter: AuthenticationRateLimiter,
        sessions: SessionStore,
    ) -> Result<Self, ServerAuthenticationError> {
        let owner_claim = authentication.claim_security_owner()?;
        Ok(Self {
            authentication,
            state: Arc::new(AuthenticationSecurityState::new(rate_limiter, sessions)),
            _owner_claim: owner_claim,
        })
    }

    pub fn authentication_coordinator(&self) -> ServerAuthenticationCoordinator {
        ServerAuthenticationCoordinator::from_security(
            Arc::clone(&self.authentication),
            Arc::clone(&self.state),
        )
    }

    pub fn initialization_coordinator(
        &self,
        initialization_token: Option<InitializationToken>,
    ) -> Result<ServerInitializationCoordinator, ServerInitializationCoordinatorError> {
        ServerInitializationCoordinator::open(
            Arc::clone(&self.authentication),
            initialization_token,
            Arc::clone(&self.state),
        )
    }
}

impl std::fmt::Debug for ServerAuthenticationSecurity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServerAuthenticationSecurity")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        panic::{catch_unwind, AssertUnwindSafe},
        path::Path,
        sync::{mpsc, Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    use tempfile::tempdir;

    use super::*;
    use crate::{
        paths::KernelPaths,
        server::{
            AuthenticationFlow, InitializationStatus, RateLimitDecision, RateLimitPolicy,
            RequestIntent, ServerAuthenticationCoordinatorError, ServerOwnerInitializationError,
            SessionPolicy,
        },
    };

    const INITIALIZATION_TOKEN: &str = "injected-random-initialization-token-at-least-32-bytes";
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

    fn security(root: &Path, maximum_in_flight: usize) -> ServerAuthenticationSecurity {
        let paths = fixture_paths(root);
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        let policy =
            RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        ServerAuthenticationSecurity::claim(
            authentication,
            AuthenticationRateLimiter::with_capacity(policy, policy, 8, maximum_in_flight).unwrap(),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        )
        .unwrap()
    }

    fn initialization_token() -> InitializationToken {
        InitializationToken::from_secret(INITIALIZATION_TOKEN.to_owned()).unwrap()
    }

    fn poison<T>(state: &Mutex<T>) {
        let poisoned = catch_unwind(AssertUnwindSafe(|| {
            let _guard = state.lock().unwrap();
            panic!("poison shared authentication security state");
        }));
        assert!(poisoned.is_err());
    }

    #[test]
    fn poisoning_any_process_security_mutex_latches_every_public_authentication_entry_closed() {
        for poisoned_component in 0..3 {
            let temporary = tempdir().unwrap();
            let security = security(temporary.path(), 2);
            let mut initialization = security
                .initialization_coordinator(Some(initialization_token()))
                .unwrap();
            initialization
                .initialize(
                    7,
                    Duration::from_secs(0),
                    INITIALIZATION_TOKEN,
                    OWNER_PASSWORD.to_owned(),
                )
                .unwrap();
            let authentication = security.authentication_coordinator();
            let login = authentication
                .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
                .unwrap();
            match poisoned_component {
                0 => poison(&security.state.rate_limiter),
                1 => poison(&security.state.sessions),
                2 => poison(&security.state.password_lifecycle),
                _ => unreachable!(),
            }

            assert_eq!(initialization.status(), InitializationStatus::Unavailable);
            assert_eq!(
                authentication.authorize(
                    login.session().credential(),
                    None,
                    RequestIntent::ReadOnly,
                    Duration::from_secs(2),
                ),
                Err(ServerAuthenticationCoordinatorError::StateUnavailable)
            );
            assert_eq!(
                authentication.logout(
                    login.session().credential(),
                    Some(login.session().csrf_token()),
                    Duration::from_secs(2),
                ),
                Err(ServerAuthenticationCoordinatorError::StateUnavailable)
            );
            assert_eq!(
                authentication.logout_all(
                    login.session().credential(),
                    Some(login.session().csrf_token()),
                    Duration::from_secs(2),
                ),
                Err(ServerAuthenticationCoordinatorError::StateUnavailable)
            );
            assert_eq!(
                authentication
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
                authentication
                    .login(7, Duration::from_secs(2), OWNER_PASSWORD.to_owned())
                    .unwrap_err(),
                ServerAuthenticationCoordinatorError::StateUnavailable
            );
            assert_eq!(
                initialization
                    .initialize(
                        7,
                        Duration::from_secs(2),
                        INITIALIZATION_TOKEN,
                        OWNER_PASSWORD.to_owned(),
                    )
                    .unwrap_err(),
                ServerOwnerInitializationError::StateUnavailable
            );
        }
    }

    #[test]
    fn poisoned_process_rate_limiter_marks_initialization_unavailable_without_exposing_secrets() {
        let temporary = tempdir().unwrap();
        let security = security(temporary.path(), 1);
        let mut initialization = security
            .initialization_coordinator(Some(initialization_token()))
            .unwrap();
        poison(&security.state.rate_limiter);

        assert_eq!(initialization.status(), InitializationStatus::Unavailable);
        let error = initialization
            .initialize(
                7,
                Duration::from_secs(0),
                INITIALIZATION_TOKEN,
                OWNER_PASSWORD.to_owned(),
            )
            .unwrap_err();
        assert_eq!(error, ServerOwnerInitializationError::StateUnavailable);
        let rendered = format!("{security:?} {initialization:?} {error:?} {error}");
        assert!(!rendered.contains(INITIALIZATION_TOKEN));
        assert!(!rendered.contains(OWNER_PASSWORD));
        assert!(!rendered.contains(temporary.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn poisoned_process_session_owner_prevents_initialization_before_spending_the_token() {
        let temporary = tempdir().unwrap();
        let security = security(temporary.path(), 1);
        let mut initialization = security
            .initialization_coordinator(Some(initialization_token()))
            .unwrap();
        poison(&security.state.sessions);

        assert_eq!(initialization.status(), InitializationStatus::Unavailable);
        assert_eq!(
            initialization
                .initialize(
                    7,
                    Duration::from_secs(0),
                    INITIALIZATION_TOKEN,
                    OWNER_PASSWORD.to_owned(),
                )
                .unwrap_err(),
            ServerOwnerInitializationError::StateUnavailable
        );
        assert!(!temporary.path().join("config/owner-auth-v1.json").exists());
    }

    #[test]
    fn concurrent_login_holds_the_only_process_permit_before_initialization_admission() {
        let temporary = tempdir().unwrap();
        let security = security(temporary.path(), 1);
        let authentication = security.authentication_coordinator();
        let initialization = security
            .initialization_coordinator(Some(initialization_token()))
            .unwrap();
        let lifecycle = security.state.password_lifecycle.lock().unwrap();
        let login_worker = thread::spawn(move || {
            authentication.login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let decision = security.state.rate_limiter.lock().unwrap().begin_attempt(
                AuthenticationFlow::Initialization,
                99,
                Duration::from_secs(0),
            );
            match decision {
                Err(RateLimitDecision::AtCapacity) => break,
                Ok(probe) => drop(probe),
                Err(other) => panic!("unexpected admission decision: {other:?}"),
            }
            assert!(Instant::now() < deadline, "login never acquired admission");
            thread::yield_now();
        }

        let (sender, receiver) = mpsc::sync_channel(1);
        let initialization_worker = thread::spawn(move || {
            let mut initialization = initialization;
            let result = initialization.initialize(
                8,
                Duration::from_secs(1),
                INITIALIZATION_TOKEN,
                OWNER_PASSWORD.to_owned(),
            );
            sender.send((result, initialization)).unwrap();
        });
        let early = receiver.recv_timeout(Duration::from_secs(2));
        drop(lifecycle);
        let (result, mut initialization) = early.unwrap_or_else(|_| {
            receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("initialization did not settle after lifecycle release")
        });

        assert_eq!(result, Err(ServerOwnerInitializationError::AtCapacity));
        assert_eq!(
            login_worker.join().unwrap().unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
        initialization_worker.join().unwrap();
        initialization
            .initialize(
                8,
                Duration::from_secs(2),
                INITIALIZATION_TOKEN,
                OWNER_PASSWORD.to_owned(),
            )
            .unwrap();
    }
}
