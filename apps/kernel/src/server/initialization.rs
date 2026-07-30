use std::{
    fmt, mem,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use super::secret::SecretDigest;
use uuid::Uuid;
use zeroize::Zeroize as _;

const MINIMUM_INITIALIZATION_TOKEN_BYTES: usize = 32;
const MAXIMUM_INITIALIZATION_TOKEN_BYTES: usize = 1024;

pub struct InitializationToken(SecretDigest);

impl InitializationToken {
    pub fn from_secret(mut secret: String) -> Result<Self, InvalidInitializationToken> {
        if !(MINIMUM_INITIALIZATION_TOKEN_BYTES..=MAXIMUM_INITIALIZATION_TOKEN_BYTES)
            .contains(&secret.len())
        {
            secret.zeroize();
            return Err(InvalidInitializationToken);
        }
        let digest = SecretDigest::from_candidate(secret.as_str());
        secret.zeroize();
        Ok(Self(digest))
    }

    fn matches(&self, candidate: &str) -> bool {
        self.0.matches(candidate)
    }
}

impl fmt::Debug for InitializationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InitializationToken([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidInitializationToken;

impl fmt::Display for InvalidInitializationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("initialization token is invalid")
    }
}

impl std::error::Error for InvalidInitializationToken {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitializationStatus {
    Pending,
    InProgress,
    Initialized,
}

pub struct InitializationPermit {
    gate_id: Uuid,
    attempt_id: Uuid,
    state: Weak<Mutex<InitializationState>>,
    settled: bool,
}

impl fmt::Debug for InitializationPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InitializationPermit(..)")
    }
}

impl Drop for InitializationPermit {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let mut state = lock_state(&state);
        let _restored = restore_pending(&mut state, self.attempt_id);
    }
}

enum InitializationState {
    Pending(InitializationToken),
    InProgress {
        token: InitializationToken,
        attempt_id: Uuid,
    },
    Initialized,
}

pub struct InitializationGate {
    gate_id: Uuid,
    state: Arc<Mutex<InitializationState>>,
}

impl InitializationGate {
    pub fn pending(token: InitializationToken) -> Self {
        Self {
            gate_id: Uuid::new_v4(),
            state: Arc::new(Mutex::new(InitializationState::Pending(token))),
        }
    }

    pub fn initialized() -> Self {
        Self {
            gate_id: Uuid::new_v4(),
            state: Arc::new(Mutex::new(InitializationState::Initialized)),
        }
    }

    pub fn status(&self) -> InitializationStatus {
        match &*lock_state(&self.state) {
            InitializationState::Pending(_) => InitializationStatus::Pending,
            InitializationState::InProgress { .. } => InitializationStatus::InProgress,
            InitializationState::Initialized => InitializationStatus::Initialized,
        }
    }

    pub fn begin(&mut self, candidate: &str) -> Result<InitializationPermit, InitializationError> {
        let attempt_id = Uuid::new_v4();
        let mut state = lock_state(&self.state);
        match &*state {
            InitializationState::Pending(token) => {
                if !token.matches(candidate) {
                    return Err(InitializationError::InvalidToken);
                }
            }
            InitializationState::InProgress { .. } => return Err(InitializationError::InProgress),
            InitializationState::Initialized => {
                return Err(InitializationError::AlreadyInitialized)
            }
        }

        let InitializationState::Pending(token) =
            mem::replace(&mut *state, InitializationState::Initialized)
        else {
            unreachable!("pending initialization state was verified above")
        };
        *state = InitializationState::InProgress { token, attempt_id };
        drop(state);
        Ok(InitializationPermit {
            gate_id: self.gate_id,
            attempt_id,
            state: Arc::downgrade(&self.state),
            settled: false,
        })
    }

    pub fn commit(&mut self, mut permit: InitializationPermit) -> Result<(), InitializationError> {
        if permit.gate_id != self.gate_id
            || !permit.state.ptr_eq(&Arc::downgrade(&self.state))
            || !matches!(
                &*lock_state(&self.state),
                InitializationState::InProgress { attempt_id, .. }
                    if *attempt_id == permit.attempt_id
            )
        {
            return Err(InitializationError::InvalidPermit);
        }
        *lock_state(&self.state) = InitializationState::Initialized;
        permit.settled = true;
        Ok(())
    }

    pub fn abort(&mut self, mut permit: InitializationPermit) -> Result<(), InitializationError> {
        if permit.gate_id != self.gate_id || !permit.state.ptr_eq(&Arc::downgrade(&self.state)) {
            return Err(InitializationError::InvalidPermit);
        }
        let mut state = lock_state(&self.state);
        if !restore_pending(&mut state, permit.attempt_id) {
            return Err(InitializationError::InvalidPermit);
        }
        permit.settled = true;
        Ok(())
    }
}

fn restore_pending(state: &mut InitializationState, attempt_id: Uuid) -> bool {
    if !matches!(
        state,
        InitializationState::InProgress {
            attempt_id: current,
            ..
        } if *current == attempt_id
    ) {
        return false;
    }
    let InitializationState::InProgress { token, .. } =
        mem::replace(state, InitializationState::Initialized)
    else {
        unreachable!("the matching initialization attempt was checked above")
    };
    *state = InitializationState::Pending(token);
    true
}

fn lock_state(state: &Arc<Mutex<InitializationState>>) -> MutexGuard<'_, InitializationState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl fmt::Debug for InitializationGate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InitializationGate")
            .field("status", &self.status())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitializationError {
    InvalidToken,
    InProgress,
    AlreadyInitialized,
    InvalidPermit,
}

impl fmt::Display for InitializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidToken => formatter.write_str("initialization is not authorized"),
            Self::InProgress => formatter.write_str("initialization is already in progress"),
            Self::AlreadyInitialized => formatter.write_str("initialization is already complete"),
            Self::InvalidPermit => formatter.write_str("initialization permit is invalid"),
        }
    }
}

impl std::error::Error for InitializationError {}
