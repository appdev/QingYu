use std::fmt;

use tokio::sync::broadcast;

use crate::contract::{DomainEvent, EventSequence, ResourceRefDto, Revision, MAX_SAFE_INTEGER};

const DEFAULT_EVENT_CAPACITY: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct EventPublication {
    pub resource: ResourceRefDto,
    pub revision: Revision,
    pub event: DomainEvent,
}

/// Synchronous host publication boundary.
///
/// The sync run's pre-existing `Attempting` event is emitted while the Kernel
/// mutation permit is held. A sink must therefore not synchronously re-enter a
/// mutation API from that callback. Terminal sync events and workspace-change
/// events are emitted after releasing the mutation permit and may perform
/// ordinary synchronous observation without inheriting that restriction.
pub trait EventSink: Send + Sync {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError>;
}

pub struct EventBroker {
    sender: broadcast::Sender<EventPublication>,
    capacity: usize,
}

impl EventBroker {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(DEFAULT_EVENT_CAPACITY);
        Self {
            sender,
            capacity: DEFAULT_EVENT_CAPACITY,
        }
    }

    pub fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        let _subscriber_count = self.sender.send(publication.clone());
        Ok(())
    }

    pub fn subscribe(&self) -> EventSubscription {
        EventSubscription {
            receiver: self.sender.subscribe(),
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }
}

impl Default for EventBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for EventBroker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventBroker")
            .field("capacity", &self.capacity)
            .field("subscriber_count", &self.subscriber_count())
            .finish()
    }
}

impl EventSink for EventBroker {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        Self::publish(self, publication)
    }
}

pub struct EventSubscription {
    receiver: broadcast::Receiver<EventPublication>,
}

impl EventSubscription {
    pub async fn recv(&mut self) -> Result<EventPublication, EventReceiveError> {
        self.receiver.recv().await.map_err(|error| match error {
            broadcast::error::RecvError::Lagged(_) => EventReceiveError::Lagged,
            broadcast::error::RecvError::Closed => EventReceiveError::Closed,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventReceiveError {
    Lagged,
    Closed,
}

pub struct ConnectionSequence {
    next: u64,
    limit: u64,
    terminal: bool,
}

impl ConnectionSequence {
    pub fn new() -> Self {
        Self {
            next: 1,
            limit: MAX_SAFE_INTEGER,
            terminal: false,
        }
    }

    pub fn with_limit(limit: EventSequence) -> Self {
        Self {
            next: 1,
            limit: limit.get(),
            terminal: false,
        }
    }

    pub(crate) fn terminal_gap(&mut self) -> Option<EventSequence> {
        if self.terminal || self.next > self.limit {
            return None;
        }
        self.terminal = true;
        EventSequence::new(self.next).ok()
    }
}

impl Iterator for ConnectionSequence {
    type Item = ConnectionSequenceStep;

    fn next(&mut self) -> Option<Self::Item> {
        if self.terminal {
            return None;
        }
        let sequence = EventSequence::new(self.next).ok()?;
        if self.next == self.limit {
            self.terminal = true;
            return Some(ConnectionSequenceStep::ExhaustedGap(sequence));
        }
        self.next += 1;
        Some(ConnectionSequenceStep::Event(sequence))
    }
}

impl Default for ConnectionSequence {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ConnectionSequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionSequence")
            .field("next", &self.next)
            .field("limit", &self.limit)
            .field("terminal", &self.terminal)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionSequenceStep {
    Event(EventSequence),
    ExhaustedGap(EventSequence),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventSinkError;

impl std::fmt::Display for EventSinkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("event publication is unavailable")
    }
}

impl std::error::Error for EventSinkError {}
