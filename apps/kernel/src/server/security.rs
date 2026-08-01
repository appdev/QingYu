use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use super::{
    AuthenticationRateLimiter, InitializationToken, ServerAuthenticationCoordinator,
    ServerAuthenticationError, ServerAuthenticationStore, ServerInitializationCoordinator,
    ServerInitializationCoordinatorError, SessionStore,
};

pub(crate) trait AuthenticationTimeSource: Send + Sync {
    fn elapsed(&self) -> Duration;
}

struct SystemAuthenticationTimeSource {
    origin: Instant,
}

impl SystemAuthenticationTimeSource {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl AuthenticationTimeSource for SystemAuthenticationTimeSource {
    fn elapsed(&self) -> Duration {
        self.origin.elapsed()
    }
}

struct AuthenticationTimeState {
    source_at: Duration,
    logical_now: Duration,
}

struct AuthenticationTimekeeper {
    source: Arc<dyn AuthenticationTimeSource>,
    state: Mutex<AuthenticationTimeState>,
}

impl AuthenticationTimekeeper {
    fn production() -> Self {
        Self::with_source(Arc::new(SystemAuthenticationTimeSource::new()))
    }

    fn with_source(source: Arc<dyn AuthenticationTimeSource>) -> Self {
        let source_at = source.elapsed();
        Self {
            source,
            state: Mutex::new(AuthenticationTimeState {
                source_at,
                logical_now: Duration::ZERO,
            }),
        }
    }

    fn observe(&self, candidate: Duration) -> Result<Duration, ()> {
        let source_now = self.source.elapsed();
        let mut state = self.state.lock().map_err(|_poisoned| ())?;
        // An older sample can arrive after a concurrent observer has already
        // committed a newer one. Never move the source anchor backwards or
        // count that elapsed interval a second time.
        let source_at = state.source_at.max(source_now);
        let projected = state
            .logical_now
            .saturating_add(source_at.saturating_sub(state.source_at));
        state.logical_now = projected.max(candidate);
        state.source_at = source_at;
        Ok(state.logical_now)
    }

    fn fresh(&self) -> Result<Duration, ()> {
        self.observe(Duration::ZERO)
    }

    fn is_poisoned(&self) -> bool {
        self.state.is_poisoned()
    }
}

pub(crate) struct AuthenticationSecurityState {
    pub(crate) rate_limiter: Mutex<AuthenticationRateLimiter>,
    pub(crate) sessions: Mutex<SessionStore>,
    pub(crate) password_lifecycle: Mutex<()>,
    pub(crate) session_mutation_lifecycle: Mutex<()>,
    timekeeper: AuthenticationTimekeeper,
    failed_closed: AtomicBool,
    _owner_claim: Option<Arc<()>>,
}

impl AuthenticationSecurityState {
    #[cfg(test)]
    pub(crate) fn new(rate_limiter: AuthenticationRateLimiter, sessions: SessionStore) -> Self {
        Self::with_owner_claim_and_timekeeper(
            rate_limiter,
            sessions,
            None,
            AuthenticationTimekeeper::production(),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_time_source(
        rate_limiter: AuthenticationRateLimiter,
        sessions: SessionStore,
        source: Arc<dyn AuthenticationTimeSource>,
    ) -> Self {
        Self::with_owner_claim_and_timekeeper(
            rate_limiter,
            sessions,
            None,
            AuthenticationTimekeeper::with_source(source),
        )
    }

    fn with_owner_claim(
        rate_limiter: AuthenticationRateLimiter,
        sessions: SessionStore,
        owner_claim: Option<Arc<()>>,
    ) -> Self {
        Self::with_owner_claim_and_timekeeper(
            rate_limiter,
            sessions,
            owner_claim,
            AuthenticationTimekeeper::production(),
        )
    }

    fn with_owner_claim_and_timekeeper(
        rate_limiter: AuthenticationRateLimiter,
        sessions: SessionStore,
        owner_claim: Option<Arc<()>>,
        timekeeper: AuthenticationTimekeeper,
    ) -> Self {
        Self {
            rate_limiter: Mutex::new(rate_limiter),
            sessions: Mutex::new(sessions),
            password_lifecycle: Mutex::new(()),
            session_mutation_lifecycle: Mutex::new(()),
            timekeeper,
            failed_closed: AtomicBool::new(false),
            _owner_claim: owner_claim,
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        if self.failed_closed.load(Ordering::Acquire) {
            return false;
        }
        let available = !self.rate_limiter.is_poisoned()
            && !self.sessions.is_poisoned()
            && !self.password_lifecycle.is_poisoned()
            && !self.session_mutation_lifecycle.is_poisoned()
            && !self.timekeeper.is_poisoned();
        if !available {
            self.fail_closed();
        }
        available
    }

    pub(crate) fn fail_closed(&self) {
        self.failed_closed.store(true, Ordering::Release);
    }

    pub(crate) fn observe_time(&self, candidate: Duration) -> Result<Duration, ()> {
        self.timekeeper.observe(candidate)
    }

    pub(crate) fn fresh_time(&self) -> Result<Duration, ()> {
        self.timekeeper.fresh()
    }

    #[cfg(test)]
    pub(crate) fn poison_timekeeper_for_test(&self) {
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.timekeeper.state.lock().unwrap();
            panic!("poison authentication timekeeper for coordinator test");
        }));
        assert!(poisoned.is_err());
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
            state: Arc::new(AuthenticationSecurityState::with_owner_claim(
                rate_limiter,
                sessions,
                Some(owner_claim),
            )),
        })
    }

    pub fn authentication_coordinator(&self) -> ServerAuthenticationCoordinator {
        ServerAuthenticationCoordinator::from_security(
            Arc::clone(&self.authentication),
            Arc::clone(&self.state),
        )
    }

    pub(crate) fn matches_config_root(&self, candidate: &crate::paths::ConfigRoot) -> bool {
        self.authentication.matches_config_root(candidate)
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
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc, Arc, Mutex,
        },
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
    const OWNER_PASSWORD: &str = "Correct-Horse-Battery-Staple!7";

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

    struct DelayedOldAuthenticationTimeSource {
        call: AtomicUsize,
        old_sample_captured: mpsc::SyncSender<()>,
        release_old_sample: Mutex<mpsc::Receiver<()>>,
    }

    impl AuthenticationTimeSource for DelayedOldAuthenticationTimeSource {
        fn elapsed(&self) -> Duration {
            match self.call.fetch_add(1, Ordering::AcqRel) {
                0 => Duration::ZERO,
                1 => {
                    self.old_sample_captured.send(()).unwrap();
                    self.release_old_sample.lock().unwrap().recv().unwrap();
                    Duration::from_secs(10)
                }
                2 => Duration::from_secs(20),
                3 => Duration::from_secs(21),
                call => panic!("unexpected authentication time sample {call}"),
            }
        }
    }

    #[test]
    fn delayed_concurrent_time_sample_cannot_regress_or_double_count_elapsed_time() {
        let (captured_sender, captured_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let source = Arc::new(DelayedOldAuthenticationTimeSource {
            call: AtomicUsize::new(0),
            old_sample_captured: captured_sender,
            release_old_sample: Mutex::new(release_receiver),
        });
        let timekeeper = Arc::new(AuthenticationTimekeeper::with_source(source));
        let delayed_timekeeper = Arc::clone(&timekeeper);
        let delayed = thread::spawn(move || delayed_timekeeper.fresh().unwrap());
        captured_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("old time sample was not captured");

        assert_eq!(timekeeper.fresh().unwrap(), Duration::from_secs(20));
        release_sender.send(()).unwrap();
        assert_eq!(delayed.join().unwrap(), Duration::from_secs(20));
        assert_eq!(timekeeper.fresh().unwrap(), Duration::from_secs(21));
    }

    #[test]
    fn poisoning_any_process_security_mutex_latches_every_public_authentication_entry_closed() {
        for poisoned_component in 0..5 {
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
                3 => poison(&security.state.session_mutation_lifecycle),
                4 => poison(&security.state.timekeeper.state),
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
                        "New-Owner-Password-Material!8".to_owned(),
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
    fn poisoned_initialization_gate_status_latches_every_authentication_entry_closed() {
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
        initialization.poison_gate_for_test();

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
    }

    #[test]
    fn poisoned_initialization_gate_begin_latches_every_authentication_entry_closed() {
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
        initialization.poison_gate_for_test();

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
        assert_eq!(
            authentication.authorize(
                login.session().credential(),
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(3),
            ),
            Err(ServerAuthenticationCoordinatorError::StateUnavailable)
        );
    }

    #[test]
    fn poisoned_initialization_gate_commit_latches_every_authentication_entry_closed() {
        let temporary = tempdir().unwrap();
        let security = security(temporary.path(), 2);
        let mut initialization = security
            .initialization_coordinator(Some(initialization_token()))
            .unwrap();
        let authentication = security.authentication_coordinator();
        initialization.poison_gate_commit_for_test();

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
        assert_eq!(
            authentication
                .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
    }

    #[test]
    fn poisoned_initialization_gate_abort_latches_every_authentication_entry_closed() {
        let temporary = tempdir().unwrap();
        let security = security(temporary.path(), 2);
        let mut initialization = security
            .initialization_coordinator(Some(initialization_token()))
            .unwrap();
        let authentication = security.authentication_coordinator();
        initialization.poison_gate_abort_for_test();

        assert_eq!(
            initialization
                .initialize(
                    7,
                    Duration::from_secs(0),
                    INITIALIZATION_TOKEN,
                    "contains space".to_owned(),
                )
                .unwrap_err(),
            ServerOwnerInitializationError::StateUnavailable
        );
        assert_eq!(
            authentication
                .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerAuthenticationCoordinatorError::StateUnavailable
        );
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
