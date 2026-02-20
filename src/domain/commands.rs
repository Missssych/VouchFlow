//! Command types for the application
//! 
//! Commands represent requests from Gateway/UI to Orchestrator

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use super::models::TransactionResult;

/// Transaction type determined by product prefix
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionType {
    /// CEK_ prefix - Check voucher status
    Check,
    /// RDM_ prefix - Redeem voucher via provider
    Redeem,
    /// FIS_ prefix - Get physical voucher from stock
    Physical,
}

impl TransactionType {
    /// Parse transaction type from product kategori field
    /// kategori is typically "CEK", "RDM", or "FIS" stored in database
    pub fn from_kategori(kategori: &str) -> Option<Self> {
        match kategori.to_uppercase().as_str() {
            "CEK" => Some(Self::Check),
            "RDM" => Some(Self::Redeem),
            "FIS" => Some(Self::Physical),
            _ => None,
        }
    }
    
    /// Parse transaction type from product code prefix (legacy, for backward compatibility)
    pub fn from_product_code(produk: &str) -> Option<Self> {
        if produk.starts_with("CEK_") {
            Some(Self::Check)
        } else if produk.starts_with("RDM_") {
            Some(Self::Redeem)
        } else if produk.starts_with("FIS_") {
            Some(Self::Physical)
        } else {
            None
        }
    }
    
    /// Get string representation for this type
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Check => "CEK",
            Self::Redeem => "RDM",
            Self::Physical => "FIS",
        }
    }
}

/// Command sent from Gateway to Orchestrator via Command Bus
#[derive(Debug)]
pub struct Command {
    /// Unique trace ID for observability
    pub trace_id: String,
    /// Request ID for idempotency
    pub request_id: String,
    /// Transaction type
    pub tx_type: TransactionType,
    /// Product info from database lookup
    pub provider: String,
    pub kode_produk: String,
    pub kategori: String,
    pub harga: f64,
    pub kode_addon: Option<String>,
    /// Product name for display
    pub produk: String,
    /// Target number (voucher code, phone number, etc)
    pub nomor: String,
    /// Timestamp when command was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Optional oneshot sender for sync response
    pub response_tx: Option<oneshot::Sender<TransactionResult>>,
    /// Optional existing transaction ID to re-run flow in-place (admin retry)
    pub retry_tx_id: Option<String>,
}

impl Command {
    /// Create new command with full product info from database lookup
    pub fn with_product_info(
        request_id: String,
        tx_type: TransactionType,
        provider: String,
        kode_produk: String,
        kategori: String,
        harga: f64,
        kode_addon: Option<String>,
        produk: String,
        nomor: String,
        response_tx: Option<oneshot::Sender<TransactionResult>>,
    ) -> Self {
        Self {
            trace_id: uuid::Uuid::new_v4().to_string(),
            request_id,
            tx_type,
            provider,
            kode_produk,
            kategori,
            harga,
            kode_addon,
            produk,
            nomor,
            created_at: chrono::Utc::now(),
            response_tx,
            retry_tx_id: None,
        }
    }

    /// Create retry command that reuses existing transaction row (`tx_id`) and request_id.
    pub fn with_retry_target(mut self, tx_id: String) -> Self {
        self.retry_tx_id = Some(tx_id);
        self
    }
    
    /// Create new command with transaction type from kategori lookup (legacy simple version)
    pub fn with_type(
        request_id: String,
        tx_type: TransactionType,
        produk: String,
        nomor: String,
        response_tx: Option<oneshot::Sender<TransactionResult>>,
    ) -> Self {
        Self {
            trace_id: uuid::Uuid::new_v4().to_string(),
            request_id,
            tx_type,
            provider: String::new(),
            kode_produk: produk.clone(),
            kategori: tx_type.as_str().to_string(),
            harga: 0.0,
            kode_addon: None,
            produk,
            nomor,
            created_at: chrono::Utc::now(),
            response_tx,
            retry_tx_id: None,
        }
    }
    
    /// Create new command with auto-generated trace_id (legacy, for backward compatibility)
    /// Parses tx_type from product code prefix (CEK_, RDM_, FIS_)
    pub fn new(
        request_id: String,
        produk: String,
        nomor: String,
        response_tx: Option<oneshot::Sender<TransactionResult>>,
    ) -> Option<Self> {
        let tx_type = TransactionType::from_product_code(&produk)?;
        Some(Self::with_type(request_id, tx_type, produk, nomor, response_tx))
    }
}

/// Voucher info returned from stock reserve operations
#[derive(Debug, Clone)]
pub struct ReservedVoucher {
    pub voucher_id: i64,
    pub barcode: String,
    pub serial_number: String,
    pub expired_date: Option<String>,
}

/// Database command sent from Orchestrator to DB Writer
///
/// NOTE: Not Clone because some variants contain oneshot::Sender.
/// mpsc channel only requires Send, not Clone.
pub enum DbCommand {
    /// Insert new transaction
    InsertTransaction {
        tx_id: String,
        request_id: String,
        trace_id: String,
        provider: String,
        kode_produk: String,
        kategori: String,
        harga: f64,
        produk: String,
        nomor: String,
    },
    /// Update transaction status
    UpdateTransaction {
        tx_id: String,
        status: super::models::TransactionStatus,
        sn: Option<String>,
        result_code: Option<String>,
        result_payload: Option<String>,
    },
    /// Append log entry
    AppendLog {
        level: String,
        message: String,
        trace_id: Option<String>,
    },
    /// Append per-transaction flow log entry (dashboard detail)
    AppendTransactionLog {
        tx_id: String,
        request_id: String,
        trace_id: Option<String>,
        kategori: String,
        attempt: i32,
        stage: String,
        level: String,
        status: Option<String>,
        message: String,
        payload: Option<String>,
        latency_ms: Option<i64>,
    },
    /// Clear all log entries
    ClearLogs,
    /// Purge old log entries by retention policy (days)
    PurgeOldLogs {
        retention_days: i64,
    },
    /// Save configuration
    SaveConfig {
        key: String,
        value: String,
        category: String,
    },
    
    // ===== Product CRUD Commands =====
    
    /// Create new product
    CreateProduct {
        provider: String,
        nama_produk: String,
        kode_produk: String,
        kategori: String,
        harga: f64,
        kode_addon: Option<String>,
    },
    /// Update existing product
    UpdateProduct {
        id: i64,
        provider: String,
        nama_produk: String,
        kode_produk: String,
        kategori: String,
        harga: f64,
        kode_addon: Option<String>,
    },
    /// Delete product by ID
    DeleteProduct {
        id: i64,
    },
    /// Delete multiple products by IDs
    DeleteProducts {
        ids: Vec<i64>,
    },
    
    // ===== Stok Voucher CRUD Commands =====
    
    /// Create new stok voucher
    CreateStokVoucher {
        provider: String,
        kode_addon: String,
        barcode: String,
        serial_number: String,
        expired_date: Option<String>,
    },
    /// Update existing stok voucher
    UpdateStokVoucher {
        id: i64,
        provider: String,
        kode_addon: String,
        barcode: String,
        serial_number: String,
        expired_date: Option<String>,
    },
    /// Delete stok voucher by ID
    DeleteStokVoucher {
        id: i64,
    },
    /// Delete multiple stok vouchers by IDs
    DeleteStokVouchers {
        ids: Vec<i64>,
    },
    /// Change status of stok voucher (ACTIVE <-> USED)
    ChangeStokStatus {
        ids: Vec<i64>,
        new_status: String,  // "ACTIVE" or "USED"
    },
    
    // ===== Stock Operations (Single-Writer) =====
    
    /// Reserve a single voucher from stock (FIFO by expired_date)
    /// Used by redeem flow. Atomically finds ACTIVE voucher and marks RESERVED.
    ReserveStokVoucher {
        kode_addon: String,
        response_tx: tokio::sync::oneshot::Sender<Result<ReservedVoucher, super::DomainError>>,
    },
    /// Reserve multiple vouchers from stock (FIFO by expired_date)
    /// Used by physical voucher flow.
    ReserveStokVoucherBatch {
        kode_addon: String,
        qty: i32,
        response_tx: tokio::sync::oneshot::Sender<Result<Vec<ReservedVoucher>, super::DomainError>>,
    },
    /// Release a reserved voucher back to ACTIVE (on failure)
    ReleaseStokVoucher {
        voucher_id: i64,
    },
    /// Mark a reserved voucher as USED (on success)
    MarkStokUsed {
        voucher_id: i64,
    },
    
    // ===== Admin Transaction Actions =====
    
    /// Manual success for pending transaction
    ManualSuccess {
        tx_id: String,
        result_code: String,
        result_payload: Option<String>,
    },
    /// Retry pending transaction
    RetryTransaction {
        tx_id: String,
        kategori: String,
        produk: String,
        nomor: String,
    },
}

