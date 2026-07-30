use std::{fmt, sync::Arc};

use super::{
    InitializationError, InitializationGate, InitializationStatus, InitializationToken,
    OwnerPasswordInitializationError, ServerAuthenticationStatus, ServerAuthenticationStore,
};

pub struct ServerInitializationCoordinator {
    authentication: Arc<ServerAuthenticationStore>,
    gate: InitializationGate,
    state_unavailable: bool,
}

impl ServerInitializationCoordinator {
    pub fn open(
        authentication: Arc<ServerAuthenticationStore>,
        initialization_token: Option<InitializationToken>,
    ) -> Result<Self, ServerInitializationCoordinatorError> {
        let status = authentication
            .status()
            .map_err(|_| ServerInitializationCoordinatorError::StateUnavailable)?;
        let gate = match status {
            ServerAuthenticationStatus::Ready => InitializationGate::initialized(),
            ServerAuthenticationStatus::NeedsInitialization => InitializationGate::pending(
                initialization_token
                    .ok_or(ServerInitializationCoordinatorError::MissingInitializationToken)?,
            ),
        };
        Ok(Self {
            authentication,
            gate,
            state_unavailable: false,
        })
    }

    pub fn status(&self) -> InitializationStatus {
        if self.state_unavailable {
            InitializationStatus::Unavailable
        } else {
            self.gate.status()
        }
    }

    /// Initializes the fixed single-user owner.
    ///
    /// Persistent Argon2id state is always published before the process-local
    /// one-time gate commits. Once persistent state exists it remains
    /// authoritative even if the request receives an uncertain result.
    pub fn initialize(
        &mut self,
        candidate_token: &str,
        owner_password: String,
    ) -> Result<(), ServerOwnerInitializationError> {
        if self.state_unavailable {
            return Err(ServerOwnerInitializationError::StateUnavailable);
        }
        let permit = self.gate.begin(candidate_token).map_err(map_gate_error)?;
        match self
            .authentication
            .initialize_owner_password(owner_password)
        {
            Ok(()) => {
                if self.gate.commit(permit).is_err() {
                    self.gate = InitializationGate::initialized();
                    return Err(ServerOwnerInitializationError::StateUnavailable);
                }
                Ok(())
            }
            Err(OwnerPasswordInitializationError::InvalidPassword) => {
                self.abort_or_fail(permit)?;
                Err(ServerOwnerInitializationError::InvalidPassword)
            }
            Err(OwnerPasswordInitializationError::AlreadyInitialized) => {
                drop(permit);
                self.reconcile_persistent_state()?;
                Err(ServerOwnerInitializationError::AlreadyInitialized)
            }
            Err(OwnerPasswordInitializationError::StateUnavailable) => {
                self.abort_or_fail(permit)?;
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
                Err(ServerOwnerInitializationError::StateUncertain)
            }
        }
    }

    fn abort_or_fail(
        &mut self,
        permit: super::InitializationPermit,
    ) -> Result<(), ServerOwnerInitializationError> {
        self.gate
            .abort(permit)
            .map_err(|_| ServerOwnerInitializationError::StateUnavailable)
    }

    fn reconcile_persistent_state(&mut self) -> Result<(), ServerOwnerInitializationError> {
        match self.authentication.status() {
            Ok(ServerAuthenticationStatus::Ready) => {
                self.gate = InitializationGate::initialized();
                Ok(())
            }
            Ok(ServerAuthenticationStatus::NeedsInitialization) | Err(_) => {
                Err(ServerOwnerInitializationError::StateUnavailable)
            }
        }
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

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use tempfile::tempdir;

    use super::*;
    use crate::{paths::KernelPaths, storage::DurableFileTestFault};

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
        let coordinator = ServerInitializationCoordinator::open(
            Arc::clone(&authentication),
            Some(InitializationToken::from_secret(INITIALIZATION_TOKEN.to_owned()).unwrap()),
        )
        .unwrap();
        (authentication, coordinator)
    }

    #[test]
    fn published_but_uncertain_owner_state_is_authoritative_without_unlocking_authentication() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let (authentication, mut coordinator) =
            coordinator_with_fault(&paths, DurableFileTestFault::ParentSyncFailure);

        assert_eq!(
            coordinator
                .initialize(INITIALIZATION_TOKEN, OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerOwnerInitializationError::StateUncertain
        );
        assert_eq!(coordinator.status(), InitializationStatus::Initialized);
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
                .initialize(INITIALIZATION_TOKEN, OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerOwnerInitializationError::StateUncertain
        );
        assert_eq!(coordinator.status(), InitializationStatus::Unavailable);
        assert_eq!(
            coordinator
                .initialize(INITIALIZATION_TOKEN, OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            ServerOwnerInitializationError::StateUnavailable
        );
        assert!(authentication.status().is_err());
        assert!(!temporary.path().join("config/owner-auth-v1.json").exists());
    }
}
