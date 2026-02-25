//! Redeem Voucher Service
//!
//! Flow:
//! 1. Check circuit breaker
//! 2. Reserve voucher via DbCommand (single-writer pattern)
//! 3. Call ProviderRouter.redeem_voucher(provider_name, msisdn=nomor, serial_number)
//! 4. On success: mark voucher USED via DbCommand, update transaction SUCCESS
//! 5. On failure: release voucher back to ACTIVE via DbCommand, update transaction FAILED

use crate::domain::{DbCommand, ReservedVoucher, TransactionResult, TransactionStatus};
use crate::application::providers::ProviderRouter;
use crate::infrastructure::provider::CircuitBreaker;
use crate::infrastructure::channels::DbCommandSender;
use super::append_flow_log;
use std::time::Instant;

/// Execute redeem voucher transaction
///
/// NOTE: No `db: &Database` parameter — all DB access goes through `db_cmd_tx` channel.
pub async fn execute_redeem(
    tx_id: &str,
    request_id: &str,
    trace_id: &str,
    provider_name: &str,
    kode_addon: &str,
    nomor: &str,
    kategori: &str,
    attempt: i32,
    provider_router: &ProviderRouter,
    circuit_breaker: &CircuitBreaker,
    db_cmd_tx: &DbCommandSender,
) -> TransactionResult {
    append_flow_log(
        db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
        "REDEEM_START", "INFO", Some("PROCESSING"), "Start redeem flow",
        Some(serde_json::json!({
            "provider": provider_name,
            "kode_addon": kode_addon,
            "nomor": nomor
        }).to_string()),
        None,
    ).await;

    // Step 1: Check circuit breaker
    if !circuit_breaker.is_allowed().await {
        append_flow_log(
            db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
            "CIRCUIT_BREAKER_BLOCKED", "WARN", Some("FAILED"),
            "Circuit breaker rejected redeem request", None, None,
        ).await;
        return TransactionResult::failed(
            request_id.to_string(),
            tx_id.to_string(),
            "CB001".to_string(),
            "Service temporarily unavailable".to_string(),
        );
    }
    
    // Step 2: Reserve voucher via DB Writer (single-writer pattern)
    append_flow_log(
        db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
        "STOCK_RESERVE_REQUEST", "INFO", Some("PROCESSING"),
        "Reserving voucher from stock via DB Writer", None, None,
    ).await;
    
    let (reserve_tx, reserve_rx) = tokio::sync::oneshot::channel();
    if let Err(e) = db_cmd_tx.send(DbCommand::ReserveStokVoucher {
        kode_addon: kode_addon.to_string(),
        response_tx: reserve_tx,
    }).await {
        append_flow_log(
            db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
            "CHANNEL_ERROR", "ERROR", Some("FAILED"),
            "Failed to send reserve command", Some(e.to_string()), None,
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
    let voucher = match reserve_rx.await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            let error_msg = format!("Stock reserve failed: {}", e);
            append_flow_log(
                db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                "STOCK_EMPTY", "WARN", Some("FAILED"),
                "No voucher available in stock",
                Some(serde_json::json!({ "kode_addon": kode_addon }).to_string()),
                None,
            ).await;
            let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
                tx_id: tx_id.to_string(),
                status: TransactionStatus::Failed,
                sn: None,
                result_code: Some("STK001".to_string()),
                result_payload: Some(error_msg.clone()),
            }).await;
            return TransactionResult::failed(
                request_id.to_string(), tx_id.to_string(),
                "STK001".to_string(), error_msg,
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
        "STOCK_RESERVED", "INFO", Some("PROCESSING"),
        "Voucher reserved successfully",
        Some(serde_json::json!({
            "voucher_id": voucher.voucher_id,
            "barcode": voucher.barcode,
            "serial_number": voucher.serial_number
        }).to_string()),
        None,
    ).await;

    tracing::info!(
        tx_id = %tx_id,
        provider = %provider_name,
        msisdn = %nomor,
        barcode = %voucher.barcode,
        serial_number = %voucher.serial_number,
        "Calling provider redeem API"
    );
    
    // Step 3: Call Provider API
    append_flow_log(
        db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
        "PROVIDER_REDEEM_REQUEST", "INFO", Some("PROCESSING"),
        "Calling provider redeem API", None, None,
    ).await;
    
    let provider_started = Instant::now();
    match provider_router.redeem_voucher(provider_name, nomor, &voucher.serial_number).await {
        Ok(response) => {
            let provider_latency = provider_started.elapsed().as_millis() as i64;
            circuit_breaker.record_success().await;
            
            let result_data = serde_json::json!({
                "msisdn": response.msisdn,
                "barcode": voucher.barcode,
                "serial_number": voucher.serial_number,
                "message": response.message,
                "transaction_id": response.transaction_id,
                "success": response.success,
            });
            let result_payload = result_data.to_string();
            
            append_flow_log(
                db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                "PROVIDER_REDEEM_RESPONSE",
                if response.success { "INFO" } else { "WARN" },
                Some(if response.success { "SUCCESS" } else { "FAILED" }),
                "Provider redeem API completed",
                Some(result_payload.clone()),
                Some(provider_latency),
            ).await;
            
            if response.success {
                // Step 4a: SUCCESS - Mark voucher as USED via DB Writer
                let _ = db_cmd_tx.send(DbCommand::MarkStokUsed {
                    voucher_id: voucher.voucher_id,
                }).await;
                
                append_flow_log(
                    db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                    "STOCK_MARK_USED", "INFO", Some("SUCCESS"),
                    "Voucher marked as USED",
                    Some(serde_json::json!({ "voucher_id": voucher.voucher_id }).to_string()),
                    None,
                ).await;
                
                let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
                    tx_id: tx_id.to_string(),
                    status: TransactionStatus::Success,
                    sn: Some(voucher.barcode.clone()),
                    result_code: Some("00".to_string()),
                    result_payload: Some(result_payload.clone()),
                }).await;
                
                append_flow_log(
                    db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                    "REDEEM_FINISHED", "INFO", Some("SUCCESS"),
                    "Redeem flow finished successfully", None, None,
                ).await;
                
                TransactionResult::success(
                    request_id.to_string(),
                    tx_id.to_string(),
                    Some(result_payload),
                )
            } else {
                // Step 4b: FAILED - Release voucher back to ACTIVE via DB Writer
                let _ = db_cmd_tx.send(DbCommand::ReleaseStokVoucher {
                    voucher_id: voucher.voucher_id,
                }).await;
                
                append_flow_log(
                    db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                    "STOCK_RELEASED", "WARN", Some("FAILED"),
                    "Voucher released back to ACTIVE",
                    Some(serde_json::json!({ "voucher_id": voucher.voucher_id }).to_string()),
                    None,
                ).await;
                
                let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
                    tx_id: tx_id.to_string(),
                    status: TransactionStatus::Failed,
                    sn: Some(voucher.barcode.clone()),
                    result_code: Some("99".to_string()),
                    result_payload: Some(result_payload.clone()),
                }).await;
                
                append_flow_log(
                    db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                    "REDEEM_FINISHED", "WARN", Some("FAILED"),
                    "Redeem flow finished with business failure", None, None,
                ).await;
                
                TransactionResult::failed(
                    request_id.to_string(),
                    tx_id.to_string(),
                    "99".to_string(),
                    format!("Redeem failed: {}", response.message.unwrap_or_else(|| "Unknown error".to_string())),
                )
            }
        }
        Err(e) => {
            let provider_latency = provider_started.elapsed().as_millis() as i64;
            circuit_breaker.record_failure().await;
            let provider_error = format!("Provider error: {}", e);
            
            // Step 5: ERROR - Release voucher back to ACTIVE via DB Writer
            let _ = db_cmd_tx.send(DbCommand::ReleaseStokVoucher {
                voucher_id: voucher.voucher_id,
            }).await;
            
            append_flow_log(
                db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                "STOCK_RELEASED", "WARN", Some("FAILED"),
                "Voucher released after provider error",
                Some(serde_json::json!({ "voucher_id": voucher.voucher_id }).to_string()),
                None,
            ).await;
            
            let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
                tx_id: tx_id.to_string(),
                status: TransactionStatus::Failed,
                sn: Some(voucher.barcode.clone()),
                result_code: Some("ERR001".to_string()),
                result_payload: Some(provider_error.clone()),
            }).await;
            
            append_flow_log(
                db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                "PROVIDER_REDEEM_ERROR", "ERROR", Some("FAILED"),
                "Provider redeem API failed",
                Some(provider_error.clone()),
                Some(provider_latency),
            ).await;
            append_flow_log(
                db_cmd_tx, tx_id, request_id, trace_id, kategori, attempt,
                "REDEEM_FINISHED", "ERROR", Some("FAILED"),
                "Redeem flow finished with provider error", None, None,
            ).await;
            
            TransactionResult::failed(
                request_id.to_string(),
                tx_id.to_string(),
                "ERR001".to_string(),
                provider_error,
            )
        }
    }
}
