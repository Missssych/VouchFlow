//! Stok Voucher Service
//! 
//! Read operations for stok voucher (Master Data stock management)

use crate::domain::{StokVoucherSummary, StokAddonSummary, DomainError};
use crate::infrastructure::database::Database;
use rusqlite::Connection;

fn next_day_ymd(date: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .ok()
        .and_then(|d| d.succ_opt())
        .map(|d| d.format("%Y-%m-%d").to_string())
}

/// Get active stock vouchers (status = ACTIVE)
pub async fn get_active_stocks(
    db: &Database,
    provider_filter: Option<&str>,
    search: Option<&str>,
) -> Result<Vec<StokVoucherSummary>, DomainError> {
    db.with_reader(|conn: &Connection| {
        let mut conditions = vec!["status = 'ACTIVE'".to_string()];
        let mut params: Vec<String> = Vec::new();
        
        if let Some(provider) = provider_filter.filter(|p| *p != "Semua") {
            conditions.push("provider = ?".to_string());
            params.push(provider.to_string());
        }
        
        if let Some(q) = search.filter(|s| !s.is_empty()) {
            conditions.push("(barcode LIKE ? OR serial_number LIKE ? OR kode_addon LIKE ?)".to_string());
            let pattern = format!("%{}%", q);
            params.push(pattern.clone());
            params.push(pattern.clone());
            params.push(pattern);
        }
        
        let sql = format!(
            "SELECT id, provider, kode_addon, barcode, serial_number, 
                    COALESCE(expired_date, '') as expired_date, status, created_at
             FROM stok_voucher 
             WHERE {}
             ORDER BY created_at DESC",
            conditions.join(" AND ")
        );
        
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter()
            .map(|p| p as &dyn rusqlite::ToSql)
            .collect();
        
        let rows = stmt.query_map(param_refs.as_slice(), map_row)?;
        let result: Vec<StokVoucherSummary> = rows.filter_map(|r| r.ok()).collect();
        
        Ok(result)
    }).await
}

/// Get used stock vouchers (status = USED) with date range filter
pub async fn get_used_stocks(
    db: &Database,
    provider_filter: Option<&str>,
    date_from: Option<&str>,
    date_to: Option<&str>,
    search: Option<&str>,
) -> Result<Vec<StokVoucherSummary>, DomainError> {
    db.with_reader(|conn: &Connection| {
        let mut conditions = vec!["status = 'USED'".to_string()];
        let mut params: Vec<String> = Vec::new();
        
        if let Some(provider) = provider_filter.filter(|p| *p != "Semua") {
            conditions.push("provider = ?".to_string());
            params.push(provider.to_string());
        }
        
        if let Some(from) = date_from.filter(|s| !s.is_empty()) {
            conditions.push("used_at >= ?".to_string());
            params.push(from.to_string());
        }
        
        if let Some(to) = date_to.filter(|s| !s.is_empty()) {
            if let Some(next_day) = next_day_ymd(to) {
                conditions.push("used_at < ?".to_string());
                params.push(next_day);
            } else {
                // Fallback for invalid date input.
                conditions.push("used_at <= ?".to_string());
                params.push(to.to_string());
            }
        }
        
        if let Some(q) = search.filter(|s| !s.is_empty()) {
            conditions.push("(barcode LIKE ? OR serial_number LIKE ? OR kode_addon LIKE ?)".to_string());
            let pattern = format!("%{}%", q);
            params.push(pattern.clone());
            params.push(pattern.clone());
            params.push(pattern);
        }
        
        let sql = format!(
            "SELECT id, provider, kode_addon, barcode, serial_number, 
                    COALESCE(expired_date, '') as expired_date, status, created_at
             FROM stok_voucher 
             WHERE {}
             ORDER BY used_at DESC",
            conditions.join(" AND ")
        );
        
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter()
            .map(|p| p as &dyn rusqlite::ToSql)
            .collect();
        
        let rows = stmt.query_map(param_refs.as_slice(), map_row)?;
        let result: Vec<StokVoucherSummary> = rows.filter_map(|r| r.ok()).collect();
        
        Ok(result)
    }).await
}

/// Get stock by ID
pub async fn get_stock_by_id(
    db: &Database,
    id: i64,
) -> Result<Option<StokVoucherSummary>, DomainError> {
    db.with_reader(|conn: &Connection| {
        let result = conn.query_row(
            "SELECT id, provider, kode_addon, barcode, serial_number, 
                    COALESCE(expired_date, '') as expired_date, status, created_at
             FROM stok_voucher WHERE id = ?",
            [id],
            map_row,
        );
        
        match result {
            Ok(voucher) => Ok(Some(voucher)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DomainError::DatabaseError(e.to_string())),
        }
    }).await
}

/// Get stock addon summary (count per kode_addon)
pub async fn get_stock_addon_summary(
    db: &Database,
) -> Result<Vec<StokAddonSummary>, DomainError> {
    db.with_reader(|conn: &Connection| {
        let mut stmt = conn.prepare(
            "SELECT kode_addon, COUNT(*) as count
             FROM stok_voucher 
             WHERE status = 'ACTIVE'
             GROUP BY kode_addon
             ORDER BY count DESC"
        )?;
        
        let rows = stmt.query_map([], |row: &rusqlite::Row| {
            Ok(StokAddonSummary {
                kode_addon: row.get(0)?,
                count: row.get(1)?,
            })
        })?;
        
        let result: Vec<StokAddonSummary> = rows.filter_map(|r| r.ok()).collect();
        Ok(result)
    }).await
}

/// Helper function to map row to StokVoucherSummary
fn map_row(row: &rusqlite::Row) -> rusqlite::Result<StokVoucherSummary> {
    Ok(StokVoucherSummary {
        id: row.get(0)?,
        provider: row.get(1)?,
        kode_addon: row.get(2)?,
        barcode: row.get(3)?,
        serial_number: row.get(4)?,
        expired_date: row.get(5)?,
        status: row.get(6)?,
        created_at: row.get(7)?,
    })
}

