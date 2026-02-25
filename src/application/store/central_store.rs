//! Central Store - In-memory Read Model for UI
//!
//! This store maintains UI-ready state updated from Event Bus.
//! UI reads from here, never directly from database.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::Utc;

use crate::domain::{
    DomainEvent, TransactionSummary, LogEntry, 
    MonitoringCounters, ConfigEntry, DomainError,
};
use crate::infrastructure::database::Database;

/// Menu types for gated rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuType {
    Dashboard,
    Transaksi,
    MasterData,
    Produk,
    Settings,
    Utility,
    Logs,
}

impl Default for MenuType {
    fn default() -> Self {
        Self::Dashboard
    }
}

/// Ring buffer for logs with fixed capacity
#[derive(Debug)]
pub struct RingBuffer<T> {
    buffer: VecDeque<T>,
    capacity: usize,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
        }
    }
    
    pub fn push(&mut self, item: T) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(item);
    }
    
    pub fn items(&self) -> impl Iterator<Item = &T> {
        self.buffer.iter()
    }
    
    pub fn to_vec(&self) -> Vec<T> where T: Clone {
        self.buffer.iter().cloned().collect()
    }
    
    pub fn len(&self) -> usize {
        self.buffer.len()
    }
    
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// Central Store - Read Model
pub struct CentralStore {
    /// Recent transactions (limited to last N)
    pub recent_transactions: Arc<RwLock<VecDeque<TransactionSummary>>>,
    /// Monitoring counters for dashboard
    pub monitoring_counters: Arc<RwLock<MonitoringCounters>>,
    /// Log ring buffer
    pub log_buffer: Arc<RwLock<RingBuffer<LogEntry>>>,
    /// Configurations cache
    pub configs: Arc<RwLock<HashMap<String, ConfigEntry>>>,
    /// Currently active menu (for gated rendering)
    pub active_menu: Arc<RwLock<MenuType>>,
    /// Last event sequence number
    pub last_seq: Arc<AtomicU64>,
    /// Global notification for UI updates
    pub notify: Arc<tokio::sync::Notify>,
    /// Transaction count limits
    max_transactions: usize,
    /// Log buffer size
    log_buffer_size: usize,
}

impl CentralStore {
    /// Create new central store with defaults
    pub fn new() -> Self {
        Self::with_capacity(100, 1000)
    }
    
    /// Create store with custom capacities
    pub fn with_capacity(max_transactions: usize, log_buffer_size: usize) -> Self {
        Self {
            recent_transactions: Arc::new(RwLock::new(VecDeque::with_capacity(max_transactions))),
            monitoring_counters: Arc::new(RwLock::new(MonitoringCounters::default())),
            log_buffer: Arc::new(RwLock::new(RingBuffer::new(log_buffer_size))),
            configs: Arc::new(RwLock::new(HashMap::new())),
            active_menu: Arc::new(RwLock::new(MenuType::default())),
            last_seq: Arc::new(AtomicU64::new(0)),
            notify: Arc::new(tokio::sync::Notify::new()),
            max_transactions,
            log_buffer_size,
        }
    }
    
    /// Hydrate store from database on startup
    pub async fn hydrate_from_db(&self, db: &Database) -> Result<(), DomainError> {
        // Load recent transactions
        db.with_reader(|conn| {
            let mut stmt = conn.prepare(
                "SELECT tx_id, request_id, provider, kategori, harga, produk, nomor, sn, status, result_code, created_at 
                 FROM transactions ORDER BY created_at DESC LIMIT ?"
            )?;
            
            let transactions: Vec<TransactionSummary> = stmt.query_map(
                [self.max_transactions as i64],
                |row| {
                    Ok(TransactionSummary {
                        tx_id: row.get(0)?,
                        request_id: row.get(1)?,
                        provider: row.get(2)?,
                        kategori: row.get(3)?,
                        harga: row.get(4)?,
                        produk: row.get(5)?,
                        nomor: row.get(6)?,
                        sn: row.get(7)?,
                        status: row.get(8)?,
                        result_code: row.get(9)?,
                        created_at: row.get(10)?,
                    })
                }
            )?.filter_map(|r| r.ok()).collect();
            
            let mut tx_guard = futures::executor::block_on(self.recent_transactions.write());
            for tx in transactions {
                tx_guard.push_back(tx);
            }
            
            Ok(())
        }).await?;
        
        // Load monitoring counters
        db.with_reader(|conn| {
            let (total, success, failed, pending): (i64, i64, i64, i64) = conn.query_row(
                "SELECT 
                    COUNT(*),
                    SUM(CASE WHEN status = 'SUCCESS' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'FAILED' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status IN ('PENDING', 'PROCESSING') THEN 1 ELSE 0 END)
                 FROM transactions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).unwrap_or((0, 0, 0, 0));
            
            let mut counters = futures::executor::block_on(self.monitoring_counters.write());
            counters.total_transactions = total;
            counters.success_count = success;
            counters.failed_count = failed;
            counters.pending_count = pending;
            counters.last_updated = Some(Utc::now());
            
            Ok(())
        }).await?;
        
        // Load configurations
        db.with_reader(|conn| {
            let mut stmt = conn.prepare(
                "SELECT key, value, category, description, updated_at FROM configurations"
            )?;
            
            let configs: Vec<ConfigEntry> = stmt.query_map([], |row| {
                let updated_str: String = row.get(4)?;
                let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                    
                Ok(ConfigEntry {
                    key: row.get(0)?,
                    value: row.get(1)?,
                    category: row.get(2)?,
                    description: row.get(3)?,
                    updated_at,
                })
            })?.filter_map(|r| r.ok()).collect();
            
            let mut config_guard = futures::executor::block_on(self.configs.write());
            for c in configs {
                config_guard.insert(c.key.clone(), c);
            }
            
            Ok(())
        }).await?;

        // Load last logs (bounded by ring buffer capacity)
        db.with_reader(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, level, message, trace_id, created_at
                 FROM logs
                 ORDER BY created_at DESC
                 LIMIT ?"
            )?;

            let mut logs: Vec<LogEntry> = stmt.query_map(
                [self.log_buffer_size as i64],
                |row| {
                    let created_at_str: String = row.get(4)?;
                    let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());

                    Ok(LogEntry {
                        id: row.get(0)?,
                        level: row.get(1)?,
                        message: row.get(2)?,
                        trace_id: row.get(3)?,
                        created_at,
                    })
                }
            )?.filter_map(|r| r.ok()).collect();

            // Query is DESC, ring buffer should be oldest -> newest.
            logs.reverse();

            let mut log_guard = futures::executor::block_on(self.log_buffer.write());
            for entry in logs {
                log_guard.push(entry);
            }

            Ok(())
        }).await?;
        
        tracing::info!("Central Store hydrated from database");
        Ok(())
    }
    
    /// Handle domain event and update state
    pub async fn handle_event(&self, event: DomainEvent) {
        let seq = event.seq();
        self.last_seq.store(seq, Ordering::SeqCst);
        
        match event {
            DomainEvent::TransactionUpdated { summary, status, .. } => {
                // Update recent transactions
                let mut transactions = self.recent_transactions.write().await;
                
                // Remove existing with same tx_id if exists
                transactions.retain(|t| t.tx_id != summary.tx_id);
                
                // Add to front
                transactions.push_front(summary);
                
                // Limit size
                while transactions.len() > self.max_transactions {
                    transactions.pop_back();
                }
                
                // Update counters
                // Only increment total for initial status (Processing/Pending),
                // not for status updates (Success/Failed/Expired)
                let mut counters = self.monitoring_counters.write().await;
                match status {
                    crate::domain::TransactionStatus::Processing |
                    crate::domain::TransactionStatus::Pending => {
                        counters.total_transactions += 1;
                        counters.pending_count += 1;
                    }
                    crate::domain::TransactionStatus::Success => {
                        counters.success_count += 1;
                        counters.pending_count = counters.pending_count.saturating_sub(1);
                    }
                    crate::domain::TransactionStatus::Failed |
                    crate::domain::TransactionStatus::Expired => {
                        counters.failed_count += 1;
                        counters.pending_count = counters.pending_count.saturating_sub(1);
                    }
                }
                counters.last_updated = Some(Utc::now());
            }
            DomainEvent::LogAppended { entry, .. } => {
                let mut logs = self.log_buffer.write().await;
                logs.push(entry);
            }
            DomainEvent::LogsCleared { .. } => {
                let mut logs = self.log_buffer.write().await;
                logs.clear();
            }
            DomainEvent::ConfigChanged { key, value, .. } => {
                let mut configs = self.configs.write().await;
                configs.entry(key.clone())
                    .and_modify(|e| {
                        e.value = value.clone();
                        e.updated_at = Utc::now();
                    })
                    .or_insert(crate::domain::models::ConfigEntry {
                        key,
                        value,
                        category: "General".to_string(),
                        description: None,
                        updated_at: Utc::now(),
                    });
            }
            // Product events are handled via ProductService read operations
            // No in-memory cache for products as they are less frequently changed
            DomainEvent::ProductCreated { .. } |
            DomainEvent::ProductUpdated { .. } |
            DomainEvent::ProductDeleted { .. } => {
                // Products are read directly from DB via ProductService
                // This allows UI to refresh product list when needed
            }
            // Stok Voucher events are handled via StokService read operations
            DomainEvent::StokVoucherCreated { .. } |
            DomainEvent::StokVoucherUpdated { .. } |
            DomainEvent::StokVoucherDeleted { .. } |
            DomainEvent::StokStatusChanged { .. } => {
                // Stok vouchers are read directly from DB via StokService
                // This allows UI to refresh when needed
            }
        }
        
        // Notify listeners (UI bridge) that state has changed
        self.notify.notify_one();
    }
    
    /// Set active menu
    pub async fn set_active_menu(&self, menu: MenuType) {
        *self.active_menu.write().await = menu;
    }
    
    /// Get active menu
    pub async fn get_active_menu(&self) -> MenuType {
        *self.active_menu.read().await
    }
    
    /// Get recent transactions
    pub async fn get_recent_transactions(&self) -> Vec<TransactionSummary> {
        self.recent_transactions.read().await.iter().cloned().collect()
    }
    
    /// Get monitoring counters
    pub async fn get_monitoring_counters(&self) -> MonitoringCounters {
        self.monitoring_counters.read().await.clone()
    }
    
    /// Get logs
    pub async fn get_logs(&self) -> Vec<LogEntry> {
        self.log_buffer.read().await.to_vec()
    }
    
    /// Get config value
    pub async fn get_config(&self, key: &str) -> Option<String> {
        self.configs.read().await.get(key).map(|c| c.value.clone())
    }
}

impl Default for CentralStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CentralStore {
    fn clone(&self) -> Self {
        Self {
            recent_transactions: Arc::clone(&self.recent_transactions),
            monitoring_counters: Arc::clone(&self.monitoring_counters),
            log_buffer: Arc::clone(&self.log_buffer),
            configs: Arc::clone(&self.configs),
            active_menu: Arc::clone(&self.active_menu),
            last_seq: Arc::clone(&self.last_seq),
            notify: Arc::clone(&self.notify),
            max_transactions: self.max_transactions,
            log_buffer_size: self.log_buffer_size,
        }
    }
}
