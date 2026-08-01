use std::{env, ffi::OsString, fmt};

use crate::paths::{KernelPaths, ServerPathLayout};

use super::{
    InitializationToken, ServerAuthenticationSecurity, ServerInitializationCoordinator,
    ServerInitializationCoordinatorError,
};

pub const SERVER_INITIALIZATION_TOKEN_ENV: &str = "QINGYU_SERVER_INITIALIZATION_TOKEN";

/// Validated, process-launch-only inputs for the fixed single-user server host.
///
/// The server data layout is deliberately not an environment input. Consuming
/// this value transfers any supplied initialization token directly into the
/// durable initialization owner without exposing a token accessor or clone.
pub struct ServerLaunchEnvironment {
    initialization_token: Option<InitializationToken>,
}

impl ServerLaunchEnvironment {
    pub fn load() -> Result<Self, ServerLaunchEnvironmentError> {
        Self::from_lookup(|name| env::var_os(name))
    }

    pub fn from_lookup(
        mut lookup: impl FnMut(&str) -> Option<OsString>,
    ) -> Result<Self, ServerLaunchEnvironmentError> {
        let initialization_token = match lookup(SERVER_INITIALIZATION_TOKEN_ENV) {
            Some(value) => {
                let secret = value.into_string().map_err(|_non_unicode| {
                    ServerLaunchEnvironmentError::NonUnicodeInitializationToken
                })?;
                Some(
                    InitializationToken::from_secret(secret).map_err(|_invalid| {
                        ServerLaunchEnvironmentError::InvalidInitializationToken
                    })?,
                )
            }
            None => None,
        };
        Ok(Self {
            initialization_token,
        })
    }

    pub const fn layout(&self) -> ServerPathLayout {
        KernelPaths::server()
    }

    pub fn into_initialization_owner(
        self,
        security: &ServerAuthenticationSecurity,
    ) -> Result<ServerInitializationCoordinator, ServerInitializationCoordinatorError> {
        security.initialization_coordinator(self.initialization_token)
    }
}

impl fmt::Debug for ServerLaunchEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerLaunchEnvironment")
            .field("layout", &"KernelPaths::server()")
            .field("initialization_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerLaunchEnvironmentError {
    NonUnicodeInitializationToken,
    InvalidInitializationToken,
}

impl fmt::Display for ServerLaunchEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonUnicodeInitializationToken => {
                "server initialization token environment encoding is invalid"
            }
            Self::InvalidInitializationToken => {
                "server initialization token environment value is invalid"
            }
        })
    }
}

impl std::error::Error for ServerLaunchEnvironmentError {}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, ffi::OsString, fs, path::Path, sync::Arc, time::Duration};

    use tempfile::tempdir;

    use super::*;
    use crate::{
        paths::KernelPaths,
        server::{
            AuthenticationRateLimiter, InitializationStatus, RateLimitPolicy,
            ServerAuthenticationSecurity, ServerAuthenticationStore,
            ServerInitializationCoordinatorError, ServerOwnerInitializationError, SessionPolicy,
            SessionStore,
        },
    };

    const INITIALIZATION_TOKEN: &str = "injected-random-initialization-token-at-least-32-bytes";
    const OWNER_PASSWORD: &str = "Correct-Horse-Battery-Staple!7";

    fn environment_with(
        entries: impl IntoIterator<Item = (&'static str, OsString)>,
    ) -> Result<ServerLaunchEnvironment, ServerLaunchEnvironmentError> {
        let values = entries
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<HashMap<_, _>>();
        ServerLaunchEnvironment::from_lookup(|name| values.get(name).cloned())
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

    fn security(authentication: Arc<ServerAuthenticationStore>) -> ServerAuthenticationSecurity {
        let policy =
            RateLimitPolicy::new(5, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
        ServerAuthenticationSecurity::claim(
            authentication,
            AuthenticationRateLimiter::new(policy, policy),
            SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn missing_initialization_token_fails_closed_for_an_uninitialized_owner() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        let security = security(authentication);
        let environment = environment_with([]).unwrap();

        assert_eq!(
            environment
                .into_initialization_owner(&security)
                .unwrap_err(),
            ServerInitializationCoordinatorError::MissingInitializationToken
        );
    }

    #[test]
    fn initialized_owner_restarts_without_retaining_the_one_time_token() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        authentication
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let security = security(authentication);
        let environment = environment_with([]).unwrap();

        let owner = environment.into_initialization_owner(&security).unwrap();

        assert_eq!(owner.status(), InitializationStatus::Initialized);
    }

    #[test]
    fn empty_or_short_initialization_token_fails_closed() {
        for candidate in ["", "short"] {
            assert_eq!(
                environment_with([(SERVER_INITIALIZATION_TOKEN_ENV, OsString::from(candidate),)])
                    .unwrap_err(),
                ServerLaunchEnvironmentError::InvalidInitializationToken
            );
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn non_unicode_initialization_token_fails_closed() {
        assert_eq!(
            environment_with([(
                SERVER_INITIALIZATION_TOKEN_ENV,
                non_unicode_environment_value(),
            )])
            .unwrap_err(),
            ServerLaunchEnvironmentError::NonUnicodeInitializationToken
        );
    }

    #[test]
    fn valid_token_is_consumed_into_the_single_initialization_owner() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        let environment = environment_with([(
            SERVER_INITIALIZATION_TOKEN_ENV,
            OsString::from(INITIALIZATION_TOKEN),
        )])
        .unwrap();
        let security = security(authentication);

        let mut owner = environment.into_initialization_owner(&security).unwrap();

        assert_eq!(owner.status(), InitializationStatus::Pending);
        owner
            .initialize(
                7,
                Duration::from_secs(0),
                INITIALIZATION_TOKEN,
                OWNER_PASSWORD.to_owned(),
            )
            .unwrap();
        assert_eq!(owner.status(), InitializationStatus::Initialized);
        assert_eq!(
            owner
                .initialize(
                    7,
                    Duration::from_secs(1),
                    INITIALIZATION_TOKEN,
                    OWNER_PASSWORD.to_owned(),
                )
                .unwrap_err(),
            ServerOwnerInitializationError::AlreadyInitialized
        );
    }

    #[test]
    fn path_override_environment_variables_cannot_change_the_server_data_layout() {
        let environment = environment_with([
            (
                SERVER_INITIALIZATION_TOKEN_ENV,
                OsString::from(INITIALIZATION_TOKEN),
            ),
            ("QINGYU_DATA_DIR", OsString::from("/tmp/attacker-data")),
            (
                "QINGYU_WORKSPACE_DIR",
                OsString::from("/tmp/attacker-workspace"),
            ),
            ("QINGYU_CONFIG_DIR", OsString::from("/tmp/attacker-config")),
            ("QINGYU_STATE_DIR", OsString::from("/tmp/attacker-state")),
            ("QINGYU_LOGS_DIR", OsString::from("/tmp/attacker-logs")),
            ("QINGYU_CACHE_DIR", OsString::from("/tmp/attacker-cache")),
        ])
        .unwrap();
        let layout = environment.layout();

        assert_eq!(layout.workspace_path(), Path::new("/data/workspace"));
        assert_eq!(layout.config_path(), Path::new("/data/config"));
        assert_eq!(layout.state_path(), Path::new("/data/state"));
        assert_eq!(layout.logs_path(), Path::new("/data/logs"));
        assert_eq!(layout.cache_path(), Path::new("/tmp/qingyu"));
    }

    #[test]
    fn launch_environment_and_errors_never_render_secret_values() {
        let environment = environment_with([(
            SERVER_INITIALIZATION_TOKEN_ENV,
            OsString::from(INITIALIZATION_TOKEN),
        )])
        .unwrap();
        let invalid_secret = "too-short-secret";
        let error = environment_with([(
            SERVER_INITIALIZATION_TOKEN_ENV,
            OsString::from(invalid_secret),
        )])
        .unwrap_err();

        assert!(!format!("{environment:?}").contains(INITIALIZATION_TOKEN));
        assert!(!format!("{error:?}").contains(invalid_secret));
        assert!(!error.to_string().contains(invalid_secret));
    }

    #[cfg(unix)]
    fn non_unicode_environment_value() -> OsString {
        use std::os::unix::ffi::OsStringExt as _;

        OsString::from_vec(vec![b't', b'o', b'k', b'e', b'n', 0x80])
    }

    #[cfg(windows)]
    fn non_unicode_environment_value() -> OsString {
        use std::os::windows::ffi::OsStringExt as _;

        OsString::from_wide(&[0xD800])
    }
}
