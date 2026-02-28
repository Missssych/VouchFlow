//! DB Writer Actor - Single writer pattern for SQLite
//!
//! All database writes go through this actor to ensure serialization.
//! This is the ONLY component that should call db.with_writer().

use chrono::{Local, Utc};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{broadcast, mpsc};

use super::Database;
use crate::domain::{
    DbCommand, DomainError, DomainEvent, LogEntry, ProductSummary, ReservedVoucher,
    StokVoucherSummary, TransactionStatus, TransactionSummary,
};
use crate::utils::normalize_expired_date_optional;

/// DB Writer Actor
pub struct DbWriter {
    db: Database,
    command_rx: mpsc::Receiver<DbCommand>,
    event_tx: broadcast::Sender<DomainEvent>,
    seq: Arc<AtomicU64>,
}

impl DbWriter {
    /// Create new DB Writer
    pub fn new(
        db: Database,
        command_rx: mpsc::Receiver<DbCommand>,
        event_tx: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            db,
            command_rx,
            event_tx,
            seq: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Get next sequence number
    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    /// Run the writer actor loop
    pub async fn run(mut self) {
        tracing::info!("DB Writer started");

        while let Some(cmd) = self.command_rx.recv().await {
            if let Err(e) = self.handle_command(cmd).await {
                tracing::error!("DB Writer error: {}", e);
            }
        }

        tracing::info!("DB Writer stopped");
    }

    /// Handle a single command
    async fn handle_command(&self, cmd: DbCommand) -> Result<(), DomainError> {
        match cmd {
            DbCommand::InsertTransaction {
                tx_id,
                request_id,
                trace_id,
                provider,
                kode_produk,
                kategori,
                harga,
                produk,
                nomor,
            } => {
                self.insert_transaction(
                    &tx_id,
                    &request_id,
                    &trace_id,
                    &provider,
                    &kode_produk,
                    &kategori,
                    harga,
                    &produk,
                    &nomor,
                )
                .await?;
            }
            DbCommand::UpdateTransaction {
                tx_id,
                status,
                sn,
                result_code,
                result_payload,
            } => {
                self.update_transaction(
                    &tx_id,
                    status,
                    sn.as_deref(),
                    result_code.as_deref(),
                    result_payload.as_deref(),
                )
                .await?;
            }
            DbCommand::AppendLog {
                level,
                message,
                trace_id,
            } => {
                self.append_log(&level, &message, trace_id.as_deref())
                    .await?;
            }
            DbCommand::AppendTransactionLog {
                tx_id,
                request_id,
                trace_id,
                kategori,
                attempt,
                stage,
                level,
                status,
                message,
                payload,
                latency_ms,
            } => {
                self.append_transaction_log(
                    &tx_id,
                    &request_id,
                    trace_id.as_deref(),
                    &kategori,
                    attempt,
                    &stage,
                    &level,
                    status.as_deref(),
                    &message,
                    payload.as_deref(),
                    latency_ms,
                )
                .await?;
            }
            DbCommand::ClearLogs => {
                self.clear_logs().await?;
            }
            DbCommand::PurgeOldLogs { retention_days } => {
                self.purge_old_logs(retention_days).await?;
            }
            DbCommand::WalCheckpoint { truncate } => {
                self.wal_checkpoint(truncate).await?;
            }
            DbCommand::SaveConfig {
                key,
                value,
                category,
            } => {
                self.save_config(&key, &value, &category).await?;
            }
            // Product CRUD handlers
            DbCommand::CreateProduct {
                provider,
                nama_produk,
                kode_produk,
                kategori,
                harga,
                kode_addon,
            } => {
                self.create_product(
                    &provider,
                    &nama_produk,
                    &kode_produk,
                    &kategori,
                    harga,
                    kode_addon.as_deref(),
                )
                .await?;
            }
            DbCommand::UpdateProduct {
                id,
                provider,
                nama_produk,
                kode_produk,
                kategori,
                harga,
                kode_addon,
            } => {
                self.update_product(
                    id,
                    &provider,
                    &nama_produk,
                    &kode_produk,
                    &kategori,
                    harga,
                    kode_addon.as_deref(),
                )
                .await?;
            }
            DbCommand::DeleteProduct { id } => {
                self.delete_products(vec![id]).await?;
            }
            DbCommand::DeleteProducts { ids } => {
                self.delete_products(ids).await?;
            }
            // Stok Voucher CRUD handlers
            DbCommand::CreateStokVoucher {
                provider,
                kode_addon,
                barcode,
                serial_number,
                expired_date,
            } => {
                self.create_stok_voucher(
                    &provider,
                    &kode_addon,
                    &barcode,
                    &serial_number,
                    expired_date.as_deref(),
                )
                .await?;
            }
            DbCommand::UpdateStokVoucher {
                id,
                provider,
                kode_addon,
                barcode,
                serial_number,
                expired_date,
            } => {
                self.update_stok_voucher(
                    id,
                    &provider,
                    &kode_addon,
                    &barcode,
                    &serial_number,
                    expired_date.as_deref(),
                )
                .await?;
            }
            DbCommand::DeleteStokVoucher { id } => {
                self.delete_stok_vouchers(vec![id]).await?;
            }
            DbCommand::DeleteStokVouchers { ids } => {
                self.delete_stok_vouchers(ids).await?;
            }
            DbCommand::ChangeStokStatus { ids, new_status } => {
                self.change_stok_status(ids, &new_status).await?;
            }
            // Stock Operations (Single-Writer pattern)
            DbCommand::ReserveStokVoucher {
                kode_addon,
                response_tx,
            } => {
                let result = self.reserve_single_voucher(&kode_addon).await;
                let _ = response_tx.send(result);
            }
            DbCommand::ReserveStokVoucherBatch {
                kode_addon,
                qty,
                response_tx,
            } => {
                let result = self.reserve_batch_vouchers(&kode_addon, qty).await;
                let _ = response_tx.send(result);
            }
            DbCommand::ReleaseStokVoucher { voucher_id } => {
                self.release_voucher(voucher_id).await?;
            }
            DbCommand::MarkStokUsed { voucher_id } => {
                self.mark_voucher_used(voucher_id).await?;
            }
            // Admin transaction action handlers
            DbCommand::ManualSuccess {
                tx_id,
                result_code,
                result_payload,
            } => {
                self.manual_success(&tx_id, &result_code, result_payload.as_deref())
                    .await?;
            }
            DbCommand::RetryTransaction {
                tx_id,
                kategori,
                produk,
                nomor,
            } => {
                // Retry is handled by orchestrator, just log here
                tracing::info!(tx_id = %tx_id, kategori = %kategori, produk = %produk, nomor = %nomor, "Retry transaction command received");
            }
        }
        Ok(())
    }

    /// Insert new transaction
    async fn insert_transaction(
        &self,
        tx_id: &str,
        request_id: &str,
        trace_id: &str,
        provider: &str,
        kode_produk: &str,
        kategori: &str,
        harga: f64,
        produk: &str,
        nomor: &str,
    ) -> Result<(), DomainError> {
        let now = Utc::now().to_rfc3339();

        self.db.with_writer(|conn| {
            conn.execute(
                "INSERT INTO transactions (tx_id, request_id, trace_id, provider, kode_produk, kategori, harga, produk, nomor, status, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'PROCESSING', ?, ?)",
                rusqlite::params![tx_id, request_id, trace_id, provider, kode_produk, kategori, harga, produk, nomor, now, now],
            )?;
            Ok(())
        }).await?;

        // Publish event
        let summary = TransactionSummary {
            tx_id: tx_id.to_string(),
            request_id: request_id.to_string(),
            provider: provider.to_string(),
            kategori: kategori.to_string(),
            harga,
            produk: produk.to_string(),
            nomor: nomor.to_string(),
            sn: None,
            status: "PROCESSING".to_string(),
            result_code: None,
            created_at: now,
        };

        let _ = self.event_tx.send(DomainEvent::TransactionUpdated {
            seq: self.next_seq(),
            tx_id: tx_id.to_string(),
            request_id: request_id.to_string(),
            status: TransactionStatus::Processing,
            tx_type: kategori.to_string(),
            summary,
        });

        Ok(())
    }

    /// Update transaction status
    async fn update_transaction(
        &self,
        tx_id: &str,
        status: TransactionStatus,
        sn: Option<&str>,
        result_code: Option<&str>,
        result_payload: Option<&str>,
    ) -> Result<(), DomainError> {
        let now = Utc::now().to_rfc3339();
        let status_str = status.as_str();

        // Get transaction info for event
        let (request_id, provider, kategori, harga, produk, nomor, created_at) = self.db.with_writer(|conn| {
            conn.execute(
                "UPDATE transactions SET status = ?, sn = ?, result_code = ?, result_payload = ?, updated_at = ? WHERE tx_id = ?",
                rusqlite::params![status_str, sn, result_code, result_payload, now, tx_id],
            )?;
            
            let row: (String, String, String, f64, String, String, String) = conn.query_row(
                "SELECT request_id, provider, kategori, harga, produk, nomor, created_at FROM transactions WHERE tx_id = ?",
                [tx_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
            )?;
            
            Ok(row)
        }).await?;

        // Publish event
        let summary = TransactionSummary {
            tx_id: tx_id.to_string(),
            request_id: request_id.clone(),
            provider,
            kategori: kategori.clone(),
            harga,
            produk,
            nomor,
            sn: sn.map(|s| s.to_string()),
            status: status_str.to_string(),
            result_code: result_code.map(|s| s.to_string()),
            created_at,
        };

        let _ = self.event_tx.send(DomainEvent::TransactionUpdated {
            seq: self.next_seq(),
            tx_id: tx_id.to_string(),
            request_id,
            status,
            tx_type: kategori,
            summary,
        });

        Ok(())
    }

    /// Append log entry
    async fn append_log(
        &self,
        level: &str,
        message: &str,
        trace_id: Option<&str>,
    ) -> Result<(), DomainError> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let id = self
            .db
            .with_writer(|conn| {
                conn.execute(
                    "INSERT INTO logs (level, message, trace_id, created_at) VALUES (?, ?, ?, ?)",
                    rusqlite::params![level, message, trace_id, now_str],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await?;

        let entry = LogEntry {
            id,
            level: level.to_string(),
            message: message.to_string(),
            trace_id: trace_id.map(|s| s.to_string()),
            created_at: now,
        };

        let _ = self.event_tx.send(DomainEvent::LogAppended {
            seq: self.next_seq(),
            entry,
        });

        Ok(())
    }

    /// Append per-transaction flow log entry
    async fn append_transaction_log(
        &self,
        tx_id: &str,
        request_id: &str,
        trace_id: Option<&str>,
        kategori: &str,
        attempt: i32,
        stage: &str,
        level: &str,
        status: Option<&str>,
        message: &str,
        payload: Option<&str>,
        latency_ms: Option<i64>,
    ) -> Result<(), DomainError> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let now_ms = now.timestamp_millis();
        let safe_attempt = if attempt <= 0 { 1 } else { attempt };

        self.db.with_writer(|conn| {
            let next_seq: i32 = conn.query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM transaction_logs WHERE tx_id = ? AND attempt = ?",
                rusqlite::params![tx_id, safe_attempt],
                |row| row.get(0),
            )?;

            conn.execute(
                "INSERT INTO transaction_logs (
                    tx_id, request_id, trace_id, kategori, attempt, seq, stage, level, status,
                    message, payload, latency_ms, created_at_ms, created_at
                )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    tx_id,
                    request_id,
                    trace_id,
                    kategori,
                    safe_attempt,
                    next_seq,
                    stage,
                    level,
                    status,
                    message,
                    payload,
                    latency_ms,
                    now_ms,
                    now_str
                ],
            )?;
            Ok(())
        }).await
    }

    /// Clear all log entries
    async fn clear_logs(&self) -> Result<(), DomainError> {
        self.db
            .with_writer(|conn| {
                conn.execute("DELETE FROM logs", [])?;
                Ok(())
            })
            .await?;

        let _ = self.event_tx.send(DomainEvent::LogsCleared {
            seq: self.next_seq(),
        });

        Ok(())
    }

    /// Purge old log entries by retention policy
    async fn purge_old_logs(&self, retention_days: i64) -> Result<(), DomainError> {
        self.db
            .with_writer(|conn| {
                conn.execute(
                    "DELETE FROM logs WHERE created_at < datetime('now', ?)",
                    rusqlite::params![format!("-{} days", retention_days)],
                )?;
                conn.execute(
                    "DELETE FROM transaction_logs WHERE created_at < datetime('now', ?)",
                    rusqlite::params![format!("-{} days", retention_days)],
                )?;
                Ok(())
            })
            .await
    }

    /// Run WAL checkpoint.
    /// PASSIVE is safe for periodic background maintenance.
    /// TRUNCATE is used on shutdown to shrink WAL file when possible.
    async fn wal_checkpoint(&self, truncate: bool) -> Result<(), DomainError> {
        let mode = if truncate { "TRUNCATE" } else { "PASSIVE" };
        let sql = if truncate {
            "PRAGMA wal_checkpoint(TRUNCATE)"
        } else {
            "PRAGMA wal_checkpoint(PASSIVE)"
        };

        let (busy, log_frames, checkpointed_frames) = self
            .db
            .with_writer(|conn| {
                let stats = conn.query_row(sql, [], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?;
                Ok(stats)
            })
            .await?;

        if busy > 0 {
            tracing::warn!(
                mode = mode,
                busy,
                log_frames,
                checkpointed_frames,
                "WAL checkpoint incomplete due to active readers"
            );
        } else if log_frames > 0 {
            tracing::debug!(
                mode = mode,
                log_frames,
                checkpointed_frames,
                "WAL checkpoint completed"
            );
        }

        Ok(())
    }

    /// Save configuration
    async fn save_config(&self, key: &str, value: &str, category: &str) -> Result<(), DomainError> {
        let now = Utc::now().to_rfc3339();

        self.db.with_writer(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO configurations (key, value, category, updated_at) VALUES (?, ?, ?, ?)",
                rusqlite::params![key, value, category, now],
            )?;
            Ok(())
        }).await?;

        let _ = self.event_tx.send(DomainEvent::ConfigChanged {
            seq: self.next_seq(),
            key: key.to_string(),
            value: value.to_string(),
        });

        Ok(())
    }

    // ===== Product CRUD Methods =====

    /// Create new product
    async fn create_product(
        &self,
        provider: &str,
        nama_produk: &str,
        kode_produk: &str,
        kategori: &str,
        harga: f64,
        kode_addon: Option<&str>,
    ) -> Result<(), DomainError> {
        let now = Utc::now().to_rfc3339();

        let id = self.db.with_writer(|conn| {
            conn.execute(
                "INSERT INTO produk (provider, nama_produk, kode_produk, kategori, harga, kode_addon, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![provider, nama_produk, kode_produk, kategori, harga, kode_addon, now, now],
            )?;
            Ok(conn.last_insert_rowid())
        }).await?;

        let product = ProductSummary {
            id,
            provider: provider.to_string(),
            nama_produk: nama_produk.to_string(),
            kode_produk: kode_produk.to_string(),
            kategori: kategori.to_string(),
            harga: format!("{:.0}", harga),
            kode_addon: kode_addon.unwrap_or("").to_string(),
        };

        let _ = self.event_tx.send(DomainEvent::ProductCreated {
            seq: self.next_seq(),
            product,
        });

        tracing::info!(product_id = id, "Product created: {}", nama_produk);
        Ok(())
    }

    /// Update existing product
    async fn update_product(
        &self,
        id: i64,
        provider: &str,
        nama_produk: &str,
        kode_produk: &str,
        kategori: &str,
        harga: f64,
        kode_addon: Option<&str>,
    ) -> Result<(), DomainError> {
        let now = Utc::now().to_rfc3339();

        self.db.with_writer(|conn| {
            conn.execute(
                "UPDATE produk SET provider = ?, nama_produk = ?, kode_produk = ?, kategori = ?, harga = ?, kode_addon = ?, updated_at = ?
                 WHERE id = ?",
                rusqlite::params![provider, nama_produk, kode_produk, kategori, harga, kode_addon, now, id],
            )?;
            Ok(())
        }).await?;

        let product = ProductSummary {
            id,
            provider: provider.to_string(),
            nama_produk: nama_produk.to_string(),
            kode_produk: kode_produk.to_string(),
            kategori: kategori.to_string(),
            harga: format!("{:.0}", harga),
            kode_addon: kode_addon.unwrap_or("").to_string(),
        };

        let _ = self.event_tx.send(DomainEvent::ProductUpdated {
            seq: self.next_seq(),
            product,
        });

        tracing::info!(product_id = id, "Product updated: {}", nama_produk);
        Ok(())
    }

    /// Delete products by IDs
    async fn delete_products(&self, ids: Vec<i64>) -> Result<(), DomainError> {
        if ids.is_empty() {
            return Ok(());
        }

        self.db
            .with_writer(|conn| {
                let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
                let sql = format!(
                    "DELETE FROM produk WHERE id IN ({})",
                    placeholders.join(",")
                );

                let params: Vec<Box<dyn rusqlite::ToSql>> = ids
                    .iter()
                    .map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>)
                    .collect();

                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();

                conn.execute(&sql, param_refs.as_slice())?;
                Ok(())
            })
            .await?;

        let _ = self.event_tx.send(DomainEvent::ProductDeleted {
            seq: self.next_seq(),
            ids: ids.clone(),
        });

        tracing::info!("Deleted {} products", ids.len());
        Ok(())
    }

    // ===== Stok Voucher CRUD Methods =====

    /// Create new stok voucher
    async fn create_stok_voucher(
        &self,
        provider: &str,
        kode_addon: &str,
        barcode: &str,
        serial_number: &str,
        expired_date: Option<&str>,
    ) -> Result<(), DomainError> {
        let normalized_expired = normalize_expired_date_optional(expired_date.unwrap_or_default())
            .map_err(DomainError::ValidationError)?;
        let expired_for_db = normalized_expired.clone();
        let now = Local::now().to_rfc3339();

        let id = self.db.with_writer(|conn| {
            conn.execute(
                "INSERT INTO stok_voucher (provider, kode_addon, barcode, serial_number, expired_date, status, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, 'ACTIVE', ?, ?)",
                rusqlite::params![
                    provider,
                    kode_addon,
                    barcode,
                    serial_number,
                    expired_for_db.as_deref(),
                    now,
                    now
                ],
            )?;
            Ok(conn.last_insert_rowid())
        }).await?;

        let voucher = StokVoucherSummary {
            id,
            provider: provider.to_string(),
            kode_addon: kode_addon.to_string(),
            barcode: barcode.to_string(),
            serial_number: serial_number.to_string(),
            expired_date: normalized_expired.unwrap_or_default(),
            status: "ACTIVE".to_string(),
            created_at: now,
        };

        let _ = self.event_tx.send(DomainEvent::StokVoucherCreated {
            seq: self.next_seq(),
            voucher,
        });

        tracing::info!(stok_id = id, "Stok voucher created: {}", serial_number);
        Ok(())
    }

    /// Update existing stok voucher
    async fn update_stok_voucher(
        &self,
        id: i64,
        provider: &str,
        kode_addon: &str,
        barcode: &str,
        serial_number: &str,
        expired_date: Option<&str>,
    ) -> Result<(), DomainError> {
        let normalized_expired = normalize_expired_date_optional(expired_date.unwrap_or_default())
            .map_err(DomainError::ValidationError)?;
        let expired_for_db = normalized_expired.clone();
        let now = Local::now().to_rfc3339();

        let status = self.db.with_writer(|conn| {
            conn.execute(
                "UPDATE stok_voucher SET provider = ?, kode_addon = ?, barcode = ?, serial_number = ?, expired_date = ?, updated_at = ?
                 WHERE id = ?",
                rusqlite::params![
                    provider,
                    kode_addon,
                    barcode,
                    serial_number,
                    expired_for_db.as_deref(),
                    now,
                    id
                ],
            )?;
            
            let status: String = conn.query_row(
                "SELECT status FROM stok_voucher WHERE id = ?",
                [id],
                |row| row.get(0),
            )?;
            Ok(status)
        }).await?;

        let voucher = StokVoucherSummary {
            id,
            provider: provider.to_string(),
            kode_addon: kode_addon.to_string(),
            barcode: barcode.to_string(),
            serial_number: serial_number.to_string(),
            expired_date: normalized_expired.unwrap_or_default(),
            status,
            created_at: now,
        };

        let _ = self.event_tx.send(DomainEvent::StokVoucherUpdated {
            seq: self.next_seq(),
            voucher,
        });

        tracing::info!(stok_id = id, "Stok voucher updated: {}", serial_number);
        Ok(())
    }

    /// Delete stok vouchers by IDs
    async fn delete_stok_vouchers(&self, ids: Vec<i64>) -> Result<(), DomainError> {
        if ids.is_empty() {
            return Ok(());
        }

        self.db
            .with_writer(|conn| {
                let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
                let sql = format!(
                    "DELETE FROM stok_voucher WHERE id IN ({})",
                    placeholders.join(",")
                );

                let params: Vec<Box<dyn rusqlite::ToSql>> = ids
                    .iter()
                    .map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>)
                    .collect();

                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();

                conn.execute(&sql, param_refs.as_slice())?;
                Ok(())
            })
            .await?;

        let _ = self.event_tx.send(DomainEvent::StokVoucherDeleted {
            seq: self.next_seq(),
            ids: ids.clone(),
        });

        tracing::info!("Deleted {} stok vouchers", ids.len());
        Ok(())
    }

    /// Change status of stok vouchers
    async fn change_stok_status(&self, ids: Vec<i64>, new_status: &str) -> Result<(), DomainError> {
        if ids.is_empty() {
            return Ok(());
        }

        let now = Local::now().to_rfc3339();
        let used_at = if new_status == "USED" {
            Some(now.as_str())
        } else {
            None
        };

        self.db.with_writer(|conn| {
            let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
            let sql = format!(
                "UPDATE stok_voucher SET status = ?, used_at = ?, updated_at = ? WHERE id IN ({})",
                placeholders.join(",")
            );
            
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
                Box::new(new_status.to_string()),
                Box::new(used_at.map(|s| s.to_string())),
                Box::new(now.clone()),
            ];
            for id in &ids {
                params.push(Box::new(*id));
            }
            
            let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter()
                .map(|p| p.as_ref())
                .collect();
            
            conn.execute(&sql, param_refs.as_slice())?;
            Ok(())
        }).await?;

        let _ = self.event_tx.send(DomainEvent::StokStatusChanged {
            seq: self.next_seq(),
            ids: ids.clone(),
            new_status: new_status.to_string(),
        });

        tracing::info!(
            "Changed status of {} stok vouchers to {}",
            ids.len(),
            new_status
        );
        Ok(())
    }

    // ===== Stock Reserve/Release Operations (Single-Writer) =====

    /// Reserve a single voucher atomically (read + mark RESERVED in one operation)
    async fn reserve_single_voucher(
        &self,
        kode_addon: &str,
    ) -> Result<ReservedVoucher, DomainError> {
        let now = Local::now().to_rfc3339();
        let reserved = self.db.with_writer(|conn| {
            // FEFO: prefer nearest valid expired_date first, push empty/invalid dates to the end.
            let result = conn.query_row(
                "SELECT id, barcode, serial_number, expired_date
                 FROM (
                     SELECT
                         id,
                         barcode,
                         serial_number,
                         expired_date,
                         created_at,
                         CASE
                             WHEN expired_date LIKE '____-__-__' THEN date(expired_date)
                             WHEN expired_date LIKE '__-__-____' THEN date(substr(expired_date, 7, 4) || '-' || substr(expired_date, 4, 2) || '-' || substr(expired_date, 1, 2))
                             WHEN expired_date LIKE '____/__/__' THEN date(replace(expired_date, '/', '-'))
                             WHEN expired_date LIKE '__/__/____' THEN date(substr(expired_date, 7, 4) || '-' || substr(expired_date, 4, 2) || '-' || substr(expired_date, 1, 2))
                             ELSE NULL
                         END AS normalized_expired
                     FROM stok_voucher
                     WHERE kode_addon = ? AND status = 'ACTIVE'
                 )
                 ORDER BY
                     CASE WHEN normalized_expired IS NULL THEN 1 ELSE 0 END ASC,
                     normalized_expired ASC,
                     created_at ASC
                 LIMIT 1",
                [kode_addon],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                )),
            );
            
            match result {
                Ok((id, barcode, serial_number, expired_date)) => {
                    // Atomically mark as RESERVED
                    let updated = conn.execute(
                        "UPDATE stok_voucher SET status = 'RESERVED', updated_at = ? WHERE id = ? AND status = 'ACTIVE'",
                        rusqlite::params![now.clone(), id],
                    )?;
                    
                    if updated == 0 {
                        return Err(DomainError::InsufficientStock(kode_addon.to_string()));
                    }
                    
                    Ok(ReservedVoucher {
                        voucher_id: id,
                        barcode,
                        serial_number,
                        expired_date,
                    })
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    Err(DomainError::InsufficientStock(kode_addon.to_string()))
                }
                Err(e) => Err(DomainError::DatabaseError(e.to_string())),
            }
        }).await?;

        // Emit StokStatusChanged event so UI can track stock changes
        let _ = self.event_tx.send(DomainEvent::StokStatusChanged {
            seq: self.next_seq(),
            ids: vec![reserved.voucher_id],
            new_status: "RESERVED".to_string(),
        });

        Ok(reserved)
    }

    /// Reserve multiple vouchers atomically
    async fn reserve_batch_vouchers(
        &self,
        kode_addon: &str,
        qty: i32,
    ) -> Result<Vec<ReservedVoucher>, DomainError> {
        let now = Local::now().to_rfc3339();
        self.db.with_writer(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, barcode, serial_number, expired_date
                 FROM (
                     SELECT
                         id,
                         barcode,
                         serial_number,
                         expired_date,
                         created_at,
                         CASE
                             WHEN expired_date LIKE '____-__-__' THEN date(expired_date)
                             WHEN expired_date LIKE '__-__-____' THEN date(substr(expired_date, 7, 4) || '-' || substr(expired_date, 4, 2) || '-' || substr(expired_date, 1, 2))
                             WHEN expired_date LIKE '____/__/__' THEN date(replace(expired_date, '/', '-'))
                             WHEN expired_date LIKE '__/__/____' THEN date(substr(expired_date, 7, 4) || '-' || substr(expired_date, 4, 2) || '-' || substr(expired_date, 1, 2))
                             ELSE NULL
                         END AS normalized_expired
                     FROM stok_voucher
                     WHERE kode_addon = ? AND status = 'ACTIVE'
                 )
                 ORDER BY
                     CASE WHEN normalized_expired IS NULL THEN 1 ELSE 0 END ASC,
                     normalized_expired ASC,
                     created_at ASC
                 LIMIT ?"
            )?;
            
            let rows = stmt.query_map(rusqlite::params![kode_addon, qty], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;
            
            let items: Vec<(i64, String, String, Option<String>)> = rows.filter_map(|r| r.ok()).collect();
            
            if items.len() < qty as usize {
                return Err(DomainError::InsufficientStock(kode_addon.to_string()));
            }
            
            let mut reserved: Vec<ReservedVoucher> = Vec::with_capacity(items.len());
            for (id, barcode, serial_number, expired_date) in items {
                let updated = conn.execute(
                    "UPDATE stok_voucher SET status = 'RESERVED', updated_at = ? WHERE id = ? AND status = 'ACTIVE'",
                    rusqlite::params![now.clone(), id],
                )?;
                if updated == 0 {
                    // Race condition: another request reserved it. Rollback our reservations.
                    for v in &reserved {
                        let _ = conn.execute(
                            "UPDATE stok_voucher SET status = 'ACTIVE', updated_at = ? WHERE id = ? AND status = 'RESERVED'",
                            rusqlite::params![now.clone(), v.voucher_id],
                        );
                    }
                    return Err(DomainError::InsufficientStock(kode_addon.to_string()));
                }
                reserved.push(ReservedVoucher { voucher_id: id, barcode, serial_number, expired_date });
            }
            
            Ok(reserved)
        }).await
    }

    /// Release a reserved voucher back to ACTIVE
    async fn release_voucher(&self, voucher_id: i64) -> Result<(), DomainError> {
        let now = Local::now().to_rfc3339();
        self.db.with_writer(|conn| {
            conn.execute(
                "UPDATE stok_voucher SET status = 'ACTIVE', updated_at = ? WHERE id = ? AND status = 'RESERVED'",
                rusqlite::params![now, voucher_id],
            )?;
            Ok(())
        }).await?;

        tracing::debug!(voucher_id = voucher_id, "Voucher released back to ACTIVE");
        Ok(())
    }

    /// Mark a reserved voucher as USED
    async fn mark_voucher_used(&self, voucher_id: i64) -> Result<(), DomainError> {
        let now = Local::now().to_rfc3339();
        self.db
            .with_writer(|conn| {
                conn.execute(
                "UPDATE stok_voucher SET status = 'USED', used_at = ?, updated_at = ? WHERE id = ?",
                rusqlite::params![now.clone(), now, voucher_id],
            )?;
                Ok(())
            })
            .await?;

        let _ = self.event_tx.send(DomainEvent::StokStatusChanged {
            seq: self.next_seq(),
            ids: vec![voucher_id],
            new_status: "USED".to_string(),
        });

        tracing::debug!(voucher_id = voucher_id, "Voucher marked as USED");
        Ok(())
    }

    // ===== Admin Transaction Actions =====

    /// Manual success for pending transaction
    async fn manual_success(
        &self,
        tx_id: &str,
        result_code: &str,
        result_payload: Option<&str>,
    ) -> Result<(), DomainError> {
        let now = Utc::now().to_rfc3339();

        // Get transaction details for event
        let request_id: String = self
            .db
            .with_writer(|conn| {
                conn.query_row(
                    "SELECT request_id FROM transactions WHERE tx_id = ?",
                    [tx_id],
                    |row| row.get(0),
                )
                .map_err(|e| DomainError::DatabaseError(e.to_string()))
            })
            .await?;

        // Update transaction to SUCCESS
        self.db.with_writer(|conn| {
            conn.execute(
                "UPDATE transactions SET status = 'SUCCESS', result_code = ?, result_payload = ?, updated_at = ? WHERE tx_id = ?",
                rusqlite::params![result_code, result_payload, now, tx_id],
            )?;
            Ok(())
        }).await?;

        // Send domain event
        let _ = self.event_tx.send(DomainEvent::TransactionUpdated {
            seq: self.next_seq(),
            tx_id: tx_id.to_string(),
            request_id: request_id.clone(),
            status: TransactionStatus::Success,
            tx_type: "ADMIN".to_string(),
            summary: TransactionSummary {
                tx_id: tx_id.to_string(),
                request_id,
                provider: String::new(),
                kategori: "ADMIN".to_string(),
                harga: 0.0,
                produk: String::new(),
                nomor: String::new(),
                sn: None,
                status: "SUCCESS".to_string(),
                result_code: Some(result_code.to_string()),
                created_at: now,
            },
        });

        tracing::info!(tx_id = %tx_id, "Transaction manually marked as success");
        Ok(())
    }
}
