use std::{
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

const DEFAULT_MAXIMUM_CLIENTS_PER_FLOW: usize = 64;
const DEFAULT_MAXIMUM_IN_FLIGHT: usize = 4;
const GLOBAL_FAILURE_MULTIPLIER: u32 = 8;

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

    fn global(self) -> Self {
        Self {
            maximum_failures: self
                .maximum_failures
                .saturating_mul(GLOBAL_FAILURE_MULTIPLIER),
            ..self
        }
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
    PasswordChange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitDecision {
    Allowed,
    Limited { retry_after: Duration },
    AtCapacity,
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
struct ClientAttemptLimiter {
    client_id: u64,
    limiter: AttemptLimiter,
    last_seen_at: Duration,
}

#[derive(Debug)]
struct FlowLimiter {
    global: AttemptLimiter,
    client_policy: RateLimitPolicy,
    clients: Vec<ClientAttemptLimiter>,
    maximum_clients: usize,
}

impl FlowLimiter {
    fn new(client_policy: RateLimitPolicy, maximum_clients: usize) -> Self {
        Self {
            global: AttemptLimiter::new(client_policy.global()),
            client_policy,
            clients: Vec::with_capacity(maximum_clients),
            maximum_clients,
        }
    }

    fn check(&mut self, client_id: u64, now: Duration) -> RateLimitDecision {
        if let limited @ RateLimitDecision::Limited { .. } = self.global.check(now) {
            return limited;
        }
        let Some(index) = self
            .clients
            .iter()
            .position(|client| client.client_id == client_id)
        else {
            return RateLimitDecision::Allowed;
        };
        self.clients[index].last_seen_at = now;
        self.clients[index].limiter.check(now)
    }

    fn record_failure(&mut self, client_id: u64, now: Duration) -> RateLimitDecision {
        let global = self.global.record_failure(now);
        let client = self.client_limiter(client_id, now).record_failure(now);
        match client {
            limited @ RateLimitDecision::Limited { .. } => limited,
            RateLimitDecision::Allowed | RateLimitDecision::AtCapacity => global,
        }
    }

    fn record_success(&mut self, client_id: u64, now: Duration) {
        let Some(client) = self
            .clients
            .iter_mut()
            .find(|client| client.client_id == client_id)
        else {
            return;
        };
        client.last_seen_at = now;
        client.limiter.record_success();
    }

    fn client_limiter(&mut self, client_id: u64, now: Duration) -> &mut AttemptLimiter {
        if let Some(index) = self
            .clients
            .iter()
            .position(|client| client.client_id == client_id)
        {
            self.clients[index].last_seen_at = now;
            return &mut self.clients[index].limiter;
        }
        if self.clients.len() == self.maximum_clients {
            let oldest = self
                .clients
                .iter()
                .enumerate()
                .min_by_key(|(index, client)| (client.last_seen_at, *index))
                .map(|(index, _client)| index)
                .expect("a full client limiter cannot be empty");
            self.clients.remove(oldest);
        }
        self.clients.push(ClientAttemptLimiter {
            client_id,
            limiter: AttemptLimiter::new(self.client_policy),
            last_seen_at: now,
        });
        &mut self
            .clients
            .last_mut()
            .expect("a client limiter was just inserted")
            .limiter
    }
}

#[must_use = "the permit must be retained for the lifetime of the authentication attempt"]
pub struct AuthenticationAttemptPermit {
    flow: AuthenticationFlow,
    client_id: u64,
    began_at: Duration,
    in_flight: Arc<AtomicUsize>,
    settled: bool,
}

impl AuthenticationAttemptPermit {
    fn settle(&mut self) {
        if self.settled {
            return;
        }
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
        self.settled = true;
    }
}

impl Drop for AuthenticationAttemptPermit {
    fn drop(&mut self) {
        self.settle();
    }
}

impl fmt::Debug for AuthenticationAttemptPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticationAttemptPermit")
            .field("flow", &self.flow)
            .field("began_at", &self.began_at)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidAuthenticationAttempt;

impl fmt::Display for InvalidAuthenticationAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("authentication attempt permit is invalid")
    }
}

impl std::error::Error for InvalidAuthenticationAttempt {}

#[derive(Debug)]
pub struct AuthenticationRateLimiter {
    login: FlowLimiter,
    initialization: FlowLimiter,
    password_change: FlowLimiter,
    in_flight: Arc<AtomicUsize>,
    maximum_in_flight: usize,
}

impl AuthenticationRateLimiter {
    pub fn new(login: RateLimitPolicy, initialization: RateLimitPolicy) -> Self {
        Self::with_capacity(
            login,
            initialization,
            DEFAULT_MAXIMUM_CLIENTS_PER_FLOW,
            DEFAULT_MAXIMUM_IN_FLIGHT,
        )
        .expect("default authentication rate-limit capacity is valid")
    }

    pub fn with_capacity(
        login: RateLimitPolicy,
        initialization: RateLimitPolicy,
        maximum_clients_per_flow: usize,
        maximum_in_flight: usize,
    ) -> Result<Self, InvalidRateLimitPolicy> {
        if maximum_clients_per_flow == 0 || maximum_in_flight == 0 {
            return Err(InvalidRateLimitPolicy);
        }
        Ok(Self {
            login: FlowLimiter::new(login, maximum_clients_per_flow),
            initialization: FlowLimiter::new(initialization, maximum_clients_per_flow),
            password_change: FlowLimiter::new(login, maximum_clients_per_flow),
            in_flight: Arc::new(AtomicUsize::new(0)),
            maximum_in_flight,
        })
    }

    pub fn begin_attempt(
        &mut self,
        flow: AuthenticationFlow,
        // The host must derive this from a normalized peer identity and must
        // not trust a forwarding header unless its reverse proxy is trusted.
        client_id: u64,
        now: Duration,
    ) -> Result<AuthenticationAttemptPermit, RateLimitDecision> {
        let decision = self.limiter(flow).check(client_id, now);
        if decision != RateLimitDecision::Allowed {
            return Err(decision);
        }
        if self
            .in_flight
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < self.maximum_in_flight).then_some(current + 1)
            })
            .is_err()
        {
            return Err(RateLimitDecision::AtCapacity);
        }
        Ok(AuthenticationAttemptPermit {
            flow,
            client_id,
            began_at: now,
            in_flight: Arc::clone(&self.in_flight),
            settled: false,
        })
    }

    pub fn record_failure(
        &mut self,
        mut permit: AuthenticationAttemptPermit,
        now: Duration,
    ) -> Result<RateLimitDecision, InvalidAuthenticationAttempt> {
        self.validate_permit(&permit)?;
        let decision = self
            .limiter(permit.flow)
            .record_failure(permit.client_id, now);
        permit.settle();
        Ok(decision)
    }

    pub fn record_success(
        &mut self,
        mut permit: AuthenticationAttemptPermit,
    ) -> Result<(), InvalidAuthenticationAttempt> {
        self.validate_permit(&permit)?;
        self.limiter(permit.flow)
            .record_success(permit.client_id, permit.began_at);
        permit.settle();
        Ok(())
    }

    fn validate_permit(
        &self,
        permit: &AuthenticationAttemptPermit,
    ) -> Result<(), InvalidAuthenticationAttempt> {
        if !permit.settled && Arc::ptr_eq(&self.in_flight, &permit.in_flight) {
            return Ok(());
        }
        Err(InvalidAuthenticationAttempt)
    }

    fn limiter(&mut self, flow: AuthenticationFlow) -> &mut FlowLimiter {
        match flow {
            AuthenticationFlow::Login => &mut self.login,
            AuthenticationFlow::Initialization => &mut self.initialization,
            AuthenticationFlow::PasswordChange => &mut self.password_change,
        }
    }
}
