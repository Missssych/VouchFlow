//! Product Service
//!
//! Read operations and business logic for products

use crate::domain::{DomainError, Product, ProductSummary};
use crate::infrastructure::database::Database;

/// Product read operations
pub struct ProductService {
    db: Database,
}

impl ProductService {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Get all products as summaries for UI
    pub async fn get_all_products(&self) -> Result<Vec<ProductSummary>, DomainError> {
        self.db
            .with_reader(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, provider, nama_produk, kode_produk, kategori, harga, kode_addon
                 FROM produk WHERE aktif = 1 ORDER BY provider, nama_produk",
                )?;

                let products: Vec<ProductSummary> = stmt
                    .query_map([], |row| {
                        Ok(ProductSummary {
                            id: row.get(0)?,
                            provider: row.get(1)?,
                            nama_produk: row.get(2)?,
                            kode_produk: row.get(3)?,
                            kategori: row.get(4)?,
                            harga: format!("{:.0}", row.get::<_, f64>(5)?),
                            kode_addon: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                        })
                    })?
                    .filter_map(|r| r.ok())
                    .collect();

                Ok(products)
            })
            .await
    }

    /// Search products by name, kode_produk, provider, and/or kategori
    pub async fn search_products(
        &self,
        query: &str,
        provider_filter: Option<&str>,
        kategori_filter: Option<&str>,
    ) -> Result<Vec<ProductSummary>, DomainError> {
        let search = format!("%{}%", query);

        self.db
            .with_reader(|conn| {
                // Build dynamic SQL based on filters
                let has_provider = provider_filter
                    .map(|p| !p.is_empty() && p != "Semua")
                    .unwrap_or(false);
                let has_kategori = kategori_filter
                    .map(|k| !k.is_empty() && k != "Semua")
                    .unwrap_or(false);

                let sql = match (has_provider, has_kategori) {
                    (true, true) => {
                        "SELECT id, provider, nama_produk, kode_produk, kategori, harga, kode_addon
                     FROM produk WHERE aktif = 1 AND provider = ?1 AND kategori = ?2 
                     AND (nama_produk LIKE ?3 OR kode_produk LIKE ?3)
                     ORDER BY provider, nama_produk"
                    }
                    (true, false) => {
                        "SELECT id, provider, nama_produk, kode_produk, kategori, harga, kode_addon
                     FROM produk WHERE aktif = 1 AND provider = ?1 
                     AND (nama_produk LIKE ?2 OR kode_produk LIKE ?2)
                     ORDER BY provider, nama_produk"
                    }
                    (false, true) => {
                        "SELECT id, provider, nama_produk, kode_produk, kategori, harga, kode_addon
                     FROM produk WHERE aktif = 1 AND kategori = ?1 
                     AND (nama_produk LIKE ?2 OR kode_produk LIKE ?2)
                     ORDER BY provider, nama_produk"
                    }
                    (false, false) => {
                        "SELECT id, provider, nama_produk, kode_produk, kategori, harga, kode_addon
                     FROM produk WHERE aktif = 1 AND (nama_produk LIKE ?1 OR kode_produk LIKE ?1)
                     ORDER BY provider, nama_produk"
                    }
                };

                let mut stmt = conn.prepare(sql)?;

                let map_row = |row: &rusqlite::Row| -> rusqlite::Result<ProductSummary> {
                    Ok(ProductSummary {
                        id: row.get(0)?,
                        provider: row.get(1)?,
                        nama_produk: row.get(2)?,
                        kode_produk: row.get(3)?,
                        kategori: row.get(4)?,
                        harga: format!("{:.0}", row.get::<_, f64>(5)?),
                        kode_addon: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    })
                };

                let products: Vec<ProductSummary> = match (has_provider, has_kategori) {
                    (true, true) => {
                        let provider = provider_filter.unwrap();
                        let kategori = kategori_filter.unwrap();
                        stmt.query_map(rusqlite::params![provider, kategori, &search], map_row)?
                            .filter_map(|r| r.ok())
                            .collect()
                    }
                    (true, false) => {
                        let provider = provider_filter.unwrap();
                        stmt.query_map(rusqlite::params![provider, &search], map_row)?
                            .filter_map(|r| r.ok())
                            .collect()
                    }
                    (false, true) => {
                        let kategori = kategori_filter.unwrap();
                        stmt.query_map(rusqlite::params![kategori, &search], map_row)?
                            .filter_map(|r| r.ok())
                            .collect()
                    }
                    (false, false) => stmt
                        .query_map(rusqlite::params![&search], map_row)?
                        .filter_map(|r| r.ok())
                        .collect(),
                };

                Ok(products)
            })
            .await
    }

    /// Get single product by ID
    pub async fn get_product(&self, id: i64) -> Result<Option<Product>, DomainError> {
        self.db.with_reader(|conn| {
            let result = conn.query_row(
                "SELECT id, provider, nama_produk, kode_produk, kategori, harga, kode_addon, aktif, created_at, updated_at
                 FROM produk WHERE id = ?",
                [id],
                |row| {
                    let created_str: String = row.get(8)?;
                    let updated_str: String = row.get(9)?;
                    
                    Ok(Product {
                        id: row.get(0)?,
                        provider: row.get(1)?,
                        nama_produk: row.get(2)?,
                        kode_produk: row.get(3)?,
                        kategori: row.get(4)?,
                        harga: row.get(5)?,
                        kode_addon: row.get(6)?,
                        aktif: row.get::<_, i32>(7)? == 1,
                        created_at: chrono::DateTime::parse_from_rfc3339(&created_str)
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                            .unwrap_or_else(|_| chrono::Utc::now()),
                        updated_at: chrono::DateTime::parse_from_rfc3339(&updated_str)
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                            .unwrap_or_else(|_| chrono::Utc::now()),
                    })
                },
            );
            
            match result {
                Ok(product) => Ok(Some(product)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        }).await
    }

    /// Get list of unique providers
    pub async fn get_providers(&self) -> Result<Vec<String>, DomainError> {
        self.db
            .with_reader(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT DISTINCT provider FROM produk WHERE aktif = 1 ORDER BY provider",
                )?;

                let providers: Vec<String> = stmt
                    .query_map([], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();

                Ok(providers)
            })
            .await
    }

    /// Get distinct addon codes from stok_voucher
    pub async fn get_addon_options(&self) -> Result<Vec<String>, DomainError> {
        self.db.with_reader(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT kode_addon FROM stok_voucher WHERE status = 'ACTIVE' ORDER BY kode_addon"
            )?;
            
            let addons: Vec<String> = stmt.query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            
            Ok(addons)
        }).await
    }
}

impl Clone for ProductService {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
        }
    }
}
