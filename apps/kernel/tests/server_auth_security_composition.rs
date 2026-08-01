use std::{
    fs,
    path::Path,
    sync::{Arc, Barrier},
    thread,
    time::Duration,
};

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
const OWNER_PASSWORD: &str = "Correct-Horse-Battery-Staple!7";
const NEW_OWNER_PASSWORD: &str = "New-Owner-Password-Material!8";

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
fn a_derived_coordinator_retains_the_root_claim_after_the_security_factory_is_dropped() {
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
    let coordinator = first.authentication_coordinator();
    drop(first);

    let overlapping = ServerAuthenticationSecurity::claim(
        Arc::clone(&second_store),
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    );
    assert!(matches!(overlapping, Err(ServerAuthenticationError)));

    drop(coordinator);
    ServerAuthenticationSecurity::claim(
        second_store,
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
    .expect("the failed claim must be retryable after the last old coordinator drops");
}

#[test]
fn a_failed_reopened_store_claim_is_retryable_after_the_current_owner_drops() {
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

    assert!(ServerAuthenticationSecurity::claim(
        Arc::clone(&second_store),
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
    .is_err());
    drop(first);

    ServerAuthenticationSecurity::claim(
        second_store,
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
    .expect("a global root collision must not permanently consume the losing store");
}

#[cfg(unix)]
#[test]
fn renaming_the_same_physical_config_root_cannot_bypass_the_owner_claim() {
    let temporary = tempdir().unwrap();
    let original_paths = fixture_paths(temporary.path());
    let first_store =
        Arc::new(ServerAuthenticationStore::open(original_paths.config_root()).unwrap());
    let policy = rate_policy(3);
    let first = ServerAuthenticationSecurity::claim(
        first_store,
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
    .unwrap();
    let moved_config = temporary.path().join("moved-config");
    fs::rename(temporary.path().join("config"), &moved_config).unwrap();
    let moved_paths = KernelPaths::desktop(
        &temporary.path().join("workspace"),
        &moved_config,
        &temporary.path().join("cache"),
    )
    .unwrap();
    let reopened = Arc::new(ServerAuthenticationStore::open(moved_paths.config_root()).unwrap());

    let overlapping = ServerAuthenticationSecurity::claim(
        reopened,
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    );
    assert!(matches!(overlapping, Err(ServerAuthenticationError)));
    drop(first);
}

#[test]
fn concurrent_reopened_store_claims_publish_exactly_one_owner() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let stores = (0..8)
        .map(|_| Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap()))
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(stores.len()));
    let policy = rate_policy(3);
    let workers = stores
        .into_iter()
        .map(|store| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                ServerAuthenticationSecurity::claim(
                    store,
                    AuthenticationRateLimiter::new(policy, policy),
                    SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
                )
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
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
    let error = initialization
        .initialize(
            7,
            Duration::from_secs(2),
            INITIALIZATION_TOKEN,
            OWNER_PASSWORD.to_owned(),
        )
        .unwrap_err();
    let ServerOwnerInitializationError::RateLimited { retry_after } = error else {
        panic!("unexpected initialization error: {error:?}");
    };
    assert!(retry_after > Duration::from_secs(29));
    assert!(retry_after <= Duration::from_secs(30));
    assert_eq!(initialization.status(), InitializationStatus::Pending);

    initialization
        .initialize(
            7,
            Duration::from_secs(32),
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
                "contains space".to_owned(),
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
