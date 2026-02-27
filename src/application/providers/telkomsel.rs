//! Telkomsel Provider Implementation
//!
//! Implements check and redeem voucher for Telkomsel provider.
//! Ported from telkomselVoucher.js

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Deserializer};

use super::{CheckResponse, ProviderApi, ProviderError, RedeemResponse};

/// Helper to deserialize Option<i32> from either integer or string
fn deserialize_option_string_or_int<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrInt {
        String(String),
        Int(i32),
    }

    match Option::<StringOrInt>::deserialize(deserializer)? {
        Some(StringOrInt::String(s)) => {
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse().map(Some).map_err(serde::de::Error::custom)
            }
        }
        Some(StringOrInt::Int(i)) => Ok(Some(i)),
        None => Ok(None),
    }
}

/// Helper to deserialize Option<String> from string/number/bool values
fn deserialize_option_string_like<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringLike {
        String(String),
        Int(i64),
        Float(f64),
        Bool(bool),
    }

    match Option::<StringLike>::deserialize(deserializer)? {
        Some(StringLike::String(s)) => {
            if s.is_empty() {
                Ok(None)
            } else {
                Ok(Some(s))
            }
        }
        Some(StringLike::Int(i)) => Ok(Some(i.to_string())),
        Some(StringLike::Float(f)) => Ok(Some(f.to_string())),
        Some(StringLike::Bool(b)) => Ok(Some(b.to_string())),
        None => Ok(None),
    }
}

/// Telkomsel API configuration
const BASE_URL: &str = "https://www.telkomsel.com/api/voucher";
const DEVICE_ID: &str = "78bc086a-fd87-4621-b656-438b7d2969f5";
const RECAPTCHA_RESPONSE: &str = "MG4sJ@b3MqUoMtdFRFWw2g7r";

/// Default headers for eTelkomsel API
fn default_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "accept",
        "application/json, text/plain, */*".parse().unwrap(),
    );
    headers.insert(
        "accept-language",
        "en-US,en;q=0.9,id;q=0.8".parse().unwrap(),
    );
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("device-id", DEVICE_ID.parse().unwrap());
    headers.insert("origin", "https://www.telkomsel.com".parse().unwrap());
    headers.insert(
        "referer",
        "https://www.telkomsel.com/shops/voucher/check"
            .parse()
            .unwrap(),
    );
    headers.insert(
        "sec-ch-ua",
        "\"Chromium\";v=\"136\", \"Google Chrome\";v=\"136\", \"Not.A/Brand\";v=\"99\""
            .parse()
            .unwrap(),
    );
    headers.insert("sec-ch-ua-mobile", "?0".parse().unwrap());
    headers.insert("sec-ch-ua-platform", "\"Windows\"".parse().unwrap());
    headers.insert("sec-fetch-dest", "empty".parse().unwrap());
    headers.insert("sec-fetch-mode", "cors".parse().unwrap());
    headers.insert("sec-fetch-site", "same-origin".parse().unwrap());
    headers.insert("user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36".parse().unwrap());
    headers.insert("x-url-payload", "T7To2wtSwlyjy6UORZC0Iw==".parse().unwrap());
    headers
}

/// Check voucher response from Telkomsel API
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TelkomselCheckResponse {
    #[serde(default, deserialize_with = "deserialize_option_string_or_int")]
    status_code: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    status_message: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string_or_int")]
    validity: Option<i32>,
    #[serde(
        default,
        alias = "expired_date",
        alias = "expireddate",
        deserialize_with = "deserialize_option_string_like"
    )]
    expired_date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    used_by: Option<String>,
    #[serde(
        default,
        rename = "usedDateTime",
        deserialize_with = "deserialize_option_string_like"
    )]
    used_date_time: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    region: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    serial_number: Option<String>,
}

/// Redeem voucher response from Telkomsel API
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TelkomselRedeemResponse {
    #[serde(default)]
    status: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_option_string_or_int")]
    status_code: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    status_message: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    message: Option<String>,
    #[serde(default)]
    data: Option<TelkomselRedeemData>,
}

#[derive(Debug, Deserialize)]
struct TelkomselRedeemData {
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    code: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    description: Option<String>,
}

/// Telkomsel Provider
pub struct TelkomselProvider {
    client: Client,
}

impl TelkomselProvider {
    /// Create new Telkomsel provider
    pub fn new() -> Self {
        let client = Client::builder()
            .default_headers(default_headers())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }

    /// Parse serial number - truncate to 12 chars if longer
    fn parse_serial(serial: &str) -> String {
        if serial.len() > 12 {
            serial[..12].to_string()
        } else {
            serial.to_string()
        }
    }

    fn clean_message(message: Option<&str>) -> Option<String> {
        let msg = message?.trim();
        if msg.is_empty() || msg.chars().all(|c| c.is_ascii_digit()) {
            None
        } else {
            Some(msg.to_string())
        }
    }

    fn build_redeem_failure_message(response: &TelkomselRedeemResponse) -> String {
        let error_code = response.data.as_ref().and_then(|data| data.code.as_deref());

        match error_code {
            Some("15") => "voucher sudah digunakan (code 15)".to_string(),
            Some("20") => "voucher belum ter-inject (code 20)".to_string(),
            Some("3023") => "kode voucher salah (code 3023)".to_string(),
            _ => {
                let description = response
                    .data
                    .as_ref()
                    .and_then(|data| data.description.as_deref())
                    .and_then(|msg| Self::clean_message(Some(msg)))
                    .or_else(|| Self::clean_message(response.message.as_deref()))
                    .or_else(|| Self::clean_message(response.status_message.as_deref()));

                if let Some(description) = description {
                    if let Some(code) = error_code {
                        format!("{} (code {})", description, code)
                    } else {
                        description
                    }
                } else if response.message.as_deref() == Some("400")
                    || response.status_message.as_deref() == Some("400")
                    || response.status_code == Some(400)
                {
                    "nomor tujuan salah".to_string()
                } else {
                    "Provider request failed".to_string()
                }
            }
        }
    }

    fn map_redeem_outcome(
        http_status: reqwest::StatusCode,
        response: &TelkomselRedeemResponse,
    ) -> (bool, Option<String>) {
        let error_code = response.data.as_ref().and_then(|data| data.code.as_deref());
        let known_failure_code = matches!(error_code, Some("15") | Some("20") | Some("3023"));
        let is_bad_request = response.message.as_deref() == Some("400")
            || response.status_message.as_deref() == Some("400")
            || response.status_code == Some(400);

        let status_flag_ok = response.status.unwrap_or(true);
        let status_code_ok = response
            .status_code
            .map(|code| matches!(code, 0 | 1 | 200))
            .unwrap_or(true);

        let success = http_status.is_success()
            && status_flag_ok
            && status_code_ok
            && !is_bad_request
            && !known_failure_code;

        let message = if success {
            Self::clean_message(response.message.as_deref())
                .or_else(|| Self::clean_message(response.status_message.as_deref()))
                .or_else(|| {
                    response
                        .data
                        .as_ref()
                        .and_then(|data| data.description.as_deref())
                        .and_then(|msg| Self::clean_message(Some(msg)))
                })
                .or_else(|| Some("Success".to_string()))
        } else {
            Some(Self::build_redeem_failure_message(response))
        };

        (success, message)
    }
}

impl Default for TelkomselProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderApi for TelkomselProvider {
    fn name(&self) -> &'static str {
        "Telkomsel"
    }

    async fn check_voucher(&self, barcode: &str) -> Result<CheckResponse, ProviderError> {
        tracing::info!("Telkomsel: Checking voucher {}", barcode);

        let parsed_serial = Self::parse_serial(barcode);
        let url = format!("{}/check", BASE_URL);

        let body = serde_json::json!({
            "serialNumber": parsed_serial,
            "recaptcharesponse": RECAPTCHA_RESPONSE,
            "voucher_type": "voucher"
        });

        let resp = self.client.post(&url).json(&body).send().await?;

        let status_code = resp.status();
        let raw_text = resp.text().await?;

        tracing::debug!("Telkomsel check response: {}", raw_text);

        let check_resp: TelkomselCheckResponse = serde_json::from_str(&raw_text).map_err(|e| {
            ProviderError::InvalidResponse(format!("Failed to parse response: {}", e))
        })?;

        let raw_json: Option<serde_json::Value> = serde_json::from_str(&raw_text).ok();

        // Parse statusCode according to JS logic
        match check_resp.status_code {
            // statusCode 0: Valid/available voucher
            Some(0) => Ok(CheckResponse {
                success: true,
                serial_number: barcode.to_string(),
                product_name: None,
                nominal: None,
                expiry_date: check_resp.expired_date.clone(),
                status: check_resp
                    .status_message
                    .unwrap_or_else(|| "AVAILABLE".to_string()),
                raw_response: raw_json.clone(),
            }),
            // statusCode 1 or 3: Used voucher - return used info
            Some(1) | Some(3) => {
                let description = match (&check_resp.description, &check_resp.validity) {
                    (Some(desc), Some(validity)) => {
                        Some(format!("Voucher {}-{} Day", desc, validity))
                    }
                    (Some(desc), None) => Some(desc.clone()),
                    _ => None,
                };

                Ok(CheckResponse {
                    success: true,
                    serial_number: barcode.to_string(),
                    product_name: description,
                    nominal: None,
                    expiry_date: check_resp.expired_date.clone(),
                    status: check_resp
                        .status_message
                        .unwrap_or_else(|| "USED".to_string()),
                    raw_response: raw_json.clone(),
                })
            }
            // statusCode 5 or 6: Expired voucher
            Some(5) | Some(6) => Ok(CheckResponse {
                success: true,
                serial_number: barcode.to_string(),
                product_name: None,
                nominal: None,
                expiry_date: check_resp.expired_date.clone(),
                status: format!(
                    "EXPIRED (exp: {})",
                    check_resp.expired_date.as_deref().unwrap_or("unknown")
                ),
                raw_response: raw_json.clone(),
            }),
            // Other statusCodes or missing serial - return raw response
            _ => {
                // Check if serial_number is missing or empty (invalid voucher)
                let is_invalid = check_resp
                    .serial_number
                    .as_ref()
                    .map(|s| s.is_empty())
                    .unwrap_or(true);

                if is_invalid {
                    Ok(CheckResponse {
                        success: false,
                        serial_number: barcode.to_string(),
                        product_name: None,
                        nominal: None,
                        expiry_date: None,
                        status: "INVALID".to_string(),
                        raw_response: raw_json.clone(),
                    })
                } else {
                    // Return raw response for unknown status codes
                    Ok(CheckResponse {
                        success: status_code.is_success(),
                        serial_number: barcode.to_string(),
                        product_name: None,
                        nominal: None,
                        expiry_date: check_resp.expired_date,
                        status: format!("UNKNOWN_STATUS_{}", check_resp.status_code.unwrap_or(-1)),
                        raw_response: raw_json,
                    })
                }
            }
        }
    }

    async fn redeem_voucher(
        &self,
        msisdn: &str,
        serial_number: &str,
    ) -> Result<RedeemResponse, ProviderError> {
        tracing::info!(
            "Telkomsel: Redeeming voucher {} for {}",
            serial_number,
            msisdn
        );

        let url = format!("{}/redeem", BASE_URL);

        let body = serde_json::json!({
            // API field still uses `hrn`, value comes from stock `serial_number`
            "hrn": serial_number,
            "msisdn": msisdn,
            "no-captcha": true,
            "voucher_type": "voucher",
            "recaptcharesponse": RECAPTCHA_RESPONSE
        });

        // Use different headers for redeem (as per JS)
        let resp = self
            .client
            .post(&url)
            .header(
                "referer",
                "https://www.telkomsel.com/shops/voucher/redeem?standalon=true",
            )
            .json(&body)
            .send()
            .await?;

        let status_code = resp.status();
        let raw_text = resp.text().await?;

        tracing::debug!("Telkomsel redeem response: {}", raw_text);

        let redeem_resp: TelkomselRedeemResponse =
            serde_json::from_str(&raw_text).map_err(|e| {
                ProviderError::InvalidResponse(format!("Failed to parse response: {}", e))
            })?;

        let raw_json: Option<serde_json::Value> = serde_json::from_str(&raw_text).ok();

        let (success, message) = Self::map_redeem_outcome(status_code, &redeem_resp);

        Ok(RedeemResponse {
            success,
            msisdn: msisdn.to_string(),
            serial_number: serial_number.to_string(),
            message,
            transaction_id: None,
            raw_response: raw_json,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_serial() {
        assert_eq!(
            TelkomselProvider::parse_serial("123456789012"),
            "123456789012"
        );
        assert_eq!(
            TelkomselProvider::parse_serial("12345678901234567"),
            "123456789012"
        );
        assert_eq!(TelkomselProvider::parse_serial("12345"), "12345");
    }

    #[test]
    fn test_deserialize_telkomsel_mixed_types() {
        let json_str = r#"
        {
            "statusCode": "0",
            "statusMessage": "Success",
            "validity": "30",
            "serialNumber": "123456789012"
        }
        "#;

        let resp: TelkomselCheckResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(resp.status_code, Some(0));
        assert_eq!(resp.validity, Some(30));

        let json_int = r#"
        {
            "statusCode": 0,
            "statusMessage": "Success",
            "validity": 30,
            "serialNumber": "123456789012"
        }
        "#;

        let resp_int: TelkomselCheckResponse = serde_json::from_str(json_int).unwrap();
        assert_eq!(resp_int.status_code, Some(0));
        assert_eq!(resp_int.validity, Some(30));
    }

    #[test]
    fn test_deserialize_telkomsel_redeem_numeric_message_fields() {
        let json_str = r#"
        {
            "statusCode": 400,
            "statusMessage": 400,
            "message": 400
        }
        "#;

        let resp: TelkomselRedeemResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(resp.status_code, Some(400));
        assert_eq!(resp.status_message.as_deref(), Some("400"));
        assert_eq!(resp.message.as_deref(), Some("400"));
    }

    #[test]
    fn test_deserialize_telkomsel_redeem_nested_data_code() {
        let json_str = r#"
        {
            "status": true,
            "message": 400,
            "data": {
                "code": "15",
                "description": "VoucherAlreadyUsed"
            }
        }
        "#;

        let resp: TelkomselRedeemResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(resp.status, Some(true));
        assert_eq!(resp.message.as_deref(), Some("400"));
        assert_eq!(
            resp.data.as_ref().and_then(|data| data.code.as_deref()),
            Some("15")
        );
        assert_eq!(
            resp.data
                .as_ref()
                .and_then(|data| data.description.as_deref()),
            Some("VoucherAlreadyUsed")
        );
    }

    #[test]
    fn test_map_redeem_outcome_code_15_returns_clear_failure_message() {
        let response = TelkomselRedeemResponse {
            status: Some(true),
            status_code: None,
            status_message: None,
            message: Some("400".to_string()),
            data: Some(TelkomselRedeemData {
                code: Some("15".to_string()),
                description: Some("VoucherAlreadyUsed".to_string()),
            }),
        };

        let (success, message) =
            TelkomselProvider::map_redeem_outcome(reqwest::StatusCode::OK, &response);

        assert!(!success);
        assert_eq!(
            message.as_deref(),
            Some("redeem voucher gagal, voucher sudah digunakan (code 15)")
        );
    }

    #[test]
    fn test_map_redeem_outcome_code_20_returns_clear_failure_message() {
        let response = TelkomselRedeemResponse {
            status: Some(true),
            status_code: None,
            status_message: None,
            message: Some("400".to_string()),
            data: Some(TelkomselRedeemData {
                code: Some("20".to_string()),
                description: Some("VoucherNotInject".to_string()),
            }),
        };

        let (success, message) =
            TelkomselProvider::map_redeem_outcome(reqwest::StatusCode::OK, &response);

        assert!(!success);
        assert_eq!(
            message.as_deref(),
            Some("redeem voucher gagal, voucher belum ter-inject (code 20)")
        );
    }

    #[test]
    fn test_map_redeem_outcome_code_3023_returns_clear_failure_message() {
        let response = TelkomselRedeemResponse {
            status: Some(true),
            status_code: None,
            status_message: None,
            message: Some("400".to_string()),
            data: Some(TelkomselRedeemData {
                code: Some("3023".to_string()),
                description: Some("InvalidVoucherCode".to_string()),
            }),
        };

        let (success, message) =
            TelkomselProvider::map_redeem_outcome(reqwest::StatusCode::OK, &response);

        assert!(!success);
        assert_eq!(
            message.as_deref(),
            Some("redeem voucher gagal, kode voucher salah (code 3023)")
        );
    }

    #[test]
    fn test_map_redeem_outcome_unknown_bad_request_falls_back_to_nomor_salah() {
        let response = TelkomselRedeemResponse {
            status: Some(true),
            status_code: None,
            status_message: None,
            message: Some("400".to_string()),
            data: None,
        };

        let (success, message) =
            TelkomselProvider::map_redeem_outcome(reqwest::StatusCode::OK, &response);

        assert!(!success);
        assert_eq!(
            message.as_deref(),
            Some("redeem voucher gagal, nomor tujuan salah")
        );
    }

    #[test]
    fn test_map_redeem_outcome_success_with_default_message() {
        let response = TelkomselRedeemResponse {
            status: Some(true),
            status_code: Some(0),
            status_message: None,
            message: None,
            data: None,
        };

        let (success, message) =
            TelkomselProvider::map_redeem_outcome(reqwest::StatusCode::OK, &response);

        assert!(success);
        assert_eq!(message.as_deref(), Some("redeem voucher berhasil"));
    }
}
