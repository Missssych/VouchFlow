//! HTTP Request Handlers
//!
//! Unified endpoint for all transaction types

use axum::{
    extract::{Query, State, Path},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use std::time::Duration;

use crate::domain::{Command, TransactionResult, TransactionType};
use super::server::AppState;

/// Query parameters for transaksi endpoint
#[derive(Debug, Deserialize)]
pub struct TransaksiParams {
    /// Transaction ID for idempotency
    pub idtrx: String,
    /// Target number (voucher code, phone, etc)
    pub nomor: String,
    /// Product code (determines transaction type via prefix)
    pub produk: String,
}

/// API Response wrapper
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }
    
    pub fn error(msg: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg),
        }
    }
}

/// Unified transaction response format for `/api/v1/transaksi`
#[derive(Debug, Serialize)]
pub struct TransaksiApiResponse {
    pub success: bool,
    pub nomor: String,
    pub produk: String,
    pub message: String,
    pub data: Option<Value>,
    pub idtrx: String,
}

impl TransaksiApiResponse {
    fn failed(idtrx: String, nomor: String, produk: String, message: String) -> Self {
        Self {
            success: false,
            nomor,
            produk,
            message,
            data: None,
            idtrx,
        }
    }

    fn pending(idtrx: String, nomor: String, produk: String) -> Self {
        Self {
            success: true,
            nomor,
            produk,
            message: "transaksi sedang diproses".to_string(),
            data: None,
            idtrx,
        }
    }

    fn from_result(
        result: TransactionResult,
        nomor: String,
        produk: String,
        tx_type: TransactionType,
        harga: f64,
    ) -> Self {
        let payload = parse_payload(result.result_payload.as_deref());

        if result.success {
            let (message, data) = match tx_type {
                TransactionType::Check => (
                    "Cek voucher berhasil".to_string(),
                    payload.as_ref().map(build_cek_data),
                ),
                TransactionType::Physical => (
                    "Transaksi voucher fisik berhasil".to_string(),
                    payload.as_ref().map(|p| build_fis_data(p, harga)),
                ),
                TransactionType::Redeem => (
                    "Transaksi redeem voucher berhasil".to_string(),
                    payload.as_ref().map(|p| build_rdm_data(p, harga)),
                ),
            };

            Self {
                success: true,
                nomor,
                produk,
                message,
                data,
                idtrx: result.request_id,
            }
        } else {
            Self {
                success: false,
                nomor,
                produk,
                message: result.message.unwrap_or_else(|| "Transaksi gagal".to_string()),
                data: payload,
                idtrx: result.request_id,
            }
        }
    }
}

fn parse_payload(payload: Option<&str>) -> Option<Value> {
    payload.and_then(|p| serde_json::from_str::<Value>(p).ok())
}

fn get_string_field(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}

fn get_first_array_string(payload: &Value, key: &str) -> Option<String> {
    payload.get(key)
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        })
}

fn build_cek_data(payload: &Value) -> Value {
    json!({
        "expiry_date": payload.get("expiry_date").cloned().unwrap_or(Value::Null),
        "product_name": payload.get("product_name").cloned().unwrap_or(Value::Null),
        "status": payload.get("status").cloned().unwrap_or(Value::Null),
    })
}

fn build_fis_data(payload: &Value, harga: f64) -> Value {
    let barcode = get_string_field(payload, "barcode")
        .or_else(|| get_first_array_string(payload, "barcodes"));
    let serial_number = get_string_field(payload, "serial_number")
        .or_else(|| get_first_array_string(payload, "serial_numbers"));
    let exp = get_string_field(payload, "exp")
        .or_else(|| get_string_field(payload, "expiry_date"));

    json!({
        "barcode": barcode,
        "serial_number": serial_number,
        "exp": exp,
        "harga": harga,
    })
}

fn build_rdm_data(payload: &Value, harga: f64) -> Value {
    let barcode = get_string_field(payload, "barcode");
    json!({
        "barcode": barcode,
        "harga": harga,
    })
}

/// Main transaction handler
/// GET /api/v1/transaksi?idtrx=&nomor=&produk=
pub async fn handle_transaksi(
    State(state): State<AppState>,
    Query(params): Query<TransaksiParams>,
) -> Result<Json<TransaksiApiResponse>, StatusCode> {
    // Lookup product from database by kode_produk to get full product info
    let kode_produk = params.produk.clone();
    let product_info: Option<(String, String, String, f64, Option<String>)> = state.db.with_reader(|conn| {
        let result = conn.query_row(
            "SELECT provider, nama_produk, kategori, harga, kode_addon FROM produk WHERE kode_produk = ?1 AND aktif = 1",
            rusqlite::params![kode_produk],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
        ).ok();
        Ok(result)
    }).await.ok().flatten();
    
    // Validate product exists and has valid kategori
    let (provider, nama_produk, kategori, harga, kode_addon) = match product_info {
        Some(info) => info,
        None => {
            // Try fallback to prefix-based detection for backward compatibility
            if let Some(tx_type) = TransactionType::from_product_code(&params.produk) {
                // Create minimal product info for legacy support
                ("Unknown".to_string(), params.produk.clone(), tx_type.as_str().to_string(), 0.0, None)
            } else {
                return Ok(Json(TransaksiApiResponse::failed(
                    params.idtrx,
                    params.nomor,
                    params.produk.clone(),
                    format!("Product not found or invalid: {}", params.produk),
                )));
            }
        }
    };
    
    let tx_type = match TransactionType::from_kategori(&kategori) {
        Some(t) => t,
        None => {
            return Ok(Json(TransaksiApiResponse::failed(
                params.idtrx,
                params.nomor,
                params.produk.clone(),
                format!("Invalid kategori: {} for product: {}", kategori, params.produk),
            )));
        }
    };
    
    // Create oneshot channel for sync response
    let (response_tx, response_rx) = oneshot::channel();
    
    // Create command with full product info
    let command = Command::with_product_info(
        params.idtrx.clone(),
        tx_type,
        provider,
        kode_produk,
        kategori,
        harga,
        kode_addon,
        nama_produk,
        params.nomor.clone(),
        Some(response_tx),
    );
    
    // Send to command bus
    if let Err(e) = state.command_tx.send(command).await {
        tracing::error!("Failed to send command: {}", e);
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    
    // Wait for response with timeout
    match tokio::time::timeout(Duration::from_secs(30), response_rx).await {
        Ok(Ok(result)) => {
            Ok(Json(TransaksiApiResponse::from_result(
                result,
                params.nomor,
                params.produk,
                tx_type,
                harga,
            )))
        }
        Ok(Err(_)) => {
            // Channel closed
            Ok(Json(TransaksiApiResponse::failed(
                params.idtrx,
                params.nomor,
                params.produk,
                "Request processing failed".to_string(),
            )))
        }
        Err(_) => {
            // Timeout - return pending status
            Ok(Json(TransaksiApiResponse::pending(
                params.idtrx,
                params.nomor,
                params.produk,
            )))
        }
    }
}

/// Health check endpoint
pub async fn health_check() -> Json<ApiResponse<String>> {
    Json(ApiResponse::success("OK".to_string()))
}

/// Get transaction status
/// GET /api/v1/status/:request_id
pub async fn get_status(
    Path(request_id): Path<String>,
) -> Json<ApiResponse<String>> {
    // This would query the database for status
    // For now, return placeholder
    Json(ApiResponse::success(format!("Status for: {}", request_id)))
}
