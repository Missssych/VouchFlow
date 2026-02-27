//! Command Bus - Gateway/UI to Orchestrator communication

use crate::domain::Command;
use tokio::sync::mpsc;

/// Command bus sender
pub type CommandSender = mpsc::Sender<Command>;

/// Command bus receiver  
pub type CommandReceiver = mpsc::Receiver<Command>;

/// Create bounded command bus
pub fn create_command_bus(capacity: usize) -> (CommandSender, CommandReceiver) {
    mpsc::channel(capacity)
}
