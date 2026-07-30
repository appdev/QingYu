use std::{fs, path::Path, sync::Arc, time::Duration};

use argon2::{
    password_hash::{PasswordHasher as _, SaltString},
    Algorithm, Argon2, Params, Version,
};
use qingyu_kernel::{
    paths::KernelPaths,
    server::{
        AuthenticationFlow, AuthenticationRateLimiter, RateLimitPolicy, RequestIntent,
        ServerAuthenticationCoordinator, ServerAuthenticationCoordinatorError,
        ServerAuthenticationStore, SessionAuthorization, SessionPolicy, SessionStore,
    },
};
use tempfile::tempdir;

const OWNER_PASSWORD: &str = "correct horse battery staple";
const INCORRECT_PASSWORD: &str = "incorrect owner password material";

fn fixture_paths(root: &Path) -> KernelPaths {
    let workspace = root.join("workspace");
    let config = root.join("config");
    let cache = root.join("cache");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&cache).unwrap();
    KernelPaths::desktop(&workspace, &config, &cache).unwrap()
}

fn initialized_store(paths: &KernelPaths) -> Arc<ServerAuthenticationStore> {
    let store = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    store
        .initialize_owner_password(OWNER_PASSWORD.to_owned())
        .unwrap();
    store
}

fn policy(maximum_failures: u32) -> RateLimitPolicy {
    RateLimitPolicy::new(
        maximum_failures,
        Duration::from_secs(60),
        Duration::from_secs(30),
    )
    .unwrap()
}

fn coordinator(
    authentication: Arc<ServerAuthenticationStore>,
    maximum_failures: u32,
) -> ServerAuthenticationCoordinator {
    let rate_policy = policy(maximum_failures);
    ServerAuthenticationCoordinator::new(
        authentication,
        AuthenticationRateLimiter::new(rate_policy, rate_policy),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
}

#[test]
fn successful_login_issues_one_session_with_uniform_session_and_csrf_authorization() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let coordinator = coordinator(initialized_store(&paths), 3);

    let login = coordinator
        .login(7, Duration::from_secs(10), OWNER_PASSWORD.to_owned())
        .unwrap();
    assert!(!login.needs_rehash());
    assert!(matches!(
        coordinator
            .authorize(
                login.session().credential(),
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(11),
            )
            .unwrap(),
        SessionAuthorization::Authorized { .. }
    ));
    assert_eq!(
        coordinator
            .authorize(
                login.session().credential(),
                None,
                RequestIntent::StateChanging,
                Duration::from_secs(11),
            )
            .unwrap(),
        SessionAuthorization::CsrfRejected
    );
    assert!(matches!(
        coordinator
            .authorize(
                login.session().credential(),
                Some(login.session().csrf_token()),
                RequestIntent::StateChanging,
                Duration::from_secs(11),
            )
            .unwrap(),
        SessionAuthorization::Authorized { .. }
    ));
}

#[test]
fn rejected_passwords_are_settled_as_failures_before_a_client_is_limited() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let coordinator = coordinator(initialized_store(&paths), 2);

    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(0), INCORRECT_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::InvalidCredentials
    );
    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(1), INCORRECT_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::RateLimited {
            retry_after: Duration::from_secs(30),
        }
    );
    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(2), OWNER_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::RateLimited {
            retry_after: Duration::from_secs(29),
        }
    );
    coordinator
        .login(7, Duration::from_secs(31), OWNER_PASSWORD.to_owned())
        .unwrap();
}

#[test]
fn unavailable_password_state_still_settles_each_admitted_attempt_as_a_failure() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let coordinator = coordinator(authentication, 2);

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
        ServerAuthenticationCoordinatorError::RateLimited {
            retry_after: Duration::from_secs(29),
        }
    );
}

#[test]
fn a_successful_password_verification_settles_before_session_issue_and_resets_its_client() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let coordinator = coordinator(initialized_store(&paths), 2);
    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(0), INCORRECT_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::InvalidCredentials
    );
    let login = coordinator
        .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
        .unwrap();
    assert!(coordinator.logout(login.session().credential()).unwrap());

    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(2), INCORRECT_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::InvalidCredentials
    );
}

#[test]
fn successful_verification_is_settled_even_when_session_issue_later_fails() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let rate_policy = policy(2);
    let coordinator = ServerAuthenticationCoordinator::new(
        initialized_store(&paths),
        AuthenticationRateLimiter::new(rate_policy, rate_policy),
        SessionStore::new(SessionPolicy::new(Duration::MAX).unwrap()),
    );
    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(0), INCORRECT_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::InvalidCredentials
    );
    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::StateUnavailable
    );

    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(2), INCORRECT_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::InvalidCredentials
    );
}

#[test]
fn global_in_flight_admission_happens_before_password_verification() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let authentication = initialized_store(&paths);
    let rate_policy = policy(3);
    let mut limiter =
        AuthenticationRateLimiter::with_capacity(rate_policy, rate_policy, 2, 1).unwrap();
    let held = limiter
        .begin_attempt(AuthenticationFlow::Login, 99, Duration::from_secs(0))
        .unwrap();
    let coordinator = ServerAuthenticationCoordinator::new(
        authentication,
        limiter,
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    );

    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::AtCapacity
    );
    drop(held);
    coordinator
        .login(7, Duration::from_secs(2), OWNER_PASSWORD.to_owned())
        .unwrap();
    assert_eq!(coordinator.logout_all().unwrap(), 1);
}

#[test]
fn a_legacy_password_hash_is_upgraded_before_a_session_is_issued() {
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
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());

    let login = coordinator(Arc::clone(&authentication), 3)
        .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
        .unwrap();
    assert!(!login.needs_rehash());
    assert_eq!(
        authentication
            .verify_owner_password(OWNER_PASSWORD)
            .unwrap(),
        qingyu_kernel::server::OwnerPasswordVerification::Authorized {
            needs_rehash: false
        }
    );
}

#[test]
fn password_change_requires_csrf_and_current_password_without_revoking_on_rejection() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let coordinator = coordinator(initialized_store(&paths), 3);
    let login = coordinator
        .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
        .unwrap();

    assert_eq!(
        coordinator
            .change_password(
                login.session().credential(),
                None,
                Duration::from_secs(1),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::CsrfRejected
    );
    assert_eq!(
        coordinator
            .change_password(
                login.session().credential(),
                Some(login.session().csrf_token()),
                Duration::from_secs(2),
                INCORRECT_PASSWORD.to_owned(),
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
                Duration::from_secs(3),
                OWNER_PASSWORD.to_owned(),
                "short".to_owned(),
            )
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::InvalidPassword
    );
    assert_eq!(
        coordinator
            .change_password(
                "invalid session credential",
                Some(login.session().csrf_token()),
                Duration::from_secs(3),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::InvalidSession
    );
    assert!(matches!(
        coordinator
            .authorize(
                login.session().credential(),
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(4),
            )
            .unwrap(),
        SessionAuthorization::Authorized { .. }
    ));
    coordinator
        .login(7, Duration::from_secs(5), OWNER_PASSWORD.to_owned())
        .unwrap();
}

#[test]
fn successful_password_change_revokes_every_existing_session_and_only_new_password_logs_in() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let coordinator = coordinator(initialized_store(&paths), 3);
    let first = coordinator
        .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
        .unwrap();
    let second = coordinator
        .login(8, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
        .unwrap();

    assert_eq!(
        coordinator
            .change_password(
                first.session().credential(),
                Some(first.session().csrf_token()),
                Duration::from_secs(2),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
            .unwrap(),
        2
    );
    for login in [&first, &second] {
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
    assert_eq!(
        coordinator
            .login(7, Duration::from_secs(4), OWNER_PASSWORD.to_owned())
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::InvalidCredentials
    );
    coordinator
        .login(
            7,
            Duration::from_secs(5),
            "new owner password material".to_owned(),
        )
        .unwrap();
}

#[test]
fn unavailable_password_state_during_change_revokes_the_authorizing_session() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let coordinator = coordinator(initialized_store(&paths), 3);
    let login = coordinator
        .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
        .unwrap();
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
                Duration::from_secs(1),
                OWNER_PASSWORD.to_owned(),
                "new owner password material".to_owned(),
            )
            .unwrap_err(),
        ServerAuthenticationCoordinatorError::StateUnavailable
    );
    assert_eq!(
        coordinator
            .authorize(
                login.session().credential(),
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(2),
            )
            .unwrap(),
        SessionAuthorization::InvalidSession
    );
}

#[test]
fn logout_revokes_one_session_and_logout_all_revokes_every_remaining_session() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let coordinator = coordinator(initialized_store(&paths), 3);
    let first = coordinator
        .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
        .unwrap();
    let second = coordinator
        .login(7, Duration::from_secs(1), OWNER_PASSWORD.to_owned())
        .unwrap();

    assert!(coordinator.logout(first.session().credential()).unwrap());
    assert!(!coordinator.logout(first.session().credential()).unwrap());
    assert_eq!(
        coordinator
            .authorize(
                first.session().credential(),
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(2),
            )
            .unwrap(),
        SessionAuthorization::InvalidSession
    );
    assert_eq!(coordinator.logout_all().unwrap(), 1);
    assert_eq!(coordinator.logout_all().unwrap(), 0);
    assert_eq!(
        coordinator
            .authorize(
                second.session().credential(),
                None,
                RequestIntent::ReadOnly,
                Duration::from_secs(2),
            )
            .unwrap(),
        SessionAuthorization::InvalidSession
    );
}

#[test]
fn authentication_debug_and_errors_do_not_expose_passwords_sessions_or_roots() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let coordinator = coordinator(initialized_store(&paths), 3);
    let login = coordinator
        .login(7, Duration::from_secs(0), OWNER_PASSWORD.to_owned())
        .unwrap();
    let credential = login.session().credential().to_owned();
    let csrf = login.session().csrf_token().to_owned();
    let error = coordinator
        .login(8, Duration::from_secs(1), INCORRECT_PASSWORD.to_owned())
        .unwrap_err();
    let rendered = format!("{coordinator:?} {login:?} {error:?} {error}");

    assert!(!rendered.contains(OWNER_PASSWORD));
    assert!(!rendered.contains(INCORRECT_PASSWORD));
    assert!(!rendered.contains(&credential));
    assert!(!rendered.contains(&csrf));
    assert!(!rendered.contains(temporary.path().to_string_lossy().as_ref()));
}
