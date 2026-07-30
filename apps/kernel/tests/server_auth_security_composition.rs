use std::{fs, path::Path, sync::Arc, time::Duration};

use qingyu_kernel::{
    paths::KernelPaths,
    server::{
        AuthenticationFlow, AuthenticationRateLimiter, InitializationStatus, InitializationToken,
        RateLimitPolicy, RequestIntent, ServerAuthenticationError, ServerAuthenticationSecurity,
        ServerAuthenticationStore, ServerOwnerInitializationError, SessionAuthorization,
        SessionPolicy, SessionStore,
    },
};
use static_assertions::assert_not_impl_any;
use tempfile::tempdir;

const INITIALIZATION_TOKEN: &str = "injected-random-initialization-token-at-least-32-bytes";
const OWNER_PASSWORD: &str = "correct horse battery staple";
const NEW_OWNER_PASSWORD: &str = "new owner password material";

assert_not_impl_any!(ServerAuthenticationSecurity: Clone, Copy);

fn fixture_paths(root: &Path) -> KernelPaths {
    let workspace = root.join("workspace");
    let config = root.join("config");
    let cache = root.join("cache");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&cache).unwrap();
    KernelPaths::desktop(&workspace, &config, &cache).unwrap()
}

fn rate_policy(maximum_failures: u32) -> RateLimitPolicy {
    RateLimitPolicy::new(
        maximum_failures,
        Duration::from_secs(60),
        Duration::from_secs(30),
    )
    .unwrap()
}

fn security(
    authentication: Arc<ServerAuthenticationStore>,
    limiter: AuthenticationRateLimiter,
) -> ServerAuthenticationSecurity {
    ServerAuthenticationSecurity::claim(
        authentication,
        limiter,
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
    .unwrap()
}

fn token() -> InitializationToken {
    InitializationToken::from_secret(INITIALIZATION_TOKEN.to_owned()).unwrap()
}

#[test]
fn the_authentication_store_allows_only_one_process_security_owner_claim() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let policy = rate_policy(3);
    let first = ServerAuthenticationSecurity::claim(
        Arc::clone(&authentication),
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
    .unwrap();

    let second = ServerAuthenticationSecurity::claim(
        Arc::clone(&authentication),
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    );
    assert!(matches!(second, Err(ServerAuthenticationError)));

    drop(first);
    let after_drop = ServerAuthenticationSecurity::claim(
        authentication,
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    );
    assert!(matches!(after_drop, Err(ServerAuthenticationError)));
}

#[test]
fn reopening_the_same_authentication_root_cannot_create_a_second_security_owner() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let first_store = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let second_store = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let policy = rate_policy(3);
    let first = ServerAuthenticationSecurity::claim(
        first_store,
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
    .unwrap();

    let second = ServerAuthenticationSecurity::claim(
        second_store,
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    );
    assert!(matches!(second, Err(ServerAuthenticationError)));
    drop(first);
}

#[test]
fn coordinators_derived_from_one_process_owner_share_sessions_and_revocation() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    authentication
        .initialize_owner_password(OWNER_PASSWORD.to_owned())
        .unwrap();
    let policy = rate_policy(3);
    let security = security(
        authentication,
        AuthenticationRateLimiter::new(policy, policy),
    );
    let first = security.authentication_coordinator();
    let second = security.authentication_coordinator();

    let login = first
        .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
        .unwrap();
    assert!(matches!(
        second
            .authorize(
                login.session().credential(),
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(1),
            )
            .unwrap(),
        SessionAuthorization::Authorized { .. }
    ));
    assert!(second
        .logout(
            login.session().credential(),
            Some(login.session().csrf_token()),
            Duration::from_secs(1),
        )
        .unwrap());
    assert_eq!(
        first
            .authorize(
                login.session().credential(),
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(2),
            )
            .unwrap(),
        SessionAuthorization::InvalidSession
    );

    let login = first
        .login(7, Duration::from_secs(3), OWNER_PASSWORD.to_owned())
        .unwrap();

    assert_eq!(
        second
            .change_password(
                login.session().credential(),
                Some(login.session().csrf_token()),
                Duration::from_secs(4),
                OWNER_PASSWORD.to_owned(),
                NEW_OWNER_PASSWORD.to_owned(),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        first
            .authorize(
                login.session().credential(),
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(5),
            )
            .unwrap(),
        SessionAuthorization::InvalidSession
    );
}

#[test]
fn initialization_shares_the_process_wide_in_flight_budget_with_login() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let policy = rate_policy(3);
    let mut limiter = AuthenticationRateLimiter::with_capacity(policy, policy, 8, 1).unwrap();
    let held_login = limiter
        .begin_attempt(AuthenticationFlow::Login, 7, Duration::from_secs(0))
        .unwrap();
    let security = security(authentication, limiter);
    let mut initialization = security.initialization_coordinator(Some(token())).unwrap();

    assert_eq!(
        initialization
            .initialize(
                8,
                Duration::from_secs(1),
                INITIALIZATION_TOKEN,
                OWNER_PASSWORD.to_owned(),
            )
            .unwrap_err(),
        ServerOwnerInitializationError::AtCapacity
    );
    assert_eq!(initialization.status(), InitializationStatus::Pending);
    assert!(!temporary.path().join("config/owner-auth-v1.json").exists());

    drop(held_login);
    initialization
        .initialize(
            8,
            Duration::from_secs(2),
            INITIALIZATION_TOKEN,
            OWNER_PASSWORD.to_owned(),
        )
        .unwrap();
    assert_eq!(initialization.status(), InitializationStatus::Initialized);
}

#[test]
fn initialization_rate_limit_counts_invalid_tokens_without_spending_the_token() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let policy = rate_policy(2);
    let security = security(
        authentication,
        AuthenticationRateLimiter::new(policy, policy),
    );
    let mut initialization = security.initialization_coordinator(Some(token())).unwrap();

    assert_eq!(
        initialization
            .initialize(
                7,
                Duration::from_secs(0),
                "wrong-initialization-token",
                OWNER_PASSWORD.to_owned(),
            )
            .unwrap_err(),
        ServerOwnerInitializationError::InvalidToken
    );
    assert_eq!(
        initialization
            .initialize(
                7,
                Duration::from_secs(1),
                "another-wrong-initialization-token",
                OWNER_PASSWORD.to_owned(),
            )
            .unwrap_err(),
        ServerOwnerInitializationError::RateLimited {
            retry_after: Duration::from_secs(30),
        }
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
        ServerOwnerInitializationError::RateLimited {
            retry_after: Duration::from_secs(29),
        }
    );
    assert_eq!(initialization.status(), InitializationStatus::Pending);

    initialization
        .initialize(
            7,
            Duration::from_secs(31),
            INITIALIZATION_TOKEN,
            OWNER_PASSWORD.to_owned(),
        )
        .unwrap();
    assert_eq!(initialization.status(), InitializationStatus::Initialized);
}

#[test]
fn authorized_token_with_invalid_password_resets_failures_and_remains_retryable() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let policy = rate_policy(2);
    let security = security(
        authentication,
        AuthenticationRateLimiter::new(policy, policy),
    );
    let mut initialization = security.initialization_coordinator(Some(token())).unwrap();

    assert_eq!(
        initialization
            .initialize(
                7,
                Duration::from_secs(0),
                "wrong-initialization-token",
                OWNER_PASSWORD.to_owned(),
            )
            .unwrap_err(),
        ServerOwnerInitializationError::InvalidToken
    );
    assert_eq!(
        initialization
            .initialize(
                7,
                Duration::from_secs(1),
                INITIALIZATION_TOKEN,
                "short".to_owned(),
            )
            .unwrap_err(),
        ServerOwnerInitializationError::InvalidPassword
    );
    assert_eq!(
        initialization
            .initialize(
                7,
                Duration::from_secs(2),
                "wrong-initialization-token",
                OWNER_PASSWORD.to_owned(),
            )
            .unwrap_err(),
        ServerOwnerInitializationError::InvalidToken
    );

    initialization
        .initialize(
            7,
            Duration::from_secs(3),
            INITIALIZATION_TOKEN,
            OWNER_PASSWORD.to_owned(),
        )
        .unwrap();
}
