//! Transport-independent security primitives for the single-user server host.
//!
//! Environment loading, HTTP cookies, headers, and routes deliberately live
//! outside this module. Callers inject bootstrap material and monotonic time.

mod auth_store;
mod authentication_coordinator;
mod csrf;
mod initialization;
mod initialization_coordinator;
mod launch_environment;
mod rate_limit;
mod secret;
mod session;

pub use auth_store::{
    OwnerPasswordInitializationError, OwnerPasswordRehash, OwnerPasswordUpdateError,
    OwnerPasswordVerification, ServerAuthenticationError, ServerAuthenticationStatus,
    ServerAuthenticationStore,
};
pub use authentication_coordinator::{
    ServerAuthenticationCoordinator, ServerAuthenticationCoordinatorError, ServerLogin,
};
pub use csrf::RequestIntent;
pub use initialization::{
    InitializationError, InitializationGate, InitializationPermit, InitializationStatus,
    InitializationToken, InvalidInitializationToken,
};
pub use initialization_coordinator::{
    ServerInitializationCoordinator, ServerInitializationCoordinatorError,
    ServerOwnerInitializationError,
};
pub use launch_environment::{
    ServerLaunchEnvironment, ServerLaunchEnvironmentError, SERVER_INITIALIZATION_TOKEN_ENV,
};
pub use rate_limit::{
    AuthenticationAttemptPermit, AuthenticationFlow, AuthenticationRateLimiter,
    InvalidAuthenticationAttempt, InvalidRateLimitPolicy, RateLimitDecision, RateLimitPolicy,
};
pub use session::{
    InvalidSessionPolicy, IssuedSession, SessionAuthorization, SessionIssueError, SessionPolicy,
    SessionStore,
};
