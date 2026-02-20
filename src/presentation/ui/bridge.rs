//! UI Bridge - Thread-safe Slint updates
//!
//! Bridges async backend events to Slint UI using invoke_from_event_loop.
//! UI never directly reads from database - only from Central Store.

use std::sync::Arc;
use tokio::sync::broadcast;
use std::time::Duration;

use crate::domain::{DomainEvent, UiEvent, LogEntry};
use crate::application::store::{CentralStore, MenuType};

/// UI Event listener that bridges domain events to UI updates
pub struct UiBridge {
    store: CentralStore,
    event_rx: broadcast::Receiver<DomainEvent>,
    // Batching configuration
    batch_interval_ms: u64,
    // Pending updates
    pending_logs: Vec<LogEntry>,
    last_flush: std::time::Instant,
    // Command receiver
    command_rx: tokio::sync::mpsc::Receiver<BridgeCommand>,
}

/// Commands sent to UI Bridge
#[derive(Debug)]
pub enum BridgeCommand {
    SetMenu(MenuType),
    RequestLogs,
}

impl UiBridge {
    /// Create new UI bridge
    pub fn new(
        store: CentralStore,
        event_rx: broadcast::Receiver<DomainEvent>,
        command_rx: tokio::sync::mpsc::Receiver<BridgeCommand>,
    ) -> Self {
        Self {
            store,
            event_rx,
            batch_interval_ms: 100,
            pending_logs: Vec::new(),
            last_flush: std::time::Instant::now(),
            command_rx,
        }
    }
    
    /// Run the UI bridge loop
    /// 
    /// This listens for domain events and updates the central store,
    /// then sends UI events via invoke_from_event_loop
    pub async fn run<F>(mut self, ui_callback: F)
    where
        F: Fn(UiEvent) + Send + Sync + 'static,
    {
        tracing::info!("UI Bridge started");
        
        let callback = Arc::new(ui_callback);
        
        loop {
            tokio::select! {
                // Receive domain event
                result = self.event_rx.recv() => {
                    match result {
                        Ok(event) => {
                            // Update store
                            self.store.handle_event(event.clone()).await;
                            
                            // Generate UI event based on active menu (gated rendering)
                            self.handle_ui_update(&event, callback.clone()).await;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("UI Bridge lagged {} events", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
                
                // Receive bridge command
                cmd = self.command_rx.recv() => {
                    match cmd {
                        Some(BridgeCommand::SetMenu(menu)) => {
                            self.store.set_active_menu(menu).await;
                            // Trigger immediate update for the new menu
                            // TODO: Maybe republish state?
                            if menu == MenuType::Logs {
                                let all_logs = self.store.get_logs().await;
                                callback(UiEvent::LogsUpdated(all_logs));
                            }
                        }
                        Some(BridgeCommand::RequestLogs) => {
                            let all_logs = self.store.get_logs().await;
                            callback(UiEvent::LogsUpdated(all_logs));
                        }
                        None => break,
                    }
                }

                // Periodic flush for batched updates
                _ = tokio::time::sleep(Duration::from_millis(self.batch_interval_ms)) => {
                    self.flush_pending_updates(callback.clone()).await;
                }
            }
        }
        
        tracing::info!("UI Bridge stopped");
    }
    
    /// Handle UI update based on active menu
    async fn handle_ui_update<F>(&mut self, event: &DomainEvent, callback: Arc<F>)
    where
        F: Fn(UiEvent) + Send + Sync + 'static,
    {
        let active_menu = self.store.get_active_menu().await;
        
        match event {
            DomainEvent::TransactionUpdated { summary, .. } => {
                // Dashboard always gets counter updates
                if active_menu == MenuType::Dashboard {
                    let counters = self.store.get_monitoring_counters().await;
                    callback(UiEvent::RefreshMonitoring {
                        total_transactions: counters.total_transactions,
                        success_count: counters.success_count,
                        failed_count: counters.failed_count,
                        tps: counters.tps,
                    });
                }
                
                // Transaction list if on transaksi page
                if active_menu == MenuType::Transaksi {
                    let transactions = self.store.get_recent_transactions().await;
                    callback(UiEvent::TransactionsUpdated(transactions));
                }
            }
            DomainEvent::LogAppended { entry, .. } => {
                // Batch logs
                self.pending_logs.push(entry.clone());
                
                // Only send if on logs page
                if active_menu == MenuType::Logs && self.pending_logs.len() >= 10 {
                    self.flush_pending_updates(callback).await;
                }
            }
            DomainEvent::LogsCleared { .. } => {
                self.pending_logs.clear();
                if active_menu == MenuType::Logs {
                    let all_logs = self.store.get_logs().await;
                    callback(UiEvent::LogsUpdated(all_logs));
                }
            }
            DomainEvent::ConfigChanged { key, value, .. } => {
                if active_menu == MenuType::Settings {
                    callback(UiEvent::ConfigLoaded {
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
            }
            // Product events trigger UI refresh on Produk page
            DomainEvent::ProductCreated { .. } |
            DomainEvent::ProductUpdated { .. } |
            DomainEvent::ProductDeleted { .. } => {
                // Product updates are handled via ProductService
                // UI can subscribe to product changes when on Produk page
                if active_menu == MenuType::Produk {
                    // Products are refreshed from DB via ProductService
                    // This event signals that a refresh is needed
                    tracing::debug!("Product changed, UI should refresh");
                }
            }
            // Stok Voucher events trigger UI refresh on MasterData page
            DomainEvent::StokVoucherCreated { .. } |
            DomainEvent::StokVoucherUpdated { .. } |
            DomainEvent::StokVoucherDeleted { .. } |
            DomainEvent::StokStatusChanged { .. } => {
                if active_menu == MenuType::MasterData {
                    tracing::debug!("Stok voucher changed, UI should refresh");
                }
            }
        }
    }
    
    /// Flush pending batched updates
    async fn flush_pending_updates<F>(&mut self, callback: Arc<F>)
    where
        F: Fn(UiEvent) + Send + Sync + 'static,
    {
        if !self.pending_logs.is_empty() {
            // Clear pending logs
            self.pending_logs.clear();
            
            // Get all logs from store (which includes the new ones we just handled)
            let all_logs = self.store.get_logs().await;
            callback(UiEvent::LogsUpdated(all_logs));
        }
        self.last_flush = std::time::Instant::now();
    }
}

/// Helper function to send UI update via Slint's invoke_from_event_loop
/// 
/// Usage:
/// ```rust
/// let ui_handle = ui.as_weak();
/// send_slint_update(ui_handle, |ui| {
///     ui.set_transaction_count(100);
/// });
/// ```
pub fn send_slint_update<F>(callback: F)
where
    F: FnOnce() + Send + 'static,
{
    let _ = slint::invoke_from_event_loop(callback);
}
