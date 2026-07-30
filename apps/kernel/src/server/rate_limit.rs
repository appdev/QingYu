use std::{fmt, time::Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateLimitPolicy {
    maximum_failures: u32,
    observation_window: Duration,
    lockout: Duration,
}

impl RateLimitPolicy {
    pub fn new(
        maximum_failures: u32,
        observation_window: Duration,
        lockout: Duration,
    ) -> Result<Self, InvalidRateLimitPolicy> {
        if maximum_failures == 0 || observation_window.is_zero() || lockout.is_zero() {
            return Err(InvalidRateLimitPolicy);
        }
        Ok(Self {
            maximum_failures,
            observation_window,
            lockout,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidRateLimitPolicy;

impl fmt::Display for InvalidRateLimitPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("rate-limit policy is invalid")
    }
}

impl std::error::Error for InvalidRateLimitPolicy {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticationFlow {
    Login,
    Initialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitDecision {
    Allowed,
    Limited { retry_after: Duration },
}

#[derive(Debug, Default)]
struct AttemptState {
    window_started_at: Option<Duration>,
    failures: u32,
    blocked_until: Option<Duration>,
}

#[derive(Debug)]
struct AttemptLimiter {
    policy: RateLimitPolicy,
    state: AttemptState,
}

impl AttemptLimiter {
    fn new(policy: RateLimitPolicy) -> Self {
        Self {
            policy,
            state: AttemptState::default(),
        }
    }

    fn check(&mut self, now: Duration) -> RateLimitDecision {
        self.normalize(now);
        match self.state.blocked_until {
            Some(blocked_until) if now < blocked_until => RateLimitDecision::Limited {
                retry_after: blocked_until.saturating_sub(now),
            },
            _ => RateLimitDecision::Allowed,
        }
    }

    fn record_failure(&mut self, now: Duration) -> RateLimitDecision {
        if let limited @ RateLimitDecision::Limited { .. } = self.check(now) {
            return limited;
        }
        let started_at = self.state.window_started_at.get_or_insert(now);
        if now
            .checked_sub(*started_at)
            .is_some_and(|elapsed| elapsed >= self.policy.observation_window)
        {
            *started_at = now;
            self.state.failures = 0;
        }
        self.state.failures = self.state.failures.saturating_add(1);
        if self.state.failures < self.policy.maximum_failures {
            return RateLimitDecision::Allowed;
        }
        let blocked_until = now
            .checked_add(self.policy.lockout)
            .unwrap_or(Duration::MAX);
        self.state.blocked_until = Some(blocked_until);
        RateLimitDecision::Limited {
            retry_after: blocked_until.saturating_sub(now),
        }
    }

    fn record_success(&mut self) {
        self.state = AttemptState::default();
    }

    fn normalize(&mut self, now: Duration) {
        if self
            .state
            .blocked_until
            .is_some_and(|blocked_until| now >= blocked_until)
        {
            self.state = AttemptState::default();
            return;
        }
        if self.state.blocked_until.is_none()
            && self.state.window_started_at.is_some_and(|started_at| {
                now.checked_sub(started_at)
                    .is_some_and(|elapsed| elapsed >= self.policy.observation_window)
            })
        {
            self.state = AttemptState::default();
        }
    }
}

#[derive(Debug)]
pub struct AuthenticationRateLimiter {
    login: AttemptLimiter,
    initialization: AttemptLimiter,
}

impl AuthenticationRateLimiter {
    pub fn new(login: RateLimitPolicy, initialization: RateLimitPolicy) -> Self {
        Self {
            login: AttemptLimiter::new(login),
            initialization: AttemptLimiter::new(initialization),
        }
    }

    pub fn check(&mut self, flow: AuthenticationFlow, now: Duration) -> RateLimitDecision {
        self.limiter(flow).check(now)
    }

    pub fn record_failure(&mut self, flow: AuthenticationFlow, now: Duration) -> RateLimitDecision {
        self.limiter(flow).record_failure(now)
    }

    pub fn record_success(&mut self, flow: AuthenticationFlow) {
        self.limiter(flow).record_success();
    }

    fn limiter(&mut self, flow: AuthenticationFlow) -> &mut AttemptLimiter {
        match flow {
            AuthenticationFlow::Login => &mut self.login,
            AuthenticationFlow::Initialization => &mut self.initialization,
        }
    }
}
