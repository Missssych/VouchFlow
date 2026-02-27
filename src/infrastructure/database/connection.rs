//! SQLite connection management with WAL mode and optimizations
//!
//! Provides separate writer and reader connections for the single-writer pattern.
//! Writer: used exclusively by DbWriter actor for all writes.
//! Reader: used by services, gateway, and orchestrator for read-only queries.

use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::domain::DomainError;

// SQLite defaults to 1000 pages, keep explicit for predictable behavior.
const WAL_AUTOCHECKPOINT_PAGES: i64 = 1_000;
// Keep WAL from retaining excessive size after successful checkpoints.
const WAL_JOURNAL_SIZE_LIMIT_BYTES: i64 = 64 * 1024 * 1024; // 64 MB

/// Database connection wrapper with separate reader/writer connections
pub struct Database {
    /// Writer connection (single-writer pattern — only DbWriter should use this)
    writer: Arc<Mutex<Connection>>,
    /// Reader connection (read-only — for services, gateway, orchestrator)
    reader: Arc<Mutex<Connection>>,
}

impl Database {
    /// Create new database with separate writer and reader connections
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, DomainError> {
        // Writer connection: read-write with full mutex
        let writer_conn = Connection::open_with_flags(
            path.as_ref(),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
        )?;
        Self::apply_pragmas(&writer_conn)?;

        // Reader connection: read-only for concurrent reads
        let reader_conn = Connection::open_with_flags(
            path.as_ref(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
        )?;
        Self::apply_reader_pragmas(&reader_conn)?;

        Ok(Self {
            writer: Arc::new(Mutex::new(writer_conn)),
            reader: Arc::new(Mutex::new(reader_conn)),
        })
    }

    /// Apply SQLite PRAGMA optimizations for writer
    fn apply_pragmas(conn: &Connection) -> Result<(), DomainError> {
        // WAL mode for better concurrent reads
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Trigger automatic checkpoint after WAL reaches threshold pages
        conn.pragma_update(None, "wal_autocheckpoint", WAL_AUTOCHECKPOINT_PAGES)?;
        // Cap WAL retained size after checkpoint
        conn.pragma_update(None, "journal_size_limit", WAL_JOURNAL_SIZE_LIMIT_BYTES)?;
        // Busy timeout to reduce SQLITE_BUSY errors
        conn.pragma_update(None, "busy_timeout", 5000)?;
        // NORMAL sync mode for balance of safety and speed
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Enable foreign keys
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Cache size (negative = KB)
        conn.pragma_update(None, "cache_size", -8000)?; // 8MB cache
        // Temp store in memory
        conn.pragma_update(None, "temp_store", "MEMORY")?;

        Ok(())
    }

    /// Apply SQLite PRAGMA optimizations for reader (read-only specific)
    fn apply_reader_pragmas(conn: &Connection) -> Result<(), DomainError> {
        // WAL mode must be set on reader too for WAL reads
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Busy timeout
        conn.pragma_update(None, "busy_timeout", 5000)?;
        // Reader can use NORMAL sync
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Cache for reader
        conn.pragma_update(None, "cache_size", -4000)?; // 4MB cache for reader
        // Temp store in memory
        conn.pragma_update(None, "temp_store", "MEMORY")?;

        Ok(())
    }

    /// Get access to writer connection (only for DbWriter actor)
    pub fn writer(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.writer)
    }

    /// Execute a function with the writer connection
    ///
    /// WARNING: This should ONLY be used by DbWriter actor and startup code.
    /// For read operations, use `with_reader()` instead.
    pub async fn with_writer<F, T>(&self, f: F) -> Result<T, DomainError>
    where
        F: FnOnce(&Connection) -> Result<T, DomainError>,
    {
        let conn = self.writer.lock().await;
        f(&conn)
    }

    /// Execute a read-only function with the reader connection
    ///
    /// Use this for all read operations from services, gateway, orchestrator, etc.
    /// This does NOT block the writer — reads and writes can happen concurrently.
    pub async fn with_reader<F, T>(&self, f: F) -> Result<T, DomainError>
    where
        F: FnOnce(&Connection) -> Result<T, DomainError>,
    {
        let conn = self.reader.lock().await;
        f(&conn)
    }

    /// Run database migrations (uses writer)
    pub async fn run_migrations(&self) -> Result<(), DomainError> {
        use super::migrations;
        let conn = self.writer.lock().await;
        migrations::run_all(&conn)
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            writer: Arc::clone(&self.writer),
            reader: Arc::clone(&self.reader),
        }
    }
}
