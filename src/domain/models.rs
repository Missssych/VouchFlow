//! Domain models for the application

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Transaction status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    Pending,
    Processing,
    Success,
    Failed,
    Expired,
}

impl TransactionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Processing => "PROCESSING",
            Self::Success => "SUCCESS",
            Self::Failed => "FAILED",
            Self::Expired => "EXPIRED",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "PENDING" => Some(Self::Pending),
            "PROCESSING" => Some(Self::Processing),
            "SUCCESS" => Some(Self::Success),
            "FAILED" => Some(Self::Failed),
            "EXPIRED" => Some(Self::Expired),
            _ => None,
        }
    }

    pub fn is_final(&self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::Expired)
    }
}

/// Transaction record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub tx_id: String,
    pub request_id: String,
    pub trace_id: String,
    pub tx_type: String,
    pub produk: String,
    pub nomor: String,
    pub status: TransactionStatus,
    pub result_code: Option<String>,
    pub result_payload: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Transaction summary for UI display (matches Dashboard table columns)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionSummary {
    pub tx_id: String,
    pub request_id: String, // Idtrx
    pub provider: String,   // Provider
    pub produk: String,     // Produk
    pub kategori: String,   // Kategori (CEK/RDM/FIS)
    pub harga: f64,         // Harga
    pub nomor: String,      // Nomor
    pub sn: Option<String>, // SN (FIS=serial_number, RDM=barcode, CEK=nomor)
    pub status: String,     // Status
    pub result_code: Option<String>,
    pub created_at: String, // Waktu trx
}

/// Transaction result for API response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionResult {
    pub success: bool,
    pub request_id: String,
    pub tx_id: String,
    pub status: String,
    pub result_code: Option<String>,
    pub result_payload: Option<String>,
    pub message: Option<String>,
}

impl TransactionResult {
    pub fn success(request_id: String, tx_id: String, payload: Option<String>) -> Self {
        Self {
            success: true,
            request_id,
            tx_id,
            status: "SUCCESS".to_string(),
            result_code: Some("00".to_string()),
            result_payload: payload,
            message: None,
        }
    }

    pub fn failed(request_id: String, tx_id: String, code: String, message: String) -> Self {
        Self {
            success: false,
            request_id,
            tx_id,
            status: "FAILED".to_string(),
            result_code: Some(code),
            result_payload: None,
            message: Some(message),
        }
    }

    pub fn pending(request_id: String, tx_id: String) -> Self {
        Self {
            success: true,
            request_id,
            tx_id,
            status: "PENDING".to_string(),
            result_code: None,
            result_payload: None,
            message: Some("Transaksi sedang diproses, hasil akan dikirim via webhook".to_string()),
        }
    }

    /// Convert to unified response format {idtrx, nomor, produk, message}
    pub fn to_unified(&self, nomor: &str, produk: &str) -> UnifiedResponse {
        UnifiedResponse {
            idtrx: self.request_id.clone(),
            nomor: nomor.to_string(),
            produk: produk.to_string(),
            message: self.format_message(),
        }
    }

    /// Format message for unified response
    fn format_message(&self) -> String {
        let status_msg = format!("Status: {}", self.status);

        match (&self.result_code, &self.result_payload, &self.message) {
            (Some(code), Some(payload), _) => {
                format!("{}. Code: {}. {}", status_msg, code, payload)
            }
            (Some(code), None, Some(msg)) => format!("{}. Code: {}. {}", status_msg, code, msg),
            (Some(code), None, None) => format!("{}. Code: {}", status_msg, code),
            (None, Some(payload), _) => format!("{}. {}", status_msg, payload),
            (None, None, Some(msg)) => format!("{}. {}", status_msg, msg),
            (None, None, None) => status_msg,
        }
    }
}

/// Unified response format for API (user-specified format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResponse {
    pub idtrx: String,
    pub nomor: String,
    pub produk: String,
    pub message: String,
}

impl UnifiedResponse {
    /// Create success response for CEK transaction
    pub fn cek_success(
        idtrx: &str,
        nomor: &str,
        produk: &str,
        nominal: f64,
        expired: &str,
    ) -> Self {
        Self {
            idtrx: idtrx.to_string(),
            nomor: nomor.to_string(),
            produk: produk.to_string(),
            message: format!(
                "Status: AVAILABLE. Nominal: {:.0}. Expired: {}",
                nominal, expired
            ),
        }
    }

    /// Create success response for RDM transaction
    pub fn rdm_success(idtrx: &str, nomor: &str, produk: &str, barcode: &str, harga: f64) -> Self {
        Self {
            idtrx: idtrx.to_string(),
            nomor: nomor.to_string(),
            produk: produk.to_string(),
            message: format!("Status: SUCCESS. Barcode: {}. Harga: {:.0}", barcode, harga),
        }
    }

    /// Create success response for FIS transaction
    pub fn fis_success(
        idtrx: &str,
        nomor: &str,
        produk: &str,
        barcode: &str,
        sn: &str,
        expired: &str,
        harga: f64,
    ) -> Self {
        Self {
            idtrx: idtrx.to_string(),
            nomor: nomor.to_string(),
            produk: produk.to_string(),
            message: format!(
                "Status: SUCCESS. Barcode: {}. SN: {}. Expired: {}. Harga: {:.0}",
                barcode, sn, expired, harga
            ),
        }
    }

    /// Create error response
    pub fn error(idtrx: &str, nomor: &str, produk: &str, error_msg: &str) -> Self {
        Self {
            idtrx: idtrx.to_string(),
            nomor: nomor.to_string(),
            produk: produk.to_string(),
            message: format!("Status: FAILED. {}", error_msg),
        }
    }

    /// Convert to webhook raw text format
    pub fn to_webhook_text(&self) -> String {
        format!(
            "idtrx={}|nomor={}|produk={}|message={}",
            self.idtrx, self.nomor, self.produk, self.message
        )
    }
}

/// Product master data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: i64,
    pub provider: String,
    pub nama_produk: String,
    pub kode_produk: String,
    pub kategori: String,
    pub harga: f64,
    pub kode_addon: Option<String>,
    pub aktif: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Product summary for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductSummary {
    pub id: i64,
    pub provider: String,
    pub nama_produk: String,
    pub kode_produk: String,
    pub kategori: String,
    pub harga: String,
    pub kode_addon: String,
}

/// Log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub level: String,
    pub message: String,
    pub trace_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Configuration entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub category: String,
    pub description: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// Monitoring counters for dashboard
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitoringCounters {
    pub total_transactions: i64,
    pub success_count: i64,
    pub failed_count: i64,
    pub pending_count: i64,
    pub tps: f64,
    pub last_updated: Option<DateTime<Utc>>,
}

/// Stock voucher status for Master Data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StokVoucherStatus {
    Active,
    Used,
}

impl StokVoucherStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Used => "USED",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "ACTIVE" => Some(Self::Active),
            "USED" => Some(Self::Used),
            _ => None,
        }
    }
}

/// Stock voucher entity (Master Data)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StokVoucher {
    pub id: i64,
    pub provider: String,
    pub kode_addon: String,
    pub barcode: String,
    pub serial_number: String,
    pub expired_date: Option<String>,
    pub status: StokVoucherStatus,
    pub used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Stock voucher summary for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StokVoucherSummary {
    pub id: i64,
    pub provider: String,
    pub kode_addon: String,
    pub barcode: String,
    pub serial_number: String,
    pub expired_date: String,
    pub status: String,
    pub created_at: String,
}

/// Stock addon summary (count per kode_addon)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StokAddonSummary {
    pub kode_addon: String,
    pub count: i32,
}

/// Check voucher result for dialog display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckVoucherResult {
    pub success: bool,
    pub product_name: Option<String>,
    pub nominal: Option<f64>,
    pub expiry_date: Option<String>,
    pub status: String,
    pub message: Option<String>,
}
