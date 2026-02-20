//! Event Bus - DB Writer to Store/UI broadcast

use tokio::sync::broadcast;
use crate::domain::DomainEvent;

/// Event sender (broadcast)
pub type EventSender = broadcast::Sender<DomainEvent>;

/// Event receiver (broadcast)
pub type EventReceiver = broadcast::Receiver<DomainEvent>;

/// Create event bus for broadcasting domain events
pub fn create_event_bus(capacity: usize) -> (EventSender, EventReceiver) {
    broadcast::channel(capacity)
}
