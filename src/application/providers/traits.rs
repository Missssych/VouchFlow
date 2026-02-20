//! Provider API trait definitions
//! 
//! All providers must implement ProviderApi trait for check and redeem operations.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Provider error types
#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    
    #[error("Decryption failed: {0}")]
    DecryptionError(String),
    
    #[error("Authentication failed: {0}")]
    AuthError(String),
    
    #[error("API error: {code} - {message}")]
    ApiError { code: String, message: String },
    
    #[error("Unknown provider: {0}")]
    UnknownProvider(String),
    
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

/// Response from check voucher operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResponse {
    pub success: bool,
    pub serial_number: String,
    pub product_name: Option<String>,
    pub nominal: Option<f64>,
    pub expiry_date: Option<String>,
    pub status: String,
    pub raw_response: Option<serde_json::Value>,
}

/// Response from redeem voucher operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemResponse {
    pub success: bool,
    pub msisdn: String,
    pub serial_number: String,
    pub message: Option<String>,
    pub transaction_id: Option<String>,
    pub raw_response: Option<serde_json::Value>,
}

/// Provider API trait - must be implemented by all providers
#[async_trait]
pub trait ProviderApi: Send + Sync {
    /// Get provider name
    fn name(&self) -> &'static str;
    
    /// Check voucher validity and details
    async fn check_voucher(&self, barcode: &str) -> Result<CheckResponse, ProviderError>;
    
    /// Redeem voucher for a phone number
    async fn redeem_voucher(&self, msisdn: &str, serial_number: &str) -> Result<RedeemResponse, ProviderError>;
}
