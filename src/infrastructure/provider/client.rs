//! Provider HTTP Client
//!
//! HTTP client for communicating with external provider/terminal services

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::domain::{DomainError, TransactionType};

/// Provider request payload
#[derive(Debug, Clone, Serialize)]
pub struct ProviderRequest {
    pub request_id: String,
    pub produk: String,
    pub nomor: String,
    pub tx_type: String,
}

/// Provider response payload
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderResponse {
    pub success: bool,
    pub result_code: String,
    pub message: Option<String>,
    pub data: Option<serde_json::Value>,
}

/// HTTP client for provider communication
pub struct ProviderClient {
    client: Client,
    base_url: String,
    timeout: Duration,
}

impl ProviderClient {
    /// Create new provider client
    pub fn new(base_url: String, timeout_secs: u64) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url,
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    /// Execute transaction with provider
    pub async fn execute_transaction(
        &self,
        request_id: &str,
        tx_type: TransactionType,
        produk: &str,
        nomor: &str,
    ) -> Result<ProviderResponse, DomainError> {
        let request = ProviderRequest {
            request_id: request_id.to_string(),
            produk: produk.to_string(),
            nomor: nomor.to_string(),
            tx_type: format!("{:?}", tx_type),
        };

        let endpoint = match tx_type {
            TransactionType::Check => "/api/check",
            TransactionType::Redeem => "/api/redeem",
            TransactionType::Physical => "/api/physical",
        };

        let url = format!("{}{}", self.base_url, endpoint);

        let response = self
            .client
            .post(&url)
            .json(&request)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| DomainError::ProviderError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(DomainError::ProviderError(format!(
                "Provider returned status: {}",
                response.status()
            )));
        }

        let result = response
            .json::<ProviderResponse>()
            .await
            .map_err(|e| DomainError::ProviderError(e.to_string()))?;

        Ok(result)
    }

    /// Health check
    pub async fn health_check(&self) -> bool {
        let url = format!("{}/health", self.base_url);

        match self
            .client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }
}

impl Clone for ProviderClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            base_url: self.base_url.clone(),
            timeout: self.timeout,
        }
    }
}
