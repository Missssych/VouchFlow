//! Check Voucher Service

use crate::domain::{DbCommand, TransactionResult, TransactionStatus};
use crate::application::providers::ProviderRouter;
use crate::infrastructure::provider::CircuitBreaker;
use crate::infrastructure::channels::DbCommandSender;
use std::time::Instant;

/// Execute check voucher transaction
/// 
/// Routes to the correct provider (Telkomsel, Byu, Smartfren) based on provider_name
pub async fn execute_check(
    tx_id: &str,
    request_id: &str,
    trace_id: &str,
    provider_name: &str,
    produk: &str,
    nomor: &str,  // CEK input number (voucher code)
    kategori: &str,
    attempt: i32,
    provider_router: &ProviderRouter,
    circuit_breaker: &CircuitBreaker,
    db_cmd_tx: &DbCommandSender,
) -> TransactionResult {
    append_flow_log(
        db_cmd_tx,
        tx_id,
        request_id,
        trace_id,
        kategori,
        attempt,
        "CHECK_START",
        "INFO",
        Some("PROCESSING"),
        "Start check voucher flow",
        Some(serde_json::json!({
            "provider": provider_name,
            "produk": produk,
            "nomor": nomor
        }).to_string()),
        None,
    ).await;

    // Check circuit breaker
    if !circuit_breaker.is_allowed().await {
        append_flow_log(
            db_cmd_tx,
            tx_id,
            request_id,
            trace_id,
            kategori,
            attempt,
            "CIRCUIT_BREAKER_BLOCKED",
            "WARN",
            Some("FAILED"),
            "Circuit breaker rejected check request",
            None,
            None,
        ).await;
        return TransactionResult::failed(
            request_id.to_string(),
            tx_id.to_string(),
            "CB001".to_string(),
            "Service temporarily unavailable".to_string(),
        );
    }
    
    append_flow_log(
        db_cmd_tx,
        tx_id,
        request_id,
        trace_id,
        kategori,
        attempt,
        "PROVIDER_CHECK_REQUEST",
        "INFO",
        Some("PROCESSING"),
        "Calling provider check API",
        None,
        None,
    ).await;
    let provider_started = Instant::now();

    // Call provider using ProviderRouter (routes to correct provider API)
    match provider_router.check_voucher(provider_name, nomor).await {
        Ok(response) => {
            let provider_latency = provider_started.elapsed().as_millis() as i64;
            circuit_breaker.record_success().await;
            
            // Update transaction status based on response
            let status = if response.success {
                TransactionStatus::Success
            } else {
                TransactionStatus::Failed
            };
            
            // Format response data
            let result_data = serde_json::json!({
                "barcode": nomor,
                "serial_number": response.serial_number,
                "product_name": response.product_name,
                "nominal": response.nominal,
                "expiry_date": response.expiry_date,
                "status": response.status,
            });
            let result_payload = result_data.to_string();
            
            append_flow_log(
                db_cmd_tx,
                tx_id,
                request_id,
                trace_id,
                kategori,
                attempt,
                "PROVIDER_CHECK_RESPONSE",
                if response.success { "INFO" } else { "WARN" },
                Some(if response.success { "SUCCESS" } else { "FAILED" }),
                "Provider check API completed",
                Some(result_payload.clone()),
                Some(provider_latency),
            ).await;
            
            let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
                tx_id: tx_id.to_string(),
                status,
                sn: Some(nomor.to_string()),
                result_code: Some(if response.success { "00".to_string() } else { "99".to_string() }),
                result_payload: Some(result_payload.clone()),
            }).await;
            
            if response.success {
                append_flow_log(
                    db_cmd_tx,
                    tx_id,
                    request_id,
                    trace_id,
                    kategori,
                    attempt,
                    "CHECK_FINISHED",
                    "INFO",
                    Some("SUCCESS"),
                    "Check flow finished successfully",
                    None,
                    None,
                ).await;
                TransactionResult::success(
                    request_id.to_string(),
                    tx_id.to_string(),
                    Some(result_payload),
                )
            } else {
                append_flow_log(
                    db_cmd_tx,
                    tx_id,
                    request_id,
                    trace_id,
                    kategori,
                    attempt,
                    "CHECK_FINISHED",
                    "WARN",
                    Some("FAILED"),
                    "Check flow finished with business failure",
                    None,
                    None,
                ).await;
                TransactionResult::failed(
                    request_id.to_string(),
                    tx_id.to_string(),
                    "99".to_string(),
                    format!("Check failed: {}", response.status),
                )
            }
        }
        Err(e) => {
            let provider_latency = provider_started.elapsed().as_millis() as i64;
            circuit_breaker.record_failure().await;
            let error_message = format!("Provider error: {}", e);
            
            let _ = db_cmd_tx.send(DbCommand::UpdateTransaction {
                tx_id: tx_id.to_string(),
                status: TransactionStatus::Failed,
                sn: Some(nomor.to_string()),
                result_code: Some("ERR001".to_string()),
                result_payload: Some(error_message.clone()),
            }).await;

            append_flow_log(
                db_cmd_tx,
                tx_id,
                request_id,
                trace_id,
                kategori,
                attempt,
                "PROVIDER_CHECK_ERROR",
                "ERROR",
                Some("FAILED"),
                "Provider check API failed",
                Some(error_message.clone()),
                Some(provider_latency),
            ).await;
            
            TransactionResult::failed(
                request_id.to_string(),
                tx_id.to_string(),
                "ERR001".to_string(),
                error_message,
            )
        }
    }
}

async fn append_flow_log(
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
    let _ = db_cmd_tx.send(DbCommand::AppendTransactionLog {
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
    }).await;
}
