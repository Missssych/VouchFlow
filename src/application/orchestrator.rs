//! Transaction Orchestrator
//!
//! Main orchestrator that coordinates transaction processing.
//! Receives commands from Gateway, routes to appropriate service,
//! and manages the transaction lifecycle.

use crate::domain::{Command, DbCommand, TransactionResult, TransactionType, DomainError};
use crate::infrastructure::channels::{CommandReceiver, DbCommandSender};
use crate::infrastructure::provider::{ProviderClient, CircuitBreaker, CircuitBreakerConfig};
use crate::infrastructure::database::Database;
use crate::application::providers::ProviderRouter;
use crate::application::services;
use std::time::Instant;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Transaction Orchestrator
pub struct Orchestrator {
    command_rx: Mutex<CommandReceiver>,
    db_cmd_tx: DbCommandSender,
    db: Database,
    provider: ProviderClient,
    provider_router: ProviderRouter,
    circuit_breaker: CircuitBreaker,
}

impl Orchestrator {
    /// Create new orchestrator
    pub fn new(
        command_rx: CommandReceiver,
        db_cmd_tx: DbCommandSender,
        db: Database,
        provider_url: String,
    ) -> Self {
        let provider = ProviderClient::new(provider_url, 30);
        let provider_router = ProviderRouter::new();
        let circuit_breaker = CircuitBreaker::new(CircuitBreakerConfig::default());
        
        Self {
            command_rx: Mutex::new(command_rx),
            db_cmd_tx,
            db,
            provider,
            provider_router,
            circuit_breaker,
        }
    }
    
    /// Run the orchestrator loop
    pub async fn run(self) {
        tracing::info!("Orchestrator started");
        
        let orchestrator = Arc::new(self);

        loop {
            let cmd_opt = {
                let mut rx: tokio::sync::MutexGuard<'_, CommandReceiver> = orchestrator.command_rx.lock().await;
                rx.recv().await
            };

            match cmd_opt {
                Some(cmd) => {
                    let orchestrator_clone = orchestrator.clone();
                    
                    tokio::spawn(async move {
                        let result = orchestrator_clone.process_command(cmd).await;
                        if let Err(e) = result {
                            tracing::error!("Orchestrator error: {}", e);
                        }
                    });
                }
                None => break, // Channel closed
            }
        }
        
        tracing::info!("Orchestrator stopped");
    }
    
    /// Process a single command
    async fn process_command(&self, cmd: Command) -> Result<(), DomainError> {
        let tx_id = cmd
            .retry_tx_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let is_retry = cmd.retry_tx_id.is_some();
        let attempt = self.determine_attempt(&tx_id, is_retry).await?;
        
        tracing::info!(
            trace_id = %cmd.trace_id,
            request_id = %cmd.request_id,
            tx_type = ?cmd.tx_type,
            "Processing command"
        );
        
        if is_retry {
            // Retry from admin UI must re-run flow on the same transaction row.
            self.db_cmd_tx.send(crate::domain::DbCommand::UpdateTransaction {
                tx_id: tx_id.clone(),
                status: crate::domain::TransactionStatus::Processing,
                sn: None,
                result_code: None,
                result_payload: None,
            }).await.map_err(|e: tokio::sync::mpsc::error::SendError<DbCommand>| DomainError::ChannelError(e.to_string()))?;

            self.append_flow_log(
                &tx_id,
                &cmd.request_id,
                &cmd.trace_id,
                &cmd.kategori,
                attempt,
                "RETRY_REQUESTED",
                "INFO",
                Some("PROCESSING"),
                &format!("Retry flow started for attempt {}", attempt),
                None,
                None,
            ).await?;
        } else {
            // Check for duplicate request (dedupe from DB)
            if self.is_duplicate_request(&cmd.request_id).await? {
                tracing::info!(request_id = %cmd.request_id, "Duplicate request detected");
                
                // Get existing result and return
                if let Some(response_tx) = cmd.response_tx {
                    let result = self.get_existing_result(&cmd.request_id).await?;
                    let _ = response_tx.send(result);
                }
                return Ok(());
            }
            
            // Insert transaction as PROCESSING
            self.db_cmd_tx.send(crate::domain::DbCommand::InsertTransaction {
                tx_id: tx_id.clone(),
                request_id: cmd.request_id.clone(),
                trace_id: cmd.trace_id.clone(),
                provider: cmd.provider.clone(),
                kode_produk: cmd.kode_produk.clone(),
                kategori: cmd.kategori.clone(),
                harga: cmd.harga,
                produk: cmd.produk.clone(),
                nomor: cmd.nomor.clone(),
            }).await.map_err(|e: tokio::sync::mpsc::error::SendError<DbCommand>| DomainError::ChannelError(e.to_string()))?;

            self.append_flow_log(
                &tx_id,
                &cmd.request_id,
                &cmd.trace_id,
                &cmd.kategori,
                attempt,
                "TRANSACTION_CREATED",
                "INFO",
                Some("PROCESSING"),
                "Transaction inserted as PROCESSING",
                None,
                None,
            ).await?;
        }

        self.append_flow_log(
            &tx_id,
            &cmd.request_id,
            &cmd.trace_id,
            &cmd.kategori,
            attempt,
            "FLOW_START",
            "INFO",
            Some("PROCESSING"),
            "Transaction flow execution started",
            None,
            None,
        ).await?;
        
        // Execute based on transaction type
        let started_at = Instant::now();
        let result = match cmd.tx_type {
            TransactionType::Check => {
                services::execute_check(
                    &tx_id,
                    &cmd.request_id,
                    &cmd.trace_id,
                    &cmd.provider,  // Pass provider name for routing
                    &cmd.produk,
                    &cmd.nomor,
                    &cmd.kategori,
                    attempt,
                    &self.provider_router,
                    &self.circuit_breaker,
                    &self.db_cmd_tx,
                ).await
            }
            TransactionType::Redeem => {
                services::execute_redeem(
                    &tx_id,
                    &cmd.request_id,
                    &cmd.trace_id,
                    &cmd.provider,  // Provider name for routing
                    cmd.kode_addon.as_deref().unwrap_or(&cmd.produk),  // kode_addon for stok_voucher lookup
                    &cmd.nomor,    // msisdn (nomor tujuan)
                    &cmd.kategori,
                    attempt,
                    &self.provider_router,
                    &self.circuit_breaker,
                    &self.db_cmd_tx,
                ).await
            }
            TransactionType::Physical => {
                services::execute_physical(
                    &tx_id,
                    &cmd.request_id,
                    &cmd.trace_id,
                    cmd.kode_addon.as_deref().unwrap_or(&cmd.produk),
                    &cmd.nomor,
                    1, // Default qty
                    &cmd.kategori,
                    attempt,
                    &self.db_cmd_tx,
                ).await
            }
        };
        let flow_latency = started_at.elapsed().as_millis() as i64;

        self.append_flow_log(
            &tx_id,
            &cmd.request_id,
            &cmd.trace_id,
            &cmd.kategori,
            attempt,
            "FLOW_FINISHED",
            if result.success { "INFO" } else { "ERROR" },
            Some(result.status.as_str()),
            result.message.as_deref().unwrap_or("Flow completed"),
            None,
            Some(flow_latency),
        ).await?;
        
        // Log transaction result
        let _ = self.db_cmd_tx.send(DbCommand::AppendLog {
            level: if result.success { "INFO".to_string() } else { "ERROR".to_string() },
            message: format!(
                "[{}] {} - {} - {}",
                cmd.tx_type.as_str(),
                cmd.request_id,
                result.status,
                result.message.as_deref().unwrap_or("OK")
            ),
            trace_id: Some(cmd.trace_id.clone()),
        }).await;
        
        // Send response if synchronous
        if let Some(response_tx) = cmd.response_tx {
            let _ = response_tx.send(result);
        }
        
        Ok(())
    }
    
    /// Check if request already exists (for idempotency)
    async fn is_duplicate_request(&self, request_id: &str) -> Result<bool, DomainError> {
        self.db.with_reader(|conn: &rusqlite::Connection| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM transactions WHERE request_id = ?",
                [request_id],
                |row: &rusqlite::Row| row.get(0),
            ).unwrap_or(0);
            Ok(count > 0)
        }).await
    }
    
    /// Get existing transaction result (for duplicate requests)
    async fn get_existing_result(&self, request_id: &str) -> Result<TransactionResult, DomainError> {
        self.db.with_reader(|conn: &rusqlite::Connection| {
            let (tx_id, status, result_code, result_payload): (String, String, Option<String>, Option<String>) = 
                conn.query_row(
                    "SELECT tx_id, status, result_code, result_payload FROM transactions WHERE request_id = ?",
                    [request_id],
                    |row: &rusqlite::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )?;
            
            let success = status == "SUCCESS";
            
            Ok(TransactionResult {
                success,
                request_id: request_id.to_string(),
                tx_id,
                status,
                result_code,
                result_payload,
                message: if success { None } else { Some("Previous result".to_string()) },
            })
        }).await
    }

    async fn determine_attempt(&self, tx_id: &str, is_retry: bool) -> Result<i32, DomainError> {
        let max_attempt = self.db.with_reader(|conn: &rusqlite::Connection| {
            let val: i32 = conn.query_row(
                "SELECT COALESCE(MAX(attempt), 0) FROM transaction_logs WHERE tx_id = ?",
                [tx_id],
                |row: &rusqlite::Row| row.get(0),
            ).unwrap_or(0);
            Ok(val)
        }).await?;

        if is_retry {
            Ok(if max_attempt <= 0 { 2 } else { max_attempt + 1 })
        } else {
            Ok(1)
        }
    }

    async fn append_flow_log(
        &self,
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
    ) -> Result<(), DomainError> {
        self.db_cmd_tx.send(crate::domain::DbCommand::AppendTransactionLog {
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
        }).await.map_err(|e: tokio::sync::mpsc::error::SendError<DbCommand>| DomainError::ChannelError(e.to_string()))
    }
}

// NOTE: Orchestrator is intentionally NOT Clone because CommandReceiver cannot be cloned.
// The orchestrator owns the receiver end of the command bus exclusively.
