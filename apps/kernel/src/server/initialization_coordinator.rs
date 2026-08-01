use std::{
    fmt,
    sync::{Arc, MutexGuard},
    time::Duration,
};

use super::secret::ServerAuthenticationSecret;
use super::security::AuthenticationSecurityState;

use super::{
    AuthenticationAttemptPermit, AuthenticationFlow, AuthenticationRateLimiter,
    InitializationError, InitializationGate, InitializationStatus, InitializationToken,
    InvalidAuthenticationAttempt, OwnerPasswordInitializationError, RateLimitDecision,
    ServerAuthenticationStatus, ServerAuthenticationStore,
};

pub struct ServerInitializationCoordinator {
    authentication: Arc<ServerAuthenticationStore>,
    security: Arc<AuthenticationSecurityState>,
    gate: InitializationGate,
    state_unavailable: bool,
}

impl ServerInitializationCoordinator {
    pub(crate) fn open(
        authentication: Arc<ServerAuthenticationStore>,
        initialization_token: Option<InitializationToken>,
        security: Arc<AuthenticationSecurityState>,
    ) -> Result<Self, ServerInitializationCoordinatorError> {
        if authentication.is_state_uncertain() || !security.is_available() {
            security.fail_closed();
            return Err(ServerInitializationCoordinatorError::StateUnavailable);
        }
        let status = authentication.status().map_err(|_| {
            security.fail_closed();
            ServerInitializationCoordinatorError::StateUnavailable
        })?;
        let gate = match status {
            ServerAuthenticationStatus::Ready => InitializationGate::initialized(),
            ServerAuthenticationStatus::NeedsInitialization => InitializationGate::pending(
                initialization_token
                    .ok_or(ServerInitializationCoordinatorError::MissingInitializationToken)?,
            ),
        };
        Ok(Self {
            authentication,
            security,
            gate,
            state_unavailable: false,
        })
    }

    pub fn status(&self) -> InitializationStatus {
        if self.authentication.is_state_uncertain() {
            self.security.fail_closed();
        }
        if self.state_unavailable || !self.security.is_available() {
            return InitializationStatus::Unavailable;
        }
        let status = self.gate.status();
        if status == InitializationStatus::Unavailable {
            self.security.fail_closed();
        }
        status
    }

    #[cfg(test)]
    pub(crate) fn poison_gate_for_test(&self) {
        self.gate.poison_for_test();
    }

    #[cfg(test)]
    pub(crate) fn poison_gate_commit_for_test(&mut self) {
        self.gate.poison_commit_for_test();
    }

    #[cfg(test)]
    pub(crate) fn poison_gate_abort_for_test(&mut self) {
        self.gate.poison_abort_for_test();
    }

    /// Initializes the fixed single-user owner.
    ///
    /// Persistent Argon2id state is always published before the process-local
    /// one-time gate commits. Once persistent state exists it remains
    /// authoritative even if the request receives an uncertain result.
    pub fn initialize(
        &mut self,
        client_id: u64,
        now: Duration,
        candidate_token: &str,
        owner_password: impl Into<ServerAuthenticationSecret>,
    ) -> Result<(), ServerOwnerInitializationError> {
        let owner_password = owner_password.into();
        if self.authentication.is_state_uncertain() {
            self.security.fail_closed();
        }
        if self.state_unavailable || !self.security.is_available() {
            return Err(ServerOwnerInitializationError::StateUnavailable);
        }
        let now = self.observe_time(now)?;
        let authentication_permit = {
            let mut limiter = self.lock_rate_limiter()?;
            limiter
                .begin_attempt(AuthenticationFlow::Initialization, client_id, now)
                .map_err(map_rate_limit_decision)?
        };
        let permit = match self.gate.begin(candidate_token) {
            Ok(permit) => permit,
            Err(InitializationError::InvalidToken) => {
                self.record_failure(authentication_permit)?;
                return Err(ServerOwnerInitializationError::InvalidToken);
            }
            Err(
                error @ (InitializationError::InProgress | InitializationError::AlreadyInitialized),
            ) => {
                drop(authentication_permit);
                return Err(map_gate_error(error));
            }
            Err(InitializationError::InvalidPermit | InitializationError::StateUnavailable) => {
                let settlement = self.settle_unavailable_attempt(authentication_permit);
                self.security.fail_closed();
                settlement?;
                return Err(ServerOwnerInitializationError::StateUnavailable);
            }
        };
        let security = Arc::clone(&self.security);
        if !security.is_available() {
            let abort = self.abort_or_fail(permit);
            let settlement = self.settle_unavailable_attempt(authentication_permit);
            abort?;
            settlement?;
            return Err(ServerOwnerInitializationError::StateUnavailable);
        }
        let _password_lifecycle = match security.password_lifecycle.lock() {
            Ok(lifecycle) => lifecycle,
            Err(_poisoned) => {
                let abort = self.abort_or_fail(permit);
                let settlement = self.settle_unavailable_attempt(authentication_permit);
                security.fail_closed();
                abort?;
                settlement?;
                return Err(ServerOwnerInitializationError::StateUnavailable);
            }
        };
        match self
            .authentication
            .initialize_owner_password(owner_password)
        {
            Ok(()) => {
                if self.gate.commit(permit).is_err() {
                    let settlement = self.record_success(authentication_permit);
                    self.security.fail_closed();
                    settlement?;
                    return Err(ServerOwnerInitializationError::StateUnavailable);
                }
                self.record_success(authentication_permit)?;
                Ok(())
            }
            Err(OwnerPasswordInitializationError::InvalidPassword) => {
                let abort = self.abort_or_fail(permit);
                let settlement = self.record_success(authentication_permit);
                abort?;
                settlement?;
                Err(ServerOwnerInitializationError::InvalidPassword)
            }
            Err(OwnerPasswordInitializationError::AlreadyInitialized) => {
                drop(permit);
                let reconciliation = self.reconcile_persistent_state();
                let settlement = self.record_success(authentication_permit);
                reconciliation?;
                settlement?;
                Err(ServerOwnerInitializationError::AlreadyInitialized)
            }
            Err(OwnerPasswordInitializationError::StateUnavailable) => {
                let abort = self.abort_or_fail(permit);
                let settlement = self.settle_unavailable_attempt(authentication_permit);
                abort?;
                settlement?;
                Err(ServerOwnerInitializationError::StateUnavailable)
            }
            Err(OwnerPasswordInitializationError::StateUncertain) => {
                drop(permit);
                match self.authentication.reconcile_uncertain_initialization() {
                    Ok(ServerAuthenticationStatus::Ready) => {
                        self.gate = InitializationGate::initialized();
                    }
                    Ok(ServerAuthenticationStatus::NeedsInitialization) | Err(_) => {
                        self.state_unavailable = true;
                    }
                }
                let settlement = self.record_success(authentication_permit);
                self.security.fail_closed();
                settlement?;
                Err(ServerOwnerInitializationError::StateUncertain)
            }
        }
    }

    fn record_failure(
        &self,
        permit: AuthenticationAttemptPermit,
    ) -> Result<(), ServerOwnerInitializationError> {
        let mut limiter = self.lock_rate_limiter()?;
        let now = self.observe_fresh_time()?;
        match limiter
            .record_failure(permit, now)
            .map_err(map_invalid_attempt)?
        {
            RateLimitDecision::Allowed => Ok(()),
            RateLimitDecision::Limited { retry_after } => {
                Err(ServerOwnerInitializationError::RateLimited { retry_after })
            }
            RateLimitDecision::AtCapacity => Err(ServerOwnerInitializationError::StateUnavailable),
        }
    }

    fn record_success(
        &self,
        permit: AuthenticationAttemptPermit,
    ) -> Result<(), ServerOwnerInitializationError> {
        self.lock_rate_limiter()?
            .record_success(permit)
            .map_err(map_invalid_attempt)
    }

    fn settle_unavailable_attempt(
        &self,
        permit: AuthenticationAttemptPermit,
    ) -> Result<(), ServerOwnerInitializationError> {
        match self.record_failure(permit) {
            Ok(()) | Err(ServerOwnerInitializationError::RateLimited { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn lock_rate_limiter(
        &self,
    ) -> Result<MutexGuard<'_, AuthenticationRateLimiter>, ServerOwnerInitializationError> {
        if !self.security.is_available() {
            return Err(ServerOwnerInitializationError::StateUnavailable);
        }
        self.security.rate_limiter.lock().map_err(|_poisoned| {
            self.security.fail_closed();
            ServerOwnerInitializationError::StateUnavailable
        })
    }

    fn abort_or_fail(
        &mut self,
        permit: super::InitializationPermit,
    ) -> Result<(), ServerOwnerInitializationError> {
        self.gate.abort(permit).map_err(|_| {
            self.security.fail_closed();
            ServerOwnerInitializationError::StateUnavailable
        })
    }

    fn reconcile_persistent_state(&mut self) -> Result<(), ServerOwnerInitializationError> {
        match self.authentication.status() {
            Ok(ServerAuthenticationStatus::Ready) => {
                self.gate = InitializationGate::initialized();
                Ok(())
            }
            Ok(ServerAuthenticationStatus::NeedsInitialization) | Err(_) => {
                self.security.fail_closed();
                Err(ServerOwnerInitializationError::StateUnavailable)
            }
        }
    }

    fn observe_time(
        &self,
        candidate: Duration,
    ) -> Result<Duration, ServerOwnerInitializationError> {
        self.security.observe_time(candidate).map_err(|()| {
            self.security.fail_closed();
            ServerOwnerInitializationError::StateUnavailable
        })
    }

    fn observe_fresh_time(&self) -> Result<Duration, ServerOwnerInitializationError> {
        self.security.fresh_time().map_err(|()| {
            self.security.fail_closed();
            ServerOwnerInitializationError::StateUnavailable
        })
    }
}

impl fmt::Debug for ServerInitializationCoordinator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerInitializationCoordinator")
            .field("status", &self.status())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerInitializationCoordinatorError {
    MissingInitializationToken,
    StateUnavailable,
}

impl fmt::Display for ServerInitializationCoordinatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingInitializationToken => "server initialization token is unavailable",
            Self::StateUnavailable => "server initialization state is unavailable",
        })
    }
}

impl std::error::Error for ServerInitializationCoordinatorError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerOwnerInitializationError {
    InvalidToken,
    InProgress,
    AlreadyInitialized,
    InvalidPassword,
    RateLimited { retry_after: Duration },
    AtCapacity,
    StateUnavailable,
    StateUncertain,
}

impl fmt::Display for ServerOwnerInitializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidToken => "server initialization is not authorized",
            Self::InProgress => "server initialization is already in progress",
            Self::AlreadyInitialized => "server initialization is already complete",
            Self::InvalidPassword => "owner password is invalid",
            Self::RateLimited { .. } => "server initialization is temporarily limited",
            Self::AtCapacity => "server authentication capacity is exhausted",
            Self::StateUnavailable => "server initialization state is unavailable",
            Self::StateUncertain => "server initialization publication is uncertain",
        })
    }
}

impl std::error::Error for ServerOwnerInitializationError {}

fn map_gate_error(error: InitializationError) -> ServerOwnerInitializationError {
    match error {
        InitializationError::InvalidToken => ServerOwnerInitializationError::InvalidToken,
        InitializationError::InProgress => ServerOwnerInitializationError::InProgress,
        InitializationError::AlreadyInitialized => {
            ServerOwnerInitializationError::AlreadyInitialized
        }
        InitializationError::InvalidPermit | InitializationError::StateUnavailable => {
            ServerOwnerInitializationError::StateUnavailable
        }
    }
}

fn map_rate_limit_decision(decision: RateLimitDecision) -> ServerOwnerInitializationError {
    match decision {
        RateLimitDecision::Limited { retry_after } => {
            ServerOwnerInitializationError::RateLimited { retry_after }
        }
        RateLimitDecision::AtCapacity => ServerOwnerInitializationError::AtCapacity,
        RateLimitDecision::Allowed => ServerOwnerInitializationError::StateUnavailable,
    }
}

fn map_invalid_attempt(_: InvalidAuthenticationAttempt) -> ServerOwnerInitializationError {
    ServerOwnerInitializationError::StateUnavailable
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::Path,
        sync::{Arc, Mutex},
    };

    use tempfile::tempdir;

    use super::*;
    use crate::{
        paths::KernelPaths,
        server::{RateLimitPolicy, SessionPolicy, SessionStore},
        storage::DurableFileTestFault,
    };

    const INITIALIZATION_TOKEN: &str = "injected-random-initialization-token-at-least-32-bytes";
    const OWNER_PASSWORD: &str = "Correct-Horse-Battery-Staple!7";

    struct ScriptedAuthenticationTimeSource {
        samples: Mutex<std::collections::VecDeque<Duration>>,
    }

    impl ScriptedAuthenticationTimeSource {
        fn new(samples: impl IntoIterator<Item = Duration>) -> Self {
            Self {
                samples: Mutex::new(samples.into_iter().collect()),
            }
        }
    }

    impl super::super::security::AuthenticationTimeSource for ScriptedAuthenticationTimeSource {
        fn elapsed(&self) -> Duration {
            self.samples
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected authentication time sample")
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

    fn coordinator_with_fault(
        paths: &KernelPaths,
        fault: DurableFileTestFault,
    ) -> (
        Arc<ServerAuthenticationStore>,
        ServerInitializationCoordinator,
    ) {
        let authentication = Arc::new(
            ServerAuthenticationStore::open_with_test_fault(paths.config_root(), fault).unwrap(),
        );
        let policy =
            RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let coordinator = ServerInitializationCoordinator::open(
            Arc::clone(&authentication),
            Some(InitializationToken::from_secret(INITIALIZATION_TOKEN.to_owned()).unwrap()),
            Arc::new(AuthenticationSecurityState::new(
                AuthenticationRateLimiter::new(policy, policy),
                SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
            )),
        )
        .unwrap();
        (authentication, coordinator)
    }

    #[test]
    fn initialization_failure_lockout_starts_when_attempt_processing_finishes() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        let policy =
            RateLimitPolicy::new(1, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        let source = Arc::new(ScriptedAuthenticationTimeSource::new([
            Duration::ZERO,
            Duration::ZERO,
            Duration::from_secs(40),
        ]));
        let security = Arc::new(AuthenticationSecurityState::new_with_time_source(
            AuthenticationRateLimiter::new(policy, policy),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
            source,
        ));
        let mut coordinator = ServerInitializationCoordinator::open(
            authentication,
            Some(InitializationToken::from_secret(INITIALIZATION_TOKEN.to_owned()).unwrap()),
            Arc::clone(&security),
        )
        .unwrap();

        assert_eq!(
            coordinator
                .initialize(
                    7,
                    Duration::ZERO,
                    "incorrect initialization token material",
                    OWNER_PASSWORD.to_owned(),
                )
                .unwrap_err(),
            ServerOwnerInitializationError::RateLimited {
                retry_after: Duration::from_secs(30),
            }
        );
        assert_eq!(
            security
                .rate_limiter
                .lock()
                .unwrap()
                .begin_attempt(
                    AuthenticationFlow::Initialization,
                    7,
                    Duration::from_secs(41),
                )
                .unwrap_err(),
            RateLimitDecision::Limited {
                retry_after: Duration::from_secs(29),
            }
        );
    }

    #[test]
    fn published_but_uncertain_owner_state_is_authoritative_without_unlocking_authentication() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let (authentication, mut coordinator) =
            coordinator_with_fault(&paths, DurableFileTestFault::ParentSyncFailure);

        assert_eq!(
            coordinator
                .initialize(
                    7,
                    Duration::from_secs(0),
                    INITIALIZATION_TOKEN,
                    OWNER_PASSWORD.to_owned(),
                )
                .unwrap_err(),
            ServerOwnerInitializationError::StateUncertain
        );
        assert_eq!(coordinator.status(), InitializationStatus::Unavailable);
        assert!(authentication.status().is_err());
        assert!(authentication
            .verify_owner_password(OWNER_PASSWORD)
            .is_err());
        assert!(temporary.path().join("config/owner-auth-v1.json").exists());
    }

    #[test]
    fn unpublished_uncertain_owner_state_never_returns_to_retryable_pending() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let (authentication, mut coordinator) =
            coordinator_with_fault(&paths, DurableFileTestFault::LeavePrepared);

        assert_eq!(
            coordinator
                .initialize(
                    7,
                    Duration::from_secs(0),
                    INITIALIZATION_TOKEN,
                    OWNER_PASSWORD.to_owned(),
                )
                .unwrap_err(),
            ServerOwnerInitializationError::StateUncertain
        );
        assert_eq!(coordinator.status(), InitializationStatus::Unavailable);
        assert_eq!(
            coordinator
                .initialize(
                    7,
                    Duration::from_secs(1),
                    INITIALIZATION_TOKEN,
                    OWNER_PASSWORD.to_owned(),
                )
                .unwrap_err(),
            ServerOwnerInitializationError::StateUnavailable
        );
        assert!(authentication.status().is_err());
        assert!(!temporary.path().join("config/owner-auth-v1.json").exists());
    }
}
