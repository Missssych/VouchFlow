//! Smartfren Provider Implementation
//! 
//! Implements redeem voucher for Smartfren provider.
//! Uses AES-ECB encryption with OpenSSL-compatible key derivation for CryptoJS compatibility.
//! Ported from smart.js

use async_trait::async_trait;
use aes::cipher::KeyInit;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::Client;
use serde::{Deserialize, Deserializer};

use super::{CheckResponse, ProviderApi, ProviderError, RedeemResponse};

/// Smartfren API configuration
const BASE_URL: &str = "https://www.smartfren.com/voucher-topup/api";
const SECRET_KEY: &str = "A:R7G!G1<1xE;AI5Cyxflma/STsXJ<Sf";

/// Default headers for Smartfren API
fn default_headers(mdn: &str) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36".parse().unwrap());
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert("Origin", "https://www.smartfren.com".parse().unwrap());
    headers.insert("Referer", format!("https://www.smartfren.com/voucher-topup?val={}", mdn).parse().unwrap());
    headers
}

/// Handshake response from Smartfren API
#[derive(Debug, Deserialize)]
struct HandshakeResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: Option<HandshakeData>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HandshakeData {
    #[serde(default)]
    token: Option<String>,
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
            if s.trim().is_empty() {
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

/// Redeem response from Smartfren API
#[derive(Debug, Deserialize)]
struct SmartfrenRedeemResponse {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    message: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string_like")]
    msg: Option<String>,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

/// Smartfren Provider
pub struct SmartfrenProvider {
    client: Client,
}

impl SmartfrenProvider {
    /// Create new Smartfren provider
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        
        Self { client }
    }
    
    /// Encrypt payload using CryptoJS-compatible AES encryption
    /// CryptoJS with string key uses OpenSSL-compatible key derivation (EVP_BytesToKey)
    /// NOTE: JavaScript uses ECB mode, so IV is derived but not used
    fn encrypt_payload(data: &str, passphrase: &str) -> Result<String, ProviderError> {
        // Generate random 8-byte salt
        let salt = Self::generate_salt();
        
        // Derive key using EVP_BytesToKey (OpenSSL compatible)
        // Note: IV is derived but ignored for ECB mode
        let (key, _iv) = Self::evp_bytes_to_key(passphrase.as_bytes(), &salt, 32, 16);
        
        // Encrypt using AES-256-ECB (as specified in smart.js)
        let encrypted = Self::aes_256_ecb_encrypt(data.as_bytes(), &key)?;
        
        // Format: "Salted__" + salt + encrypted_data (OpenSSL format)
        let mut result = Vec::with_capacity(8 + 8 + encrypted.len());
        result.extend_from_slice(b"Salted__");
        result.extend_from_slice(&salt);
        result.extend_from_slice(&encrypted);
        
        Ok(BASE64.encode(&result))
    }
    
    /// Generate random 8-byte salt
    fn generate_salt() -> [u8; 8] {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        
        let mut salt = [0u8; 8];
        for (i, byte) in salt.iter_mut().enumerate() {
            *byte = ((seed >> (i * 8)) & 0xFF) as u8;
        }
        salt
    }
    
    /// EVP_BytesToKey key derivation (OpenSSL compatible, used by CryptoJS)
    /// CryptoJS default hasher for passphrase mode is MD5.
    fn evp_bytes_to_key(password: &[u8], salt: &[u8], key_len: usize, iv_len: usize) -> (Vec<u8>, Vec<u8>) {
        let mut derived = Vec::new();
        let mut block = Vec::new();
        
        while derived.len() < key_len + iv_len {
            let mut hasher_input = Vec::with_capacity(block.len() + password.len() + salt.len());
            if !block.is_empty() {
                hasher_input.extend_from_slice(&block);
            }
            hasher_input.extend_from_slice(password);
            hasher_input.extend_from_slice(salt);

            block = md5::compute(hasher_input).0.to_vec();
            derived.extend_from_slice(&block);
        }
        
        let key = derived[..key_len].to_vec();
        let iv = derived[key_len..key_len + iv_len].to_vec();
        (key, iv)
    }
    
    /// AES-256-ECB encryption (matches smart.js mode: CryptoJS.mode.ECB)
    fn aes_256_ecb_encrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>, ProviderError> {
        use aes::cipher::{BlockEncrypt, generic_array::GenericArray};
        use aes::Aes256;
        
        if key.len() != 32 {
            return Err(ProviderError::DecryptionError("Invalid key length".to_string()));
        }
        
        let key_arr: [u8; 32] = key.try_into()
            .map_err(|_| ProviderError::DecryptionError("Invalid key length".to_string()))?;
        
        // Create cipher
        let cipher = Aes256::new(GenericArray::from_slice(&key_arr));
        
        // Apply PKCS7 padding
        let block_size = 16;
        let padding_len = block_size - (data.len() % block_size);
        let mut padded_data = data.to_vec();
        padded_data.extend(std::iter::repeat(padding_len as u8).take(padding_len));
        
        // Encrypt each block
        let mut result = Vec::with_capacity(padded_data.len());
        for chunk in padded_data.chunks(block_size) {
            let mut block = GenericArray::clone_from_slice(chunk);
            cipher.encrypt_block(&mut block);
            result.extend_from_slice(&block);
        }
        
        Ok(result)
    }

    fn clean_message(message: Option<&str>) -> Option<String> {
        let msg = message?.trim();
        if msg.is_empty() {
            None
        } else {
            Some(msg.to_string())
        }
    }

    fn value_as_message(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(s) => Self::clean_message(Some(s)),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    fn extract_message_from_data(data: Option<&serde_json::Value>) -> Option<String> {
        let data = data?;
        if let Some(msg) = Self::value_as_message(data) {
            return Some(msg);
        }

        let obj = data.as_object()?;
        for key in ["message", "msg", "description", "statusMessage", "resultMessage"] {
            if let Some(value) = obj.get(key) {
                if let Some(msg) = Self::value_as_message(value) {
                    return Some(msg);
                }
            }
        }

        None
    }

    fn is_failure_status(status: &str) -> bool {
        matches!(
            status.trim().to_ascii_lowercase().as_str(),
            "-1" | "99" | "false" | "failed" | "error"
        )
    }

    fn is_success_status(status: &str) -> bool {
        matches!(
            status.trim().to_ascii_lowercase().as_str(),
            "1" | "0" | "00" | "200" | "true" | "success" | "ok"
        )
    }

    fn build_redeem_message(
        response: &SmartfrenRedeemResponse,
        success: bool,
    ) -> Option<String> {
        let status = response
            .status
            .as_deref()
            .and_then(|s| Self::clean_message(Some(s)));

        let provider_message = Self::clean_message(response.msg.as_deref())
            .or_else(|| Self::clean_message(response.message.as_deref()))
            .or_else(|| Self::extract_message_from_data(response.data.as_ref()));

        if success {
            return provider_message.or_else(|| Some("redeem voucher berhasil".to_string()));
        }

        let base = if let Some(msg) = provider_message {
            let lower = msg.to_ascii_lowercase();
            if lower.contains("pin voucher") {
                format!("redeem voucher gagal, PIN voucher kemungkinan salah/tidak valid: {}", msg)
            } else {
                format!("redeem voucher gagal: {}", msg)
            }
        } else {
            "redeem voucher gagal".to_string()
        };

        if let Some(status) = status {
            Some(format!("{} (status {})", base, status))
        } else {
            Some(base)
        }
    }

    fn map_redeem_outcome(
        http_status: reqwest::StatusCode,
        response: &SmartfrenRedeemResponse,
    ) -> (bool, Option<String>) {
        let status_failure = response
            .status
            .as_deref()
            .map(Self::is_failure_status)
            .unwrap_or(false);

        let status_success = response
            .status
            .as_deref()
            .map(Self::is_success_status)
            .unwrap_or(false);

        let explicit_success = response.success.unwrap_or(false);

        let success = http_status.is_success()
            && !status_failure
            && (explicit_success || status_success);

        let message = Self::build_redeem_message(response, success);
        (success, message)
    }

    fn extract_transaction_id(data: Option<&serde_json::Value>) -> Option<String> {
        data.and_then(|d| d.as_object())
            .and_then(|obj| {
                obj.get("transactionId")
                    .or_else(|| obj.get("transaction_id"))
                    .or_else(|| obj.get("trxId"))
                    .or_else(|| obj.get("trx_id"))
            })
            .and_then(|v| v.as_str())
            .map(String::from)
    }
    
    /// Perform handshake to get session token
    async fn handshake(&self, mdn: &str) -> Result<String, ProviderError> {
        let encrypted_mdn = Self::encrypt_payload(mdn, SECRET_KEY)?;
        
        tracing::debug!("Smartfren: Handshake with encrypted MDN");
        
        let url = format!("{}/hand-shake", BASE_URL);
        let resp = self.client.post(&url)
            .headers(default_headers(mdn))
            .json(&serde_json::json!({ "mdn": encrypted_mdn }))
            .send()
            .await?;
        
        let handshake_resp: HandshakeResponse = resp.json().await
            .map_err(|e| ProviderError::InvalidResponse(format!("Failed to parse handshake: {}", e)))?;
        
        if !handshake_resp.success {
            return Err(ProviderError::AuthError(
                handshake_resp.message.unwrap_or_else(|| "Handshake failed".to_string())
            ));
        }
        
        handshake_resp.data
            .and_then(|d| d.token)
            .ok_or_else(|| ProviderError::AuthError("No token in handshake response".to_string()))
    }
}

impl Default for SmartfrenProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderApi for SmartfrenProvider {
    fn name(&self) -> &'static str {
        "Smartfren"
    }
    
    async fn check_voucher(&self, barcode: &str) -> Result<CheckResponse, ProviderError> {
        // Smartfren doesn't have a public check API based on the JS code
        // Return not implemented
        tracing::warn!("Smartfren: check_voucher not available (no public API)");
        
        Ok(CheckResponse {
            success: false,
            serial_number: barcode.to_string(),
            product_name: None,
            nominal: None,
            expiry_date: None,
            status: "NOT_AVAILABLE".to_string(),
            raw_response: None,
        })
    }
    
    async fn redeem_voucher(&self, msisdn: &str, serial_number: &str) -> Result<RedeemResponse, ProviderError> {
        tracing::info!("Smartfren: Redeeming voucher {} for {}", serial_number, msisdn);
        
        // Step 1: Handshake to get session token
        let session_token = self.handshake(msisdn).await?;
        tracing::debug!("Smartfren: Got session token");
        
        // Step 2: Encrypt voucher code and session token
        let encrypted_voucher = Self::encrypt_payload(serial_number, SECRET_KEY)?;
        let encrypted_session = Self::encrypt_payload(&session_token, SECRET_KEY)?;
        
        // Step 3: Send redeem request
        let url = format!("{}/voucher-code", BASE_URL);
        let resp = self.client.post(&url)
            .headers(default_headers(msisdn))
            .json(&serde_json::json!({
                "voucherCode": encrypted_voucher,
                "sessionID": encrypted_session
            }))
            .send()
            .await?;
        
        let status_code = resp.status();
        let raw_text = resp.text().await?;
        
        tracing::info!("Smartfren redeem response: {}", raw_text);
        tracing::debug!("Smartfren redeem response: {}", raw_text);
        
        let redeem_resp: SmartfrenRedeemResponse = serde_json::from_str(&raw_text)
            .map_err(|e| ProviderError::InvalidResponse(format!("Failed to parse response: {}", e)))?;
        
        let raw_json: Option<serde_json::Value> = serde_json::from_str(&raw_text).ok();
        let (success, message) = Self::map_redeem_outcome(status_code, &redeem_resp);
        
        Ok(RedeemResponse {
            success,
            msisdn: msisdn.to_string(),
            serial_number: serial_number.to_string(),
            message,
            transaction_id: Self::extract_transaction_id(redeem_resp.data.as_ref()),
            raw_response: raw_json,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_evp_bytes_to_key() {
        let password = b"testpassword";
        let salt = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let (key, iv) = SmartfrenProvider::evp_bytes_to_key(password, &salt, 32, 16);
        
        assert_eq!(key.len(), 32);
        assert_eq!(iv.len(), 16);
    }
    
    #[test]
    fn test_encrypt_payload() {
        let result = SmartfrenProvider::encrypt_payload("test", SECRET_KEY);
        assert!(result.is_ok());
        
        let encrypted = result.unwrap();
        // Should be base64 and start with "Salted__" when decoded
        let decoded = BASE64.decode(&encrypted).unwrap();
        assert!(decoded.starts_with(b"Salted__"));
    }

    #[test]
    fn test_evp_bytes_to_key_matches_openssl_md5_vector() {
        // Expected values generated from:
        // openssl enc -aes-256-cbc -md md5 -pass pass:'<SECRET_KEY>' -S 0102030405060708 -P
        let salt = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let (key, iv) = SmartfrenProvider::evp_bytes_to_key(SECRET_KEY.as_bytes(), &salt, 32, 16);

        let expected_key = vec![
            0x73, 0xA2, 0x65, 0xBE, 0xCD, 0x10, 0x85, 0x4E,
            0x59, 0xC1, 0xF6, 0xDE, 0x8A, 0x66, 0x41, 0xB2,
            0x4A, 0xC3, 0x8D, 0x70, 0x63, 0x48, 0xB7, 0x6F,
            0xCC, 0x8D, 0xF5, 0x74, 0x1C, 0x59, 0x09, 0x8D,
        ];
        let expected_iv = vec![
            0x2B, 0x70, 0x73, 0x99, 0xD5, 0xC0, 0x70, 0xC6,
            0xE0, 0xB8, 0x5D, 0x17, 0x6A, 0x93, 0xBA, 0xCA,
        ];

        assert_eq!(key, expected_key);
        assert_eq!(iv, expected_iv);
    }

    #[test]
    fn test_encrypt_payload_matches_openssl_vector_with_fixed_salt() {
        let salt = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let (key, _iv) = SmartfrenProvider::evp_bytes_to_key(SECRET_KEY.as_bytes(), &salt, 32, 16);
        let encrypted = SmartfrenProvider::aes_256_ecb_encrypt(b"test", &key).unwrap();

        let mut result = Vec::with_capacity(8 + 8 + encrypted.len());
        result.extend_from_slice(b"Salted__");
        result.extend_from_slice(&salt);
        result.extend_from_slice(&encrypted);

        assert_eq!(
            BASE64.encode(&result),
            "U2FsdGVkX18BAgMEBQYHCG7upTJmMvw72eflrVQZFPY="
        );
    }

    #[tokio::test]
    async fn test_redeem_voucher_live_opt_in() {
        if std::env::var("SMARTFREN_RUN_LIVE_TEST").ok().as_deref() != Some("1") {
            return;
        }

        let msisdn = std::env::var("SMARTFREN_TEST_MDN")
            .expect("SMARTFREN_TEST_MDN must be set when SMARTFREN_RUN_LIVE_TEST=1");
        let voucher = std::env::var("SMARTFREN_TEST_VOUCHER")
            .expect("SMARTFREN_TEST_VOUCHER must be set when SMARTFREN_RUN_LIVE_TEST=1");

        let provider = SmartfrenProvider::new();
        let result = provider.redeem_voucher(&msisdn, &voucher).await;

        assert!(
            result.is_ok(),
            "redeem request failed at transport/auth layer: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_deserialize_redeem_failure_with_status_msg_and_string_data() {
        let json_str = r#"{
            "status":"-1",
            "msg":"Maaf, topup Anda gagal. Silahkan periksa kembali PIN voucher Anda atau hub customer service kami",
            "data":"Maaf, topup Anda gagal. Silahkan periksa kembali PIN voucher Anda atau hub customer service kami"
        }"#;

        let resp: SmartfrenRedeemResponse = serde_json::from_str(json_str).unwrap();

        assert_eq!(resp.status.as_deref(), Some("-1"));
        assert_eq!(
            resp.msg.as_deref(),
            Some("Maaf, topup Anda gagal. Silahkan periksa kembali PIN voucher Anda atau hub customer service kami")
        );
        assert_eq!(
            resp.data.as_ref().and_then(|v| v.as_str()),
            Some("Maaf, topup Anda gagal. Silahkan periksa kembali PIN voucher Anda atau hub customer service kami")
        );
    }

    #[test]
    fn test_map_redeem_outcome_status_minus_one_returns_clear_message() {
        let response = SmartfrenRedeemResponse {
            success: None,
            status: Some("-1".to_string()),
            message: None,
            msg: Some(
                "Maaf, topup Anda gagal. Silahkan periksa kembali PIN voucher Anda atau hub customer service kami"
                    .to_string(),
            ),
            data: Some(serde_json::Value::String(
                "Maaf, topup Anda gagal. Silahkan periksa kembali PIN voucher Anda atau hub customer service kami"
                    .to_string(),
            )),
        };

        let (success, message) =
            SmartfrenProvider::map_redeem_outcome(reqwest::StatusCode::OK, &response);

        assert!(!success);
        let msg = message.unwrap();
        assert!(msg.contains("PIN voucher kemungkinan salah/tidak valid"));
        assert!(msg.contains("status -1"));
    }

    #[test]
    fn test_map_redeem_outcome_status_one_marks_success() {
        let response = SmartfrenRedeemResponse {
            success: None,
            status: Some("1".to_string()),
            message: None,
            msg: Some("Topup berhasil".to_string()),
            data: Some(serde_json::json!({
                "transactionId": "TRX123456"
            })),
        };

        let (success, message) =
            SmartfrenProvider::map_redeem_outcome(reqwest::StatusCode::OK, &response);

        assert!(success);
        assert_eq!(message.as_deref(), Some("Topup berhasil"));
        assert_eq!(
            SmartfrenProvider::extract_transaction_id(response.data.as_ref()).as_deref(),
            Some("TRX123456")
        );
    }
}

