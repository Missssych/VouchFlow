//! Domain errors

use thiserror::Error;

/// Domain-level errors
#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Invalid product code: {0}")]
    InvalidProductCode(String),

    #[error("Transaction not found: {0}")]
    TransactionNotFound(String),

    #[error("Duplicate request: {0}")]
    DuplicateRequest(String),

    #[error("Insufficient stock for product: {0}")]
    InsufficientStock(String),

    #[error("Stock reservation failed: {0}")]
    ReservationFailed(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),
}

impl From<rusqlite::Error> for DomainError {
    fn from(e: rusqlite::Error) -> Self {
        DomainError::DatabaseError(e.to_string())
    }
}

impl From<tokio::sync::broadcast::error::SendError<crate::domain::DomainEvent>> for DomainError {
    fn from(e: tokio::sync::broadcast::error::SendError<crate::domain::DomainEvent>) -> Self {
        DomainError::ChannelError(e.to_string())
    }
}
