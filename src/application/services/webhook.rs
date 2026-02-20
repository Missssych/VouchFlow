//! Webhook Service
//!
//! Sends transaction results to client webhook URL (raw text body)

use reqwest::Client;
use std::time::Duration;

/// Webhook client for sending transaction results
#[derive(Clone)]
pub struct WebhookClient {
    client: Client,
    max_retries: u32,
}

impl WebhookClient {
    /// Create new webhook client
    pub fn new(timeout_secs: u64) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to create HTTP client");
        
        Self {
            client,
            max_retries: 3,
        }
    }
    
    /// Send webhook notification (raw text body)
    /// Returns true if successful
    pub async fn send(&self, url: &str, body: &str) -> bool {
        let mut attempts = 0;
        
        while attempts < self.max_retries {
            attempts += 1;
            
            match self.client
                .post(url)
                .header("Content-Type", "text/plain")
                .body(body.to_string())
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        tracing::info!(
                            url = %url,
                            attempt = attempts,
                            "Webhook sent successfully"
                        );
                        return true;
                    } else {
                        tracing::warn!(
                            url = %url,
                            status = %response.status(),
                            attempt = attempts,
                            "Webhook returned error status"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        url = %url,
                        error = %e,
                        attempt = attempts,
                        "Webhook request failed"
                    );
                }
            }
            
            // Exponential backoff: 1s, 2s, 4s
            if attempts < self.max_retries {
                let delay = Duration::from_secs(1 << (attempts - 1));
                tokio::time::sleep(delay).await;
            }
        }
        
        tracing::error!(
            url = %url,
            "Webhook failed after {} attempts",
            self.max_retries
        );
        false
    }
    
    /// Build webhook payload for transaction result
    pub fn build_payload(
        request_id: &str,
        tx_id: &str,
        status: &str,
        result_code: Option<&str>,
        message: Option<&str>,
    ) -> String {
        // Raw text format: request_id|tx_id|status|result_code|message
        format!(
            "{}|{}|{}|{}|{}",
            request_id,
            tx_id,
            status,
            result_code.unwrap_or(""),
            message.unwrap_or("")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_build_payload() {
        let payload = WebhookClient::build_payload(
            "TRX001",
            "tx-123",
            "SUCCESS",
            Some("00"),
            Some("OK"),
        );
        assert_eq!(payload, "TRX001|tx-123|SUCCESS|00|OK");
    }
}
