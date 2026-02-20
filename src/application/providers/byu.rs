//! Byu Provider Implementation
//!
//! Implements redeem and check voucher for Byu (by.u) with the latest PIDAW endpoints.
//! Check flow:
//! 1) GET voucher status (contains sku)
//! 2) GET product variant detail by sku
//! 3) Return merged response in `raw_response`

use async_trait::async_trait;
use reqwest::{
    Client,
    header::{HeaderMap, HeaderValue},
};
use serde_json::Value;

use super::{CheckResponse, ProviderApi, ProviderError, RedeemResponse};

/// Byu API configuration
const BASE_URL: &str = "https://pidaw-app.cx.byu.id";
const PATH_CHECK_SKU: &str = "v1/voucher/status";
const PATH_CHECK_DETAIL_SKU: &str = "v3/internal/product-variants";
const PATH_HANDSHAKE: &str = "v1/msisdn/validation";
const PATH_REDEEM: &str = "v1/vouchers/fetch-product";
const TOKEN_HEADER: &str = "byutoken";
const DEFAULT_PAYMENT_METHOD_ID: &str = "PMP-VRTC9Q2VWU";
const DEFAULT_DEVICE_ID: &str = "17705307848413598796933";

/// Default headers for PIDAW Byu API.
fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("accept", HeaderValue::from_static("application/json"));
    headers.insert(
        "accept-language",
        HeaderValue::from_static("en-GB,en;q=0.9,en-US;q=0.8"),
    );
    headers.insert("cache-control", HeaderValue::from_static("no-cache"));
    headers.insert(
        "origin",
        HeaderValue::from_static("https://pidaw-webfront.cx.byu.id"),
    );
    headers.insert("pragma", HeaderValue::from_static("no-cache"));
    headers.insert("priority", HeaderValue::from_static("u=1, i"));
    headers.insert(
        "referer",
        HeaderValue::from_static("https://pidaw-webfront.cx.byu.id/"),
    );
    headers.insert(
        "sec-ch-ua",
        HeaderValue::from_static(
            "\"Not(A:Brand\";v=\"8\", \"Chromium\";v=\"144\", \"Microsoft Edge\";v=\"144\"",
        ),
    );
    headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
    headers.insert("sec-ch-ua-platform", HeaderValue::from_static("\"Windows\""));
    headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
    headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    headers.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
    headers.insert("slocation", HeaderValue::from_static("CL"));
    headers.insert(
        "user-agent",
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36 Edg/144.0.0.0"),
    );
    headers.insert("x-deviceid", HeaderValue::from_static(DEFAULT_DEVICE_ID));

    if let Ok(cookie) = std::env::var("BYU_COOKIE") {
        let cookie = cookie.trim();
        if !cookie.is_empty() {
            if let Ok(value) = HeaderValue::from_str(cookie) {
                headers.insert("cookie", value);
            } else {
                tracing::warn!("Byu: BYU_COOKIE is set but invalid, skipping cookie header");
            }
        }
    }

    headers
}

/// Byu Provider
pub struct ByuProvider {
    client: Client,
}

impl ByuProvider {
    /// Create new Byu provider
    pub fn new() -> Self {
        let client = Client::builder()
            .default_headers(default_headers())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }

    fn payment_method_id() -> String {
        std::env::var("BYU_PAYMENT_METHOD_ID")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_PAYMENT_METHOD_ID.to_string())
    }

    fn value_as_non_empty_str(value: &Value) -> Option<&str> {
        let value = value.as_str()?.trim();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    }

    fn parse_f64(value: &Value) -> Option<f64> {
        if let Some(v) = value.as_f64() {
            return Some(v);
        }
        if let Some(v) = value.as_i64() {
            return Some(v as f64);
        }
        if let Some(v) = value.as_u64() {
            return Some(v as f64);
        }

        value.as_str()?.trim().parse::<f64>().ok()
    }

    fn find_attribute_value<'a>(variant: &'a Value, attribute_name: &str) -> Option<&'a Value> {
        variant
            .get("attributes")
            .and_then(Value::as_array)?
            .iter()
            .find_map(|attr| {
                let name = attr.get("name").and_then(Value::as_str)?;
                if name.eq_ignore_ascii_case(attribute_name) {
                    attr.get("value")
                } else {
                    None
                }
            })
    }

    fn extract_product_name(variant: &Value) -> Option<String> {
        Self::find_attribute_value(variant, "displayName")
            .and_then(Self::value_as_non_empty_str)
            .or_else(|| {
                Self::find_attribute_value(variant, "title").and_then(Self::value_as_non_empty_str)
            })
            .or_else(|| variant.get("title").and_then(Self::value_as_non_empty_str))
            .or_else(|| variant.get("subtitle").and_then(Self::value_as_non_empty_str))
            .map(ToOwned::to_owned)
    }

    fn extract_nominal(variant: &Value) -> Option<f64> {
        Self::find_attribute_value(variant, "price")
            .and_then(Self::parse_f64)
            .or_else(|| variant.get("discountedPrice").and_then(Self::parse_f64))
            .or_else(|| variant.get("price").and_then(Self::parse_f64))
    }

    fn pick_variant<'a>(detail_response: &'a Value, sku: Option<&str>) -> Option<&'a Value> {
        let variants = detail_response
            .pointer("/result/productVariants")
            .and_then(Value::as_array)?;

        if let Some(target_sku) = sku {
            variants
                .iter()
                .find(|variant| {
                    variant
                        .get("sku")
                        .and_then(Self::value_as_non_empty_str)
                        .map(|sku| sku.eq_ignore_ascii_case(target_sku))
                        .unwrap_or(false)
                })
                .or_else(|| variants.first())
        } else {
            variants.first()
        }
    }

    fn json_string_pointer(value: &Value, pointer: &str) -> Option<String> {
        value
            .pointer(pointer)
            .and_then(Self::value_as_non_empty_str)
            .map(ToOwned::to_owned)
    }

    fn json_i64_pointer(value: &Value, pointer: &str) -> Option<i64> {
        value.pointer(pointer).and_then(Value::as_i64)
    }

    fn extract_redeem_message(response: &Value) -> Option<String> {
        let message = Self::json_string_pointer(response, "/message")
            .or_else(|| Self::json_string_pointer(response, "/result/message"))
            .or_else(|| Self::json_string_pointer(response, "/failure/description"))
            .or_else(|| Self::json_string_pointer(response, "/errorDetails/errorMessage"))
            .or_else(|| Self::json_string_pointer(response, "/error/message"))
            .or_else(|| Self::json_string_pointer(response, "/error"))
            .or_else(|| Self::json_string_pointer(response, "/detail"));

        let failure_code = Self::json_string_pointer(response, "/failure/errorCode")
            .or_else(|| Self::json_string_pointer(response, "/errorDetails/errorCode"));
        let failure_id = Self::json_i64_pointer(response, "/failure/errorId");

        match (message, failure_code, failure_id) {
            (Some(msg), Some(code), Some(id)) => Some(format!("{msg} ({code}, id={id})")),
            (Some(msg), Some(code), None) => Some(format!("{msg} ({code})")),
            (Some(msg), None, Some(id)) => Some(format!("{msg} (id={id})")),
            (Some(msg), None, None) => Some(msg),
            (None, Some(code), Some(id)) => Some(format!("{code} (id={id})")),
            (None, Some(code), None) => Some(code),
            (None, None, Some(id)) => Some(format!("Error ID {id}")),
            (None, None, None) => None,
        }
    }

    fn extract_transaction_id(response: &Value) -> Option<String> {
        Self::json_string_pointer(response, "/transactionId")
            .or_else(|| Self::json_string_pointer(response, "/result/transactionId"))
            .or_else(|| Self::json_string_pointer(response, "/result/data/transactionId"))
            .or_else(|| Self::json_string_pointer(response, "/data/transactionId"))
            .or_else(|| Self::json_string_pointer(response, "/result/id"))
            .or_else(|| Self::json_string_pointer(response, "/requestId"))
    }

    /// Handshake redeem flow to get one-time `byutoken` from response header.
    async fn handshake_get_token(&self, msisdn: &str) -> Result<String, ProviderError> {
        let url = format!("{}/{}", BASE_URL, PATH_HANDSHAKE);

        let response = self
            .client
            .get(&url)
            .query(&[("number", msisdn)])
            .header("msisdn", msisdn)
            .send()
            .await?;

        let status = response.status();
        let token = response.headers().get(TOKEN_HEADER).cloned();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(ProviderError::AuthError(format!(
                "Handshake failed: status={} body={}",
                status, body
            )));
        }

        let token = token.ok_or_else(|| {
            ProviderError::AuthError(format!(
                "Handshake response missing '{}' header",
                TOKEN_HEADER
            ))
        })?;

        let token = token.to_str().map_err(|e| {
            ProviderError::AuthError(format!(
                "Handshake '{}' header invalid UTF-8: {}",
                TOKEN_HEADER, e
            ))
        })?;

        if token.trim().is_empty() {
            return Err(ProviderError::AuthError(
                "Handshake token is empty".to_string(),
            ));
        }

        Ok(token.to_string())
    }
}

impl Default for ByuProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderApi for ByuProvider {
    fn name(&self) -> &'static str {
        "Byu"
    }

    async fn check_voucher(&self, barcode: &str) -> Result<CheckResponse, ProviderError> {
        tracing::info!("Byu: Checking voucher {}", barcode);

        let check_url = format!("{}/{}", BASE_URL, PATH_CHECK_SKU);
        let check_response = self
            .client
            .get(&check_url)
            .query(&[("serial_number", barcode)])
            .send()
            .await?;

        let check_status = check_response.status();
        let check_text = check_response.text().await?;
        let check_json: Value = serde_json::from_str(&check_text).map_err(|e| {
            ProviderError::InvalidResponse(format!("Failed to parse byu check_sku response: {}", e))
        })?;

        let check_success = check_status.is_success()
            && check_json
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false);

        let check_data = check_json.pointer("/result/data");
        let sku = check_data
            .and_then(|data| data.get("sku"))
            .and_then(Self::value_as_non_empty_str)
            .map(ToOwned::to_owned);
        let expiry_date = check_data
            .and_then(|data| data.get("expiryDate"))
            .and_then(Self::value_as_non_empty_str)
            .map(ToOwned::to_owned);
        let state = check_data
            .and_then(|data| data.get("state"))
            .and_then(Self::value_as_non_empty_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "UNKNOWN".to_string());

        let mut detail_json: Option<Value> = None;
        let mut detail_success = false;

        if let Some(sku_value) = sku.as_deref() {
            let detail_url = format!("{}/{}", BASE_URL, PATH_CHECK_DETAIL_SKU);
            let detail_response = self
                .client
                .get(&detail_url)
                .query(&[("sku", sku_value)])
                .send()
                .await?;

            let detail_status = detail_response.status();
            let detail_text = detail_response.text().await?;
            let parsed_detail: Value = serde_json::from_str(&detail_text).map_err(|e| {
                ProviderError::InvalidResponse(format!(
                    "Failed to parse byu check_detail_sku response: {}",
                    e
                ))
            })?;

            detail_success = detail_status.is_success()
                && parsed_detail
                    .get("success")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            detail_json = Some(parsed_detail);
        }

        let variant = detail_json
            .as_ref()
            .and_then(|detail| Self::pick_variant(detail, sku.as_deref()));

        let success = if sku.is_some() {
            check_success && detail_success
        } else {
            check_success
        };

        Ok(CheckResponse {
            success,
            serial_number: barcode.to_string(),
            product_name: variant.and_then(Self::extract_product_name),
            nominal: variant.and_then(Self::extract_nominal),
            expiry_date,
            status: state,
            raw_response: Some(serde_json::json!({
                "check_sku": check_json,
                "detail_sku": detail_json,
            })),
        })
    }

    async fn redeem_voucher(&self, msisdn: &str, serial_number: &str) -> Result<RedeemResponse, ProviderError> {
        tracing::info!("Byu: Redeeming voucher {} for {}", serial_number, msisdn);

        let token = self.handshake_get_token(msisdn).await?;

        let redeem_url = format!("{}/{}", BASE_URL, PATH_REDEEM);
        let redeem_payload = serde_json::json!({
            "msisdn": msisdn,
            "voucher": serial_number,
            "paymentMethodId": Self::payment_method_id(),
        });

        let response = self
            .client
            .post(&redeem_url)
            .header("content-type", "application/json")
            .header("msisdn", msisdn)
            .header(TOKEN_HEADER, &token)
            .json(&redeem_payload)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let parsed_json: Option<Value> = serde_json::from_str(&body).ok();

        let body_success = parsed_json
            .as_ref()
            .and_then(|json| json.get("success"))
            .and_then(Value::as_bool);

        let success = status.is_success() && body_success.unwrap_or(true);
        let message = parsed_json
            .as_ref()
            .and_then(Self::extract_redeem_message)
            .or_else(|| {
                if status.is_success() {
                    None
                } else {
                    Some(body.clone())
                }
            });
        let transaction_id = parsed_json
            .as_ref()
            .and_then(Self::extract_transaction_id);

        Ok(RedeemResponse {
            success,
            msisdn: msisdn.to_string(),
            serial_number: serial_number.to_string(),
            message,
            transaction_id,
            raw_response: parsed_json,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_product_name_prefers_display_name_attribute() {
        let variant = serde_json::json!({
            "title": "TITLE",
            "attributes": [
                {"name": "displayName", "value": "Jajan 7.5 GB 7 Hari"},
                {"name": "title", "value": "Other Title"}
            ]
        });

        let product_name = ByuProvider::extract_product_name(&variant);
        assert_eq!(product_name.as_deref(), Some("Jajan 7.5 GB 7 Hari"));
    }

    #[test]
    fn test_extract_nominal_from_price_attribute() {
        let variant = serde_json::json!({
            "price": 0,
            "attributes": [
                {"name": "price", "value": "17000"}
            ]
        });

        let nominal = ByuProvider::extract_nominal(&variant);
        assert_eq!(nominal, Some(17000.0));
    }

    #[test]
    fn test_extract_redeem_message_used_voucher_failure_payload() {
        let payload = serde_json::json!({
            "errorDetails": {
                "errorCode": "PVIRERR005",
                "errorMessage": "Used Voucher"
            },
            "failure": {
                "description": "Used Voucher",
                "errorCode": "USED_VOUCHER",
                "errorId": 3006
            },
            "requestId": "eb458682-4f8f-4332-a3ff-9614011219c4",
            "success": false
        });

        let message = ByuProvider::extract_redeem_message(&payload);
        assert_eq!(
            message.as_deref(),
            Some("Used Voucher (USED_VOUCHER, id=3006)")
        );
    }

    #[test]
    fn test_extract_redeem_message_invalid_voucher_failure_payload() {
        let payload = serde_json::json!({
            "errorDetails": {
                "errorCode": "PVIRERR003",
                "errorMessage": "Invalid Voucher"
            },
            "failure": {
                "description": "Invalid Voucher",
                "errorCode": "INVALID_VOUCHER",
                "errorId": 3002
            },
            "requestId": "0638b5d2-ca66-4b83-8114-f1e16250da55",
            "success": false
        });

        let message = ByuProvider::extract_redeem_message(&payload);
        assert_eq!(
            message.as_deref(),
            Some("Invalid Voucher (INVALID_VOUCHER, id=3002)")
        );
    }

    #[test]
    fn test_extract_transaction_id_from_request_id() {
        let payload = serde_json::json!({
            "requestId": "eb458682-4f8f-4332-a3ff-9614011219c4",
            "success": false
        });

        let tx_id = ByuProvider::extract_transaction_id(&payload);
        assert_eq!(
            tx_id.as_deref(),
            Some("eb458682-4f8f-4332-a3ff-9614011219c4")
        );
    }
}

