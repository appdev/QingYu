//! Transport-independent security primitives for the single-user server host.
//!
//! Environment loading, HTTP cookies, headers, and routes deliberately live
//! outside this module. Callers inject bootstrap material and monotonic time.

mod auth_store;
mod csrf;
mod initialization;
mod rate_limit;
mod secret;
mod session;

pub use auth_store::{
    OwnerPasswordInitializationError, OwnerPasswordVerification, ServerAuthenticationError,
    ServerAuthenticationStatus, ServerAuthenticationStore,
};
pub use csrf::RequestIntent;
pub use initialization::{
    InitializationError, InitializationGate, InitializationPermit, InitializationStatus,
    InitializationToken, InvalidInitializationToken,
};
pub use rate_limit::{
    AuthenticationFlow, AuthenticationRateLimiter, InvalidRateLimitPolicy, RateLimitDecision,
    RateLimitPolicy,
};
pub use session::{
    InvalidSessionPolicy, IssuedSession, SessionAuthorization, SessionIssueError, SessionPolicy,
    SessionStore,
};
