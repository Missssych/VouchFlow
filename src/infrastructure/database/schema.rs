//! Database schema definitions

/// SQL for creating transactions table
pub const CREATE_TRANSACTIONS: &str = r#"
CREATE TABLE IF NOT EXISTS transactions (
    tx_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    trace_id TEXT NOT NULL,
    
    -- Product Info (from produk table)
    provider TEXT NOT NULL,
    kode_produk TEXT NOT NULL,
    kategori TEXT NOT NULL,
    harga REAL NOT NULL DEFAULT 0,
    
    -- Transaction Data
    produk TEXT NOT NULL,
    nomor TEXT NOT NULL,
    sn TEXT,
    
    -- Status & Result
    status TEXT NOT NULL DEFAULT 'PENDING',
    result_code TEXT,
    result_payload TEXT,
    
    -- Timestamps
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_transactions_request_id ON transactions(request_id);
CREATE INDEX IF NOT EXISTS idx_transactions_status ON transactions(status);
CREATE INDEX IF NOT EXISTS idx_transactions_provider ON transactions(provider);
CREATE INDEX IF NOT EXISTS idx_transactions_kategori ON transactions(kategori);
CREATE INDEX IF NOT EXISTS idx_transactions_created_at ON transactions(created_at);
"#;

/// SQL for creating produk (products) table
pub const CREATE_PRODUK: &str = r#"
CREATE TABLE IF NOT EXISTS produk (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    nama_produk TEXT NOT NULL,
    kode_produk TEXT NOT NULL UNIQUE,
    kategori TEXT NOT NULL DEFAULT 'RDM',
    harga REAL NOT NULL DEFAULT 0,
    kode_addon TEXT,
    aktif INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_produk_provider ON produk(provider);
CREATE INDEX IF NOT EXISTS idx_produk_nama ON produk(nama_produk);
CREATE INDEX IF NOT EXISTS idx_produk_kode ON produk(kode_produk);
CREATE INDEX IF NOT EXISTS idx_produk_kategori ON produk(kategori);
"#;

/// SQL for creating configurations table
pub const CREATE_CONFIGURATIONS: &str = r#"
CREATE TABLE IF NOT EXISTS configurations (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'GENERAL',
    description TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_config_category ON configurations(category);
"#;

/// SQL for creating logs table
pub const CREATE_LOGS: &str = r#"
CREATE TABLE IF NOT EXISTS logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    trace_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_logs_level ON logs(level);
CREATE INDEX IF NOT EXISTS idx_logs_trace_id ON logs(trace_id);
CREATE INDEX IF NOT EXISTS idx_logs_created_at ON logs(created_at);
"#;

/// SQL for creating stok_voucher table (Master Data stock)
pub const CREATE_STOK_VOUCHER: &str = r#"
CREATE TABLE IF NOT EXISTS stok_voucher (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    kode_addon TEXT NOT NULL,
    barcode TEXT NOT NULL,
    serial_number TEXT NOT NULL UNIQUE,
    expired_date TEXT,
    status TEXT NOT NULL DEFAULT 'ACTIVE',
    used_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_stok_provider ON stok_voucher(provider);
CREATE INDEX IF NOT EXISTS idx_stok_status ON stok_voucher(status);
CREATE INDEX IF NOT EXISTS idx_stok_serial ON stok_voucher(serial_number);
CREATE INDEX IF NOT EXISTS idx_stok_kode_addon ON stok_voucher(kode_addon);
CREATE INDEX IF NOT EXISTS idx_stok_created_at ON stok_voucher(created_at);
CREATE INDEX IF NOT EXISTS idx_stok_used_at ON stok_voucher(used_at);
CREATE INDEX IF NOT EXISTS idx_stok_status_created_at ON stok_voucher(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_stok_status_used_at ON stok_voucher(status, used_at DESC);
"#;

/// SQL for creating transaction_logs table (flow logs per transaction)
pub const CREATE_TRANSACTION_LOGS: &str = r#"
CREATE TABLE IF NOT EXISTS transaction_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tx_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    trace_id TEXT,
    kategori TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 1,
    seq INTEGER NOT NULL,
    stage TEXT NOT NULL,
    level TEXT NOT NULL DEFAULT 'INFO',
    status TEXT,
    latency_ms INTEGER,
    message TEXT NOT NULL,
    payload TEXT,
    created_at_ms INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (tx_id) REFERENCES transactions(tx_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_tx_logs_unique_seq ON transaction_logs(tx_id, attempt, seq);
CREATE INDEX IF NOT EXISTS idx_tx_logs_tx_attempt_seq ON transaction_logs(tx_id, attempt, seq, id);
CREATE INDEX IF NOT EXISTS idx_tx_logs_tx_created_ms ON transaction_logs(tx_id, created_at_ms, id);
CREATE INDEX IF NOT EXISTS idx_tx_logs_request_id ON transaction_logs(request_id);
CREATE INDEX IF NOT EXISTS idx_tx_logs_trace_id ON transaction_logs(trace_id);
"#;

/// All schema creation statements in order
pub const ALL_SCHEMAS: &[&str] = &[
    CREATE_PRODUK,
    CREATE_TRANSACTIONS,
    CREATE_CONFIGURATIONS,
    CREATE_LOGS,
    CREATE_STOK_VOUCHER,
    CREATE_TRANSACTION_LOGS,
];

