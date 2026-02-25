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
        tracing::info!("UI Bridge started with batch interval: {}ms", self.batch_interval_ms);
        
        let callback = Arc::new(ui_callback);
        
        // Interval for rendering UI debouncing
        let mut render_interval = tokio::time::interval(Duration::from_millis(self.batch_interval_ms));
        render_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // Keep track of which menu was last rendered to avoid duplicate renders
        let mut needs_render = false;

        loop {
            tokio::select! {
                // 1. Receive domain event (Updates Store ONLY, no UI render)
                result = self.event_rx.recv() => {
                    match result {
                        Ok(event) => {
                            self.store.handle_event(event).await;
                            // `handle_event` calls `store.notify.notify_one()` inside CentralStore
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("UI Bridge event channel lagged {} events. But it's OK, state will be resync'd via store.", n);
                            // We might have missed some events, but the store state might have been 
                            // updated by another consumer or next events. Flag render anyway.
                            needs_render = true;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
                
                // 2. Receive bridge command (Direct UI action / Menu Change)
                cmd = self.command_rx.recv() => {
                    match cmd {
                        Some(BridgeCommand::SetMenu(menu)) => {
                            self.store.set_active_menu(menu).await;
                            needs_render = true; // Force render on menu change
                        }
                        Some(BridgeCommand::RequestLogs) => {
                            let all_logs = self.store.get_logs().await;
                            callback(UiEvent::LogsUpdated(all_logs));
                        }
                        None => break,
                    }
                }

                // 3. Listen to Central Store Notification
                _ = self.store.notify.notified() => {
                    needs_render = true;
                }

                // 4. Periodic Debouncer for UI Rendering
                _ = render_interval.tick() => {
                    if needs_render {
                        self.flush_ui_state(callback.clone()).await;
                        needs_render = false;
                    }
                }
            }
        }
        
        tracing::info!("UI Bridge stopped");
    }
    
    /// Extract entire state for the active menu and send it to Slint at once
    async fn flush_ui_state<F>(&self, callback: Arc<F>)
    where
        F: Fn(UiEvent) + Send + Sync + 'static,
    {
        let active_menu = self.store.get_active_menu().await;
        
        match active_menu {
            MenuType::Dashboard => {
                let counters = self.store.get_monitoring_counters().await;
                callback(UiEvent::RefreshMonitoring {
                    total_transactions: counters.total_transactions,
                    success_count: counters.success_count,
                    failed_count: counters.failed_count,
                    tps: counters.tps,
                });
                // Also send table data so dashboard auto-refreshes on new transactions
                let transactions = self.store.get_recent_transactions().await;
                callback(UiEvent::TransactionsUpdated(transactions));
            }
            MenuType::Logs => {
                let all_logs = self.store.get_logs().await;
                callback(UiEvent::LogsUpdated(all_logs));
            }
            MenuType::Settings => {
                // Settings config can be loaded dynamically if needed
            }
            MenuType::Produk | MenuType::MasterData | MenuType::Utility | MenuType::Transaksi => {
                // UI reads straight from DB when on these menus (via async commands)
                // Nothing to push from CentralStore for now.
            }
        }
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
