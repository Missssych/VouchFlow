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
    nomor: &str,
    produk: &str,
    kategori: &str,
    harga: f64,
    status: &str,
    _result_code: Option<&str>,
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

    let success = status.eq_ignore_ascii_case("SUCCESS");

    // Parse payload the same way as the local gateway
    let parsed_data: Option<serde_json::Value> = message.and_then(|m| serde_json::from_str(m).ok());
    let mut data_obj: Option<serde_json::Value> = None;
    let mut msg_str = message.unwrap_or("Transaksi gagal").to_string();

    if success {
        let kategori_upper = kategori.trim().to_uppercase();
        if kategori_upper == "CEK" {
            msg_str = "Cek voucher berhasil".to_string();
            if let Some(p) = parsed_data {
                // Field order: expiry_date → product_name → status
                let mut m = serde_json::Map::new();
                m.insert("expiry_date".into(), p.get("expiry_date").cloned().unwrap_or(serde_json::Value::Null));
                m.insert("product_name".into(), p.get("product_name").cloned().unwrap_or(serde_json::Value::Null));
                m.insert("status".into(), p.get("status").cloned().unwrap_or(serde_json::Value::Null));
                data_obj = Some(serde_json::Value::Object(m));
            }
        } else if kategori_upper == "FIS" {
            msg_str = "Transaksi voucher fisik berhasil".to_string();
            if let Some(p) = parsed_data {
                let barcode = p.get("barcode").and_then(|v| v.as_str()).or_else(|| {
                    p.get("barcodes").and_then(|arr| arr.as_array()).and_then(|arr| arr.first()).and_then(|v| v.as_str())
                });
                let serial_number = p.get("serial_number").and_then(|v| v.as_str()).or_else(|| {
                    p.get("serial_numbers").and_then(|arr| arr.as_array()).and_then(|arr| arr.first()).and_then(|v| v.as_str())
                });
                let exp = p.get("exp").and_then(|v| v.as_str()).or_else(|| p.get("expiry_date").and_then(|v| v.as_str()));
                // Field order: barcode → serial_number → exp → harga
                let mut m = serde_json::Map::new();
                m.insert("barcode".into(), barcode.map(|s| serde_json::Value::String(s.to_string())).unwrap_or(serde_json::Value::Null));
                m.insert("serial_number".into(), serial_number.map(|s| serde_json::Value::String(s.to_string())).unwrap_or(serde_json::Value::Null));
                m.insert("exp".into(), exp.map(|s| serde_json::Value::String(s.to_string())).unwrap_or(serde_json::Value::Null));
                m.insert("harga".into(), serde_json::Value::Number(serde_json::Number::from(harga as i64)));
                data_obj = Some(serde_json::Value::Object(m));
            }
        } else if kategori_upper == "RDM" {
            msg_str = "Transaksi redeem voucher berhasil".to_string();
            if let Some(p) = parsed_data {
                let barcode = p.get("barcode").and_then(|v| v.as_str());
                // Field order: barcode → harga
                let mut m = serde_json::Map::new();
                m.insert("barcode".into(), barcode.map(|s| serde_json::Value::String(s.to_string())).unwrap_or(serde_json::Value::Null));
                m.insert("harga".into(), serde_json::Value::Number(serde_json::Number::from(harga as i64)));
                data_obj = Some(serde_json::Value::Object(m));
            }
        } else {
            msg_str = "Transaksi berhasil".to_string();
        }
    }

    // Build payload with field order matching TransaksiApiResponse:
    // success → nomor → produk → message → data (optional) → idtrx
    let mut map = serde_json::Map::new();
    map.insert("success".into(), serde_json::Value::Bool(success));
    map.insert("nomor".into(), serde_json::Value::String(nomor.to_string()));
    map.insert("produk".into(), serde_json::Value::String(produk.to_string()));
    map.insert("message".into(), serde_json::Value::String(msg_str));
    if let Some(d) = data_obj {
        map.insert("data".into(), d);
    }
    map.insert("idtrx".into(), serde_json::Value::String(request_id.to_string()));

    let payload = serde_json::Value::Object(map);

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
