//! Database migrations

use rusqlite::Connection;
use std::collections::HashMap;
use crate::domain::DomainError;
use super::schema;

/// Run all database migrations
pub fn run_all(conn: &Connection) -> Result<(), DomainError> {
    // Create migrations table first
    conn.execute(
        "CREATE TABLE IF NOT EXISTS migrations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        [],
    )?;
    
    // Run schema migrations
    for (i, sql) in schema::ALL_SCHEMAS.iter().enumerate() {
        let migration_name = format!("schema_v{}", i + 1);
        
        // Check if already applied
        let applied = is_migration_applied(conn, &migration_name)?;
        
        if !applied {
            execute_sql_batch(conn, sql)?;
            mark_migration_applied(conn, &migration_name)?;
            
            tracing::info!("Applied migration: {}", migration_name);
        }
    }

    // Upgrade transaction flow log schema for better retry/observability support.
    // Runs once and migrates legacy table if needed.
    let tx_log_migration = "transaction_logs_v2";
    if !is_migration_applied(conn, tx_log_migration)? {
        migrate_transaction_logs_v2(conn)?;
        mark_migration_applied(conn, tx_log_migration)?;
        tracing::info!("Applied migration: {}", tx_log_migration);
    }

    // Ensure performant indexes for stock date-range queries.
    let stok_index_migration = "stok_voucher_indexes_v2";
    if !is_migration_applied(conn, stok_index_migration)? {
        migrate_stok_voucher_indexes_v2(conn)?;
        mark_migration_applied(conn, stok_index_migration)?;
        tracing::info!("Applied migration: {}", stok_index_migration);
    }
    
    // Insert default configurations if not exist
    insert_default_configs(conn)?;
    
    Ok(())
}

/// Insert default configuration values
fn insert_default_configs(conn: &Connection) -> Result<(), DomainError> {
    let defaults = [
        ("server_host", "127.0.0.1", "API", "API server host"),
        ("server_port", "8080", "API", "API server port"),
        ("terminal_host", "127.0.0.1", "API", "Terminal/provider host"),
        ("terminal_port", "8081", "API", "Terminal/provider port"),
        ("db_batch_size", "10", "DATABASE", "DB writer batch size"),
        ("db_batch_timeout_ms", "100", "DATABASE", "DB writer batch timeout"),
        ("ui_refresh_interval_ms", "250", "UI", "UI refresh interval"),
        ("log_buffer_size", "1000", "UI", "Log ring buffer size"),
    ];
    
    for (key, value, category, desc) in defaults {
        conn.execute(
            "INSERT OR IGNORE INTO configurations (key, value, category, description) 
             VALUES (?, ?, ?, ?)",
            [key, value, category, desc],
        )?;
    }
    
    Ok(())
}

fn is_migration_applied(conn: &Connection, migration_name: &str) -> Result<bool, DomainError> {
    let applied: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM migrations WHERE name = ?",
        [migration_name],
        |row| row.get(0),
    ).unwrap_or(false);
    Ok(applied)
}

fn mark_migration_applied(conn: &Connection, migration_name: &str) -> Result<(), DomainError> {
    conn.execute(
        "INSERT INTO migrations (name) VALUES (?)",
        [migration_name],
    )?;
    Ok(())
}

fn execute_sql_batch(conn: &Connection, sql: &str) -> Result<(), DomainError> {
    for statement in sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            conn.execute(trimmed, [])?;
        }
    }
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, DomainError> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?",
        [table],
        |row| row.get(0),
    )?;
    Ok(exists)
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, DomainError> {
    let pragma = format!("PRAGMA table_info({})", table);
    let mut stmt = conn.prepare(&pragma)?;
    let cols: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(cols.iter().any(|c| c == column))
}

fn migrate_transaction_logs_v2(conn: &Connection) -> Result<(), DomainError> {
    if !table_exists(conn, "transaction_logs")? {
        execute_sql_batch(conn, schema::CREATE_TRANSACTION_LOGS)?;
        return Ok(());
    }

    let already_v2 = table_has_column(conn, "transaction_logs", "attempt")?
        && table_has_column(conn, "transaction_logs", "seq")?
        && table_has_column(conn, "transaction_logs", "stage")?
        && table_has_column(conn, "transaction_logs", "created_at_ms")?;
    if already_v2 {
        // Re-apply index DDL to ensure all required indexes exist.
        execute_sql_batch(
            conn,
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_tx_logs_unique_seq ON transaction_logs(tx_id, attempt, seq);
             CREATE INDEX IF NOT EXISTS idx_tx_logs_tx_attempt_seq ON transaction_logs(tx_id, attempt, seq, id);
             CREATE INDEX IF NOT EXISTS idx_tx_logs_tx_created_ms ON transaction_logs(tx_id, created_at_ms, id);
             CREATE INDEX IF NOT EXISTS idx_tx_logs_request_id ON transaction_logs(request_id);
             CREATE INDEX IF NOT EXISTS idx_tx_logs_trace_id ON transaction_logs(trace_id);",
        )?;
        return Ok(());
    }

    conn.execute("ALTER TABLE transaction_logs RENAME TO transaction_logs_legacy", [])?;
    execute_sql_batch(conn, schema::CREATE_TRANSACTION_LOGS)?;

    let legacy_rows: Vec<(String, String, String, String, String, String, String, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT
                l.tx_id,
                COALESCE(t.request_id, ''),
                COALESCE(t.trace_id, ''),
                COALESCE(t.kategori, ''),
                COALESCE(l.event_type, 'FLOW'),
                COALESCE(l.message, ''),
                COALESCE(l.payload, ''),
                COALESCE(CAST(strftime('%s', l.timestamp) AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000)
             FROM transaction_logs_legacy l
             LEFT JOIN transactions t ON t.tx_id = l.tx_id
             ORDER BY l.tx_id ASC, l.timestamp ASC, l.id ASC",
        )?;

        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let mut seq_by_tx: HashMap<String, i32> = HashMap::new();
    for (tx_id, request_id_raw, trace_id_raw, kategori_raw, stage, message, payload_raw, created_at_ms) in legacy_rows {
        let seq = {
            let entry = seq_by_tx.entry(tx_id.clone()).or_insert(0);
            *entry += 1;
            *entry
        };

        let request_id = if request_id_raw.is_empty() {
            tx_id.clone()
        } else {
            request_id_raw
        };
        let kategori = if kategori_raw.is_empty() {
            "UNKNOWN".to_string()
        } else {
            kategori_raw
        };
        let trace_id = if trace_id_raw.is_empty() {
            None
        } else {
            Some(trace_id_raw)
        };
        let payload = if payload_raw.is_empty() {
            None
        } else {
            Some(payload_raw)
        };

        conn.execute(
            "INSERT INTO transaction_logs (
                tx_id, request_id, trace_id, kategori, attempt, seq, stage, level, status,
                latency_ms, message, payload, created_at_ms, created_at
             ) VALUES (?, ?, ?, ?, 1, ?, ?, 'INFO', NULL, NULL, ?, ?, ?, datetime(? / 1000, 'unixepoch'))",
            rusqlite::params![
                tx_id,
                request_id,
                trace_id,
                kategori,
                seq,
                stage,
                message,
                payload,
                created_at_ms,
                created_at_ms
            ],
        )?;
    }

    conn.execute("DROP TABLE IF EXISTS transaction_logs_legacy", [])?;
    Ok(())
}

fn migrate_stok_voucher_indexes_v2(conn: &Connection) -> Result<(), DomainError> {
    execute_sql_batch(
        conn,
        "CREATE INDEX IF NOT EXISTS idx_stok_used_at ON stok_voucher(used_at);
         CREATE INDEX IF NOT EXISTS idx_stok_status_created_at ON stok_voucher(status, created_at DESC);
         CREATE INDEX IF NOT EXISTS idx_stok_status_used_at ON stok_voucher(status, used_at DESC);",
    )
}
