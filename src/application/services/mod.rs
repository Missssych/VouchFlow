//! Transaction Services

pub mod check;
pub mod physical;
pub mod product;
pub mod redeem;
pub mod stok;
pub mod webhook;

pub use check::*;
pub use physical::*;
pub use product::*;
pub use redeem::*;
pub use stok::*;
pub use webhook::*;

use crate::domain::DbCommand;
use crate::infrastructure::channels::DbCommandSender;

/// Shared flow log helper used by all transaction services.
/// Appends a per-transaction audit log entry via the DB Writer channel.
pub async fn append_flow_log(
    db_cmd_tx: &DbCommandSender,
    tx_id: &str,
    request_id: &str,
    trace_id: &str,
    kategori: &str,
    attempt: i32,
    stage: &str,
    level: &str,
    status: Option<&str>,
    message: &str,
    payload: Option<String>,
    latency_ms: Option<i64>,
) {
    let _ = db_cmd_tx
        .send(DbCommand::AppendTransactionLog {
            tx_id: tx_id.to_string(),
            request_id: request_id.to_string(),
            trace_id: Some(trace_id.to_string()),
            kategori: kategori.to_string(),
            attempt,
            stage: stage.to_string(),
            level: level.to_string(),
            status: status.map(|s| s.to_string()),
            message: message.to_string(),
            payload,
            latency_ms,
        })
        .await;
}
