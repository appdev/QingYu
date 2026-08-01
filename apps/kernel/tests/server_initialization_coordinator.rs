use std::{fs, path::Path, sync::Arc, time::Duration};

use qingyu_kernel::{
    paths::KernelPaths,
    server::{
        AuthenticationRateLimiter, InitializationStatus, InitializationToken,
        OwnerPasswordVerification, RateLimitPolicy, ServerAuthenticationSecurity,
        ServerAuthenticationStore, ServerInitializationCoordinatorError,
        ServerOwnerInitializationError, SessionPolicy, SessionStore,
    },
};
use tempfile::tempdir;

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

fn token(value: &str) -> InitializationToken {
    InitializationToken::from_secret(value.to_owned()).unwrap()
}

fn security_owner(authentication: Arc<ServerAuthenticationStore>) -> ServerAuthenticationSecurity {
    let policy = RateLimitPolicy::new(5, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
    ServerAuthenticationSecurity::claim(
        authentication,
        AuthenticationRateLimiter::new(policy, policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
    .unwrap()
}

#[test]
fn initialization_persists_before_the_process_gate_commits_and_survives_restart() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let security = security_owner(Arc::clone(&authentication));
    let mut coordinator = security
        .initialization_coordinator(Some(token(INITIALIZATION_TOKEN)))
        .unwrap();

    assert_eq!(coordinator.status(), InitializationStatus::Pending);
    coordinator
        .initialize(
            7,
            Duration::from_secs(0),
            INITIALIZATION_TOKEN,
            OWNER_PASSWORD.to_owned(),
        )
        .unwrap();
    assert_eq!(coordinator.status(), InitializationStatus::Initialized);
    assert_eq!(
        authentication
            .verify_owner_password(OWNER_PASSWORD)
            .unwrap(),
        OwnerPasswordVerification::Authorized {
            needs_rehash: false
        }
    );

    drop(coordinator);
    drop(authentication);
    drop(security);
    let reopened = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let restarted_security = security_owner(Arc::clone(&reopened));
    let mut restarted = restarted_security
        .initialization_coordinator(Some(token(
            "different-injected-token-that-must-be-ignored-after-restart",
        )))
        .unwrap();
    assert_eq!(restarted.status(), InitializationStatus::Initialized);
    assert_eq!(
        restarted
            .initialize(
                7,
                Duration::from_secs(1),
                "different-injected-token-that-must-be-ignored-after-restart",
                "another sufficiently long password".to_owned(),
            )
            .unwrap_err(),
        ServerOwnerInitializationError::AlreadyInitialized
    );
    assert_eq!(
        reopened.verify_owner_password(OWNER_PASSWORD).unwrap(),
        OwnerPasswordVerification::Authorized {
            needs_rehash: false
        }
    );
}

#[test]
fn uninitialized_state_requires_an_injected_token_but_initialized_state_does_not() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let security = security_owner(Arc::clone(&authentication));
    assert_eq!(
        security.initialization_coordinator(None).unwrap_err(),
        ServerInitializationCoordinatorError::MissingInitializationToken
    );

    authentication
        .initialize_owner_password(OWNER_PASSWORD.to_owned())
        .unwrap();
    let initialized = security.initialization_coordinator(None).unwrap();
    assert_eq!(initialized.status(), InitializationStatus::Initialized);
}

#[test]
fn invalid_token_or_password_never_spends_the_token_or_creates_partial_state() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let security = security_owner(Arc::clone(&authentication));
    let mut coordinator = security
        .initialization_coordinator(Some(token(INITIALIZATION_TOKEN)))
        .unwrap();

    assert_eq!(
        coordinator
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
        coordinator
            .initialize(
                7,
                Duration::from_secs(1),
                INITIALIZATION_TOKEN,
                "contains space".to_owned(),
            )
            .unwrap_err(),
        ServerOwnerInitializationError::InvalidPassword
    );
    assert_eq!(coordinator.status(), InitializationStatus::Pending);
    assert!(!temporary.path().join("config/owner-auth-v1.json").exists());

    coordinator
        .initialize(
            7,
            Duration::from_secs(2),
            INITIALIZATION_TOKEN,
            OWNER_PASSWORD.to_owned(),
        )
        .unwrap();
    assert_eq!(coordinator.status(), InitializationStatus::Initialized);
}

#[test]
fn initialization_debug_and_errors_do_not_expose_tokens_passwords_or_roots() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let security = security_owner(authentication);
    let mut coordinator = security
        .initialization_coordinator(Some(token(INITIALIZATION_TOKEN)))
        .unwrap();
    let rejected_password = "rejected-owner-password-material";
    let error = coordinator
        .initialize(
            7,
            Duration::from_secs(0),
            "wrong-initialization-token",
            rejected_password.to_owned(),
        )
        .unwrap_err();
    let rendered = format!("{coordinator:?} {error:?} {error}");

    assert!(!rendered.contains(INITIALIZATION_TOKEN));
    assert!(!rendered.contains(rejected_password));
    assert!(!rendered.contains(temporary.path().to_string_lossy().as_ref()));
}
