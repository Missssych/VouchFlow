//! Physical Voucher Service
//!
//! Handles physical voucher retrieval from local stock.
//!
//! Flow:
//! 1. Reserve batch vouchers via DbCommand (single-writer pattern)
//! 2. Mark all as USED via DbCommand
//! 3. On error: release back to ACTIVE via DbCommand

use crate::domain::{DbCommand, TransactionResult, TransactionStatus};
use crate::infrastructure::channels::DbCommandSender;
use super::append_flow_log;

/// Execute physical voucher transaction (get from stock)
///
/// NOTE: No `db: &Database` parameter — all DB access goes through `db_cmd_tx` channel.
pub async fn execute_physical(
    tx_id: &str,
    request_id: &str,
    trace_id: &str,
    kode_addon: &str,
    _nomor: &str,
    qty: i32,
    kategori: &str,
    attempt: i32,
    db_cmd_tx: &DbCommandSender,
) -> TransactionResult {
    append_flow_log(
        db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
        "FIS_START", "INFO", Some("PROCESSING"),
        "Start physical voucher flow",
        Some(serde_json::json!({ "kode_addon": kode_addon, "qty": qty }).to_string()),
        None,
    ).await;

    if qty <= 0 {
        append_flow_log(
            db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
            "FIS_INVALID_QTY", "WARN", Some("FAILED"),
            "Invalid quantity requested",
            Some(serde_json::json!({ "qty": qty }).to_string()),
            None,
        ).await;
        return TransactionResult::failed(
            request_id.to_string(),
            tx_id.to_string(),
            "REQ001".to_string(),
            "Invalid quantity".to_string(),
        );
    }

    // Step 1: Reserve batch vouchers via DB Writer (single-writer pattern)
    append_flow_log(
        db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
        "FIS_STOCK_LOOKUP", "INFO", Some("PROCESSING"),
        "Reserving physical vouchers from stock via DB Writer", None, None,
    ).await;

    let (reserve_tx, reserve_rx) = tokio::sync::oneshot::channel();
    if let Err(e) = db_cmd_tx.send(DbCommand::ReserveStokVoucherBatch {
        kode_addon: kode_addon.to_string(),
        qty,
        response_tx: reserve_tx,
    }).await {
        append_flow_log(
            db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
            "CHANNEL_ERROR", "ERROR", Some("FAILED"),
            "Failed to send reserve batch command", Some(e.to_string()), None,
        ).await;
        let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
            tx_id: tx_id.to_string(),
            status: TransactionStatus::Failed,
            sn: None,
            result_code: Some("SYS001".to_string()),
            result_payload: Some("Internal channel error".to_string()),
        }).await;
        return TransactionResult::failed(
            request_id.to_string(), tx_id.to_string(),
            "SYS001".to_string(), "Internal channel error".to_string(),
        );
    }

    // Wait for DB Writer response
    let vouchers = match reserve_rx.await {
        Ok(Ok(v)) => v,
        Ok(Err(_e)) => {
            append_flow_log(
                db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                "FIS_STOCK_EMPTY", "WARN", Some("FAILED"),
                "Insufficient physical stock",
                Some(serde_json::json!({ "kode_addon": kode_addon, "qty": qty }).to_string()),
                None,
            ).await;
            let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
                tx_id: tx_id.to_string(),
                status: TransactionStatus::Failed,
                sn: None,
                result_code: Some("STK001".to_string()),
                result_payload: Some("Insufficient stock".to_string()),
            }).await;
            return TransactionResult::failed(
                request_id.to_string(), tx_id.to_string(),
                "STK001".to_string(), "Insufficient stock".to_string(),
            );
        }
        Err(_) => {
            append_flow_log(
                db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                "CHANNEL_CLOSED", "ERROR", Some("FAILED"),
                "DB Writer channel closed", None, None,
            ).await;
            let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
                tx_id: tx_id.to_string(),
                status: TransactionStatus::Failed,
                sn: None,
                result_code: Some("SYS002".to_string()),
                result_payload: Some("DB Writer unavailable".to_string()),
            }).await;
            return TransactionResult::failed(
                request_id.to_string(), tx_id.to_string(),
                "SYS002".to_string(), "DB Writer unavailable".to_string(),
            );
        }
    };

    append_flow_log(
        db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
        "FIS_STOCK_RESERVED", "INFO", Some("PROCESSING"),
        "Physical vouchers reserved",
        Some(serde_json::json!({ "count": vouchers.len() }).to_string()),
        None,
    ).await;

    // Step 2: Mark all reserved vouchers as USED via DB Writer
    for v in &vouchers {
        let _ = db_cmd_tx.send(DbCommand::MarkStokUsed {
            voucher_id: v.voucher_id,
        }).await;
    }

    let barcodes: Vec<String> = vouchers.iter().map(|v| v.barcode.clone()).collect();
    let serial_numbers: Vec<String> = vouchers.iter().map(|v| v.serial_number.clone()).collect();
    let first_exp = vouchers.first().and_then(|v| v.expired_date.clone());
    let payload = serde_json::json!({
        "kode_addon": kode_addon,
        "barcode": barcodes.first().cloned(),
        "serial_number": serial_numbers.first().cloned(),
        "exp": first_exp,
        "barcodes": barcodes,
        "serial_numbers": serial_numbers,
        "count": vouchers.len()
    }).to_string();

    append_flow_log(
        db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
        "FIS_MARK_USED", "INFO", Some("SUCCESS"),
        "Physical vouchers marked as USED",
        Some(payload.clone()), None,
    ).await;

    let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
        tx_id: tx_id.to_string(),
        status: TransactionStatus::Success,
        sn: serial_numbers.first().cloned(),
        result_code: Some("00".to_string()),
        result_payload: Some(payload.clone()),
    }).await;

    append_flow_log(
        db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
        "FIS_FINISHED", "INFO", Some("SUCCESS"),
        "Physical voucher flow finished successfully", None, None,
    ).await;

    TransactionResult::success(
        request_id.to_string(),
        tx_id.to_string(),
        Some(payload),
    )
}
