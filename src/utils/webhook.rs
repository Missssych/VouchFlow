//! Webhook sending utility
//!
//! Standalone webhook sender, used by UI callbacks and services.
//! Extracted from DB Writer to ensure network I/O stays outside the database actor.

/// Send webhook notification
///
/// Fires and forgets — does not block the caller on failure.
pub async fn send_webhook(
    url: &str,
    request_id: &str,
    tx_id: &str,
    status: &str,
    result_code: Option<&str>,
    message: Option<&str>,
) {
    if url.is_empty() {
        return;
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to build HTTP client for webhook: {}", e);
            return;
        }
    };

    let payload = serde_json::json!({
        "request_id": request_id,
        "tx_id": tx_id,
        "status": status,
        "result_code": result_code,
        "message": message,
    });

    tracing::info!(url = %url, tx_id = %tx_id, status = %status, "Sending webhook");

    match client.post(url).json(&payload).send().await {
        Ok(resp) => {
            tracing::info!(
                url = %url, status_code = %resp.status(),
                "Webhook sent successfully"
            );
        }
        Err(e) => {
            tracing::error!(url = %url, error = %e, "Failed to send webhook");
        }
    }
}
