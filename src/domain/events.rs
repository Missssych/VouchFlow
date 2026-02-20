//! Event types for the application
//! 
//! Events are published by DB Writer after successful commits

use serde::{Deserialize, Serialize};
use super::models::{TransactionStatus, TransactionSummary, LogEntry, ProductSummary, StokVoucherSummary, StokAddonSummary};

/// Domain events published after DB commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    /// Transaction status changed
    TransactionUpdated {
        seq: u64,
        tx_id: String,
        request_id: String,
        status: TransactionStatus,
        tx_type: String,
        summary: TransactionSummary,
    },
    /// Log entry appended
    LogAppended {
        seq: u64,
        entry: LogEntry,
    },
    /// All logs cleared
    LogsCleared {
        seq: u64,
    },
    /// Configuration changed
    ConfigChanged {
        seq: u64,
        key: String,
        value: String,
    },
    /// Product created
    ProductCreated {
        seq: u64,
        product: ProductSummary,
    },
    /// Product updated
    ProductUpdated {
        seq: u64,
        product: ProductSummary,
    },
    /// Product(s) deleted
    ProductDeleted {
        seq: u64,
        ids: Vec<i64>,
    },
    /// Stok voucher created
    StokVoucherCreated {
        seq: u64,
        voucher: StokVoucherSummary,
    },
    /// Stok voucher updated
    StokVoucherUpdated {
        seq: u64,
        voucher: StokVoucherSummary,
    },
    /// Stok voucher(s) deleted
    StokVoucherDeleted {
        seq: u64,
        ids: Vec<i64>,
    },
    /// Stok voucher status changed
    StokStatusChanged {
        seq: u64,
        ids: Vec<i64>,
        new_status: String,
    },
}

impl DomainEvent {
    /// Get sequence number for event consistency
    pub fn seq(&self) -> u64 {
        match self {
            DomainEvent::TransactionUpdated { seq, .. } => *seq,
            DomainEvent::LogAppended { seq, .. } => *seq,
            DomainEvent::LogsCleared { seq, .. } => *seq,
            DomainEvent::ConfigChanged { seq, .. } => *seq,
            DomainEvent::ProductCreated { seq, .. } => *seq,
            DomainEvent::ProductUpdated { seq, .. } => *seq,
            DomainEvent::ProductDeleted { seq, .. } => *seq,
            DomainEvent::StokVoucherCreated { seq, .. } => *seq,
            DomainEvent::StokVoucherUpdated { seq, .. } => *seq,
            DomainEvent::StokVoucherDeleted { seq, .. } => *seq,
            DomainEvent::StokStatusChanged { seq, .. } => *seq,
        }
    }
}

/// UI-specific events for thread-safe updates
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Refresh monitoring counters
    RefreshMonitoring {
        total_transactions: i64,
        success_count: i64,
        failed_count: i64,
        tps: f64,
    },
    /// Logs list updated (full list)
    LogsUpdated(Vec<LogEntry>),
    /// Transaction list update
    TransactionsUpdated(Vec<TransactionSummary>),
    /// Configuration loaded
    ConfigLoaded {
        key: String,
        value: String,
    },
    /// Products list updated
    ProductsUpdated(Vec<ProductSummary>),
    /// Stok voucher list updated (active stocks)
    StokVouchersUpdated(Vec<StokVoucherSummary>),
    /// Used voucher list updated
    UsedVouchersUpdated(Vec<StokVoucherSummary>),
    /// Stok addon summary updated
    StokAddonSummaryUpdated(Vec<StokAddonSummary>),
}

