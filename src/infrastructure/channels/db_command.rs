//! DB Command Queue - Orchestrator to DB Writer communication

use tokio::sync::mpsc;
use crate::domain::DbCommand;

/// DB command sender
pub type DbCommandSender = mpsc::Sender<DbCommand>;

/// DB command receiver
pub type DbCommandReceiver = mpsc::Receiver<DbCommand>;

/// Create bounded DB command queue
pub fn create_db_command_queue(capacity: usize) -> (DbCommandSender, DbCommandReceiver) {
    mpsc::channel(capacity)
}
