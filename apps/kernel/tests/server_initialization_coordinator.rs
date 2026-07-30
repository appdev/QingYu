use std::{fs, path::Path, sync::Arc};

use qingyu_kernel::{
    paths::KernelPaths,
    server::{
        InitializationStatus, InitializationToken, OwnerPasswordVerification,
        ServerAuthenticationStore, ServerInitializationCoordinator,
        ServerInitializationCoordinatorError, ServerOwnerInitializationError,
    },
};
use tempfile::tempdir;

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

fn token(value: &str) -> InitializationToken {
    InitializationToken::from_secret(value.to_owned()).unwrap()
}

#[test]
fn initialization_persists_before_the_process_gate_commits_and_survives_restart() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let mut coordinator = ServerInitializationCoordinator::open(
        Arc::clone(&authentication),
        Some(token(INITIALIZATION_TOKEN)),
    )
    .unwrap();

    assert_eq!(coordinator.status(), InitializationStatus::Pending);
    coordinator
        .initialize(INITIALIZATION_TOKEN, OWNER_PASSWORD.to_owned())
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
    let reopened = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let mut restarted = ServerInitializationCoordinator::open(
        Arc::clone(&reopened),
        Some(token(
            "different-injected-token-that-must-be-ignored-after-restart",
        )),
    )
    .unwrap();
    assert_eq!(restarted.status(), InitializationStatus::Initialized);
    assert_eq!(
        restarted
            .initialize(
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
    assert_eq!(
        ServerInitializationCoordinator::open(Arc::clone(&authentication), None).unwrap_err(),
        ServerInitializationCoordinatorError::MissingInitializationToken
    );

    authentication
        .initialize_owner_password(OWNER_PASSWORD.to_owned())
        .unwrap();
    let initialized = ServerInitializationCoordinator::open(authentication, None).unwrap();
    assert_eq!(initialized.status(), InitializationStatus::Initialized);
}

#[test]
fn invalid_token_or_password_never_spends_the_token_or_creates_partial_state() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let mut coordinator = ServerInitializationCoordinator::open(
        Arc::clone(&authentication),
        Some(token(INITIALIZATION_TOKEN)),
    )
    .unwrap();

    assert_eq!(
        coordinator
            .initialize("wrong-initialization-token", OWNER_PASSWORD.to_owned())
            .unwrap_err(),
        ServerOwnerInitializationError::InvalidToken
    );
    assert_eq!(
        coordinator
            .initialize(INITIALIZATION_TOKEN, "short".to_owned())
            .unwrap_err(),
        ServerOwnerInitializationError::InvalidPassword
    );
    assert_eq!(coordinator.status(), InitializationStatus::Pending);
    assert!(!temporary.path().join("config/owner-auth-v1.json").exists());

    coordinator
        .initialize(INITIALIZATION_TOKEN, OWNER_PASSWORD.to_owned())
        .unwrap();
    assert_eq!(coordinator.status(), InitializationStatus::Initialized);
}

#[test]
fn initialization_debug_and_errors_do_not_expose_tokens_passwords_or_roots() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let mut coordinator =
        ServerInitializationCoordinator::open(authentication, Some(token(INITIALIZATION_TOKEN)))
            .unwrap();
    let rejected_password = "rejected-owner-password-material";
    let error = coordinator
        .initialize("wrong-initialization-token", rejected_password.to_owned())
        .unwrap_err();
    let rendered = format!("{coordinator:?} {error:?} {error}");

    assert!(!rendered.contains(INITIALIZATION_TOKEN));
    assert!(!rendered.contains(rejected_password));
    assert!(!rendered.contains(temporary.path().to_string_lossy().as_ref()));
}
