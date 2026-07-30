use std::time::Duration;

use qingyu_kernel::server::{
    AuthenticationFlow, AuthenticationRateLimiter, InitializationError, InitializationGate,
    InitializationPermit, InitializationStatus, InitializationToken, IssuedSession,
    RateLimitDecision, RateLimitPolicy, RequestIntent, SessionAuthorization, SessionPolicy,
    SessionStore,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(InitializationToken: Clone, Copy);
assert_not_impl_any!(InitializationGate: Clone, Copy);
assert_not_impl_any!(InitializationPermit: Clone, Copy);
assert_not_impl_any!(IssuedSession: Clone, Copy);
assert_not_impl_any!(SessionStore: Clone, Copy);
assert_not_impl_any!(AuthenticationRateLimiter: Clone, Copy);

const INITIALIZATION_SECRET: &str = "initialization-secret-with-at-least-32-bytes";
const OTHER_INITIALIZATION_SECRET: &str = "different-initialization-secret-32-bytes";

#[test]
fn initialization_token_is_retryable_until_the_matching_attempt_is_committed_once() {
    let token = InitializationToken::from_secret(INITIALIZATION_SECRET.to_owned()).unwrap();
    let mut gate = InitializationGate::pending(token);

    assert_eq!(gate.status(), InitializationStatus::Pending);
    assert_eq!(
        gate.begin("wrong-initialization-secret").unwrap_err(),
        InitializationError::InvalidToken
    );
    assert_eq!(gate.status(), InitializationStatus::Pending);

    let permit = gate.begin(INITIALIZATION_SECRET).unwrap();
    assert_eq!(gate.status(), InitializationStatus::InProgress);
    assert_eq!(
        gate.begin(INITIALIZATION_SECRET).unwrap_err(),
        InitializationError::InProgress
    );
    gate.commit(permit).unwrap();

    assert_eq!(gate.status(), InitializationStatus::Initialized);
    assert_eq!(
        gate.begin(INITIALIZATION_SECRET).unwrap_err(),
        InitializationError::AlreadyInitialized
    );
}

#[test]
fn failed_initialization_can_abort_without_spending_the_one_time_token() {
    let token = InitializationToken::from_secret(INITIALIZATION_SECRET.to_owned()).unwrap();
    let mut gate = InitializationGate::pending(token);
    let permit = gate.begin(INITIALIZATION_SECRET).unwrap();

    gate.abort(permit).unwrap();

    assert_eq!(gate.status(), InitializationStatus::Pending);
    let retry = gate.begin(INITIALIZATION_SECRET).unwrap();
    gate.commit(retry).unwrap();
    assert_eq!(gate.status(), InitializationStatus::Initialized);
}

#[test]
fn dropping_an_unsettled_initialization_attempt_restores_the_pending_token() {
    let token = InitializationToken::from_secret(INITIALIZATION_SECRET.to_owned()).unwrap();
    let mut gate = InitializationGate::pending(token);
    let permit = gate.begin(INITIALIZATION_SECRET).unwrap();
    assert_eq!(gate.status(), InitializationStatus::InProgress);

    drop(permit);

    assert_eq!(gate.status(), InitializationStatus::Pending);
    let retry = gate.begin(INITIALIZATION_SECRET).unwrap();
    gate.commit(retry).unwrap();
    assert_eq!(gate.status(), InitializationStatus::Initialized);
}

#[test]
fn initialization_permits_cannot_be_replayed_or_crossed_between_gates() {
    let mut first = InitializationGate::pending(
        InitializationToken::from_secret(INITIALIZATION_SECRET.to_owned()).unwrap(),
    );
    let mut second = InitializationGate::pending(
        InitializationToken::from_secret(OTHER_INITIALIZATION_SECRET.to_owned()).unwrap(),
    );
    let first_permit = first.begin(INITIALIZATION_SECRET).unwrap();
    let second_permit = second.begin(OTHER_INITIALIZATION_SECRET).unwrap();

    assert_eq!(
        first.commit(second_permit).unwrap_err(),
        InitializationError::InvalidPermit
    );
    assert_eq!(first.status(), InitializationStatus::InProgress);
    assert_eq!(second.status(), InitializationStatus::Pending);
    first.commit(first_permit).unwrap();
    assert_eq!(first.status(), InitializationStatus::Initialized);
}

#[test]
fn initialization_secrets_and_candidates_are_redacted_from_debug_and_errors() {
    let token = InitializationToken::from_secret(INITIALIZATION_SECRET.to_owned()).unwrap();
    assert!(!format!("{token:?}").contains(INITIALIZATION_SECRET));
    let mut gate = InitializationGate::pending(token);
    assert!(!format!("{gate:?}").contains(INITIALIZATION_SECRET));

    let rejected_candidate = "rejected-candidate-must-not-leak";
    let error = gate.begin(rejected_candidate).unwrap_err();
    let debug = format!("{error:?} {error}");
    assert!(!debug.contains(rejected_candidate));
    assert!(!debug.contains(INITIALIZATION_SECRET));
}

#[test]
fn initialization_requires_bounded_nontrivial_injected_secret_material() {
    assert!(InitializationToken::from_secret("short-token".to_owned()).is_err());
    assert!(InitializationToken::from_secret("x".repeat(1025)).is_err());
    assert!(InitializationToken::from_secret("x".repeat(32)).is_ok());
}

#[test]
fn an_already_initialized_server_never_accepts_a_bootstrap_attempt() {
    let mut gate = InitializationGate::initialized();

    assert_eq!(gate.status(), InitializationStatus::Initialized);
    assert_eq!(
        gate.begin(INITIALIZATION_SECRET).unwrap_err(),
        InitializationError::AlreadyInitialized
    );
}

#[test]
fn session_authorization_requires_csrf_only_for_state_changing_requests() {
    let policy = SessionPolicy::new(Duration::from_secs(30)).unwrap();
    let mut sessions = SessionStore::new(policy);
    let issued = sessions.issue(Duration::from_secs(100)).unwrap();

    assert_eq!(
        sessions.authorize(
            issued.credential(),
            None,
            RequestIntent::ReadOnly,
            Duration::from_secs(101),
        ),
        SessionAuthorization::Authorized {
            expires_at: Duration::from_secs(130),
        }
    );
    assert_eq!(
        sessions.authorize(
            issued.credential(),
            None,
            RequestIntent::StateChanging,
            Duration::from_secs(101),
        ),
        SessionAuthorization::CsrfRejected
    );
    assert_eq!(
        sessions.authorize(
            issued.credential(),
            Some("wrong-csrf-token"),
            RequestIntent::StateChanging,
            Duration::from_secs(101),
        ),
        SessionAuthorization::CsrfRejected
    );
    assert_eq!(
        sessions.authorize(
            issued.credential(),
            Some(issued.csrf_token()),
            RequestIntent::StateChanging,
            Duration::from_secs(101),
        ),
        SessionAuthorization::Authorized {
            expires_at: Duration::from_secs(130),
        }
    );
}

#[test]
fn csrf_tokens_are_bound_to_the_session_that_issued_them() {
    let policy = SessionPolicy::new(Duration::from_secs(30)).unwrap();
    let mut sessions = SessionStore::new(policy);
    let first = sessions.issue(Duration::from_secs(0)).unwrap();
    let second = sessions.issue(Duration::from_secs(0)).unwrap();

    assert_eq!(
        sessions.authorize(
            first.credential(),
            Some(second.csrf_token()),
            RequestIntent::StateChanging,
            Duration::from_secs(1),
        ),
        SessionAuthorization::CsrfRejected
    );
}

#[test]
fn sessions_are_rejected_at_the_exact_expiry_boundary_without_an_expiry_oracle() {
    let policy = SessionPolicy::new(Duration::from_secs(30)).unwrap();
    let mut sessions = SessionStore::new(policy);
    let issued = sessions.issue(Duration::from_secs(100)).unwrap();

    assert!(matches!(
        sessions.authorize(
            issued.credential(),
            None,
            RequestIntent::ReadOnly,
            Duration::from_secs(129),
        ),
        SessionAuthorization::Authorized { .. }
    ));
    assert_eq!(
        sessions.authorize(
            issued.credential(),
            Some(issued.csrf_token()),
            RequestIntent::StateChanging,
            Duration::from_secs(130),
        ),
        SessionAuthorization::InvalidSession
    );
    assert_eq!(
        sessions.authorize(
            "never-issued-session",
            None,
            RequestIntent::ReadOnly,
            Duration::from_secs(130),
        ),
        SessionAuthorization::InvalidSession
    );
}

#[test]
fn issued_session_secrets_are_not_exposed_by_debug_output() {
    let policy = SessionPolicy::new(Duration::from_secs(30)).unwrap();
    let mut sessions = SessionStore::new(policy);
    let issued = sessions.issue(Duration::from_secs(100)).unwrap();
    let credential = issued.credential().to_owned();
    let csrf = issued.csrf_token().to_owned();

    let debug = format!("{issued:?} {sessions:?}");

    assert!(!debug.contains(&credential));
    assert!(!debug.contains(&csrf));
}

#[test]
fn zero_length_sessions_are_rejected() {
    assert!(SessionPolicy::new(Duration::ZERO).is_err());
}

#[test]
fn login_and_initialization_failures_have_independent_lockout_policies() {
    let login = RateLimitPolicy::new(3, Duration::from_secs(60), Duration::from_secs(120)).unwrap();
    let initialization =
        RateLimitPolicy::new(2, Duration::from_secs(60), Duration::from_secs(300)).unwrap();
    let mut limiter = AuthenticationRateLimiter::new(login, initialization);

    assert_eq!(
        limiter.record_failure(AuthenticationFlow::Initialization, Duration::from_secs(0)),
        RateLimitDecision::Allowed
    );
    assert_eq!(
        limiter.record_failure(AuthenticationFlow::Initialization, Duration::from_secs(1)),
        RateLimitDecision::Limited {
            retry_after: Duration::from_secs(300),
        }
    );
    assert_eq!(
        limiter.check(AuthenticationFlow::Login, Duration::from_secs(1)),
        RateLimitDecision::Allowed
    );
    assert_eq!(
        limiter.check(AuthenticationFlow::Initialization, Duration::from_secs(100)),
        RateLimitDecision::Limited {
            retry_after: Duration::from_secs(201),
        }
    );
    assert_eq!(
        limiter.check(AuthenticationFlow::Initialization, Duration::from_secs(301)),
        RateLimitDecision::Allowed
    );
}

#[test]
fn authentication_failure_windows_reset_at_the_boundary_and_after_success() {
    let policy = RateLimitPolicy::new(2, Duration::from_secs(10), Duration::from_secs(30)).unwrap();
    let mut limiter = AuthenticationRateLimiter::new(policy, policy);

    assert_eq!(
        limiter.record_failure(AuthenticationFlow::Login, Duration::from_secs(0)),
        RateLimitDecision::Allowed
    );
    assert_eq!(
        limiter.record_failure(AuthenticationFlow::Login, Duration::from_secs(10)),
        RateLimitDecision::Allowed
    );
    limiter.record_success(AuthenticationFlow::Login);
    assert_eq!(
        limiter.record_failure(AuthenticationFlow::Login, Duration::from_secs(11)),
        RateLimitDecision::Allowed
    );
    assert_eq!(
        limiter.record_failure(AuthenticationFlow::Login, Duration::from_secs(12)),
        RateLimitDecision::Limited {
            retry_after: Duration::from_secs(30),
        }
    );
}

#[test]
fn rate_limit_policy_rejects_disabled_or_zero_length_boundaries() {
    assert!(RateLimitPolicy::new(0, Duration::from_secs(1), Duration::from_secs(1)).is_err());
    assert!(RateLimitPolicy::new(1, Duration::ZERO, Duration::from_secs(1)).is_err());
    assert!(RateLimitPolicy::new(1, Duration::from_secs(1), Duration::ZERO).is_err());
}
