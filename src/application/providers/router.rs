//! Provider Router - Routes to correct provider based on name
//! 
//! Selects the appropriate provider implementation based on provider name.

use super::{ByuProvider, SmartfrenProvider, TelkomselProvider, ProviderApi, CheckResponse, RedeemResponse, ProviderError};
use std::sync::Arc;

/// Provider router - selects correct provider based on name
pub struct ProviderRouter {
    byu: Arc<ByuProvider>,
    smartfren: Arc<SmartfrenProvider>,
    telkomsel: Arc<TelkomselProvider>,
}

impl ProviderRouter {
    /// Create new provider router with all provider instances
    pub fn new() -> Self {
        Self {
            byu: Arc::new(ByuProvider::new()),
            smartfren: Arc::new(SmartfrenProvider::new()),
            telkomsel: Arc::new(TelkomselProvider::new()),
        }
    }
    
    /// Get provider by name (case-insensitive)
    pub fn get_provider(&self, provider_name: &str) -> Result<Arc<dyn ProviderApi>, ProviderError> {
        let name = provider_name.to_uppercase();
        
        match name.as_str() {
            "BYU" | "BY.U" => Ok(self.byu.clone()),
            "SMARTFREN" | "SMARTFREN_CEK_VOUCHER" => Ok(self.smartfren.clone()),
            "TELKOMSEL" | "TELKOMSEL_CEK_VOUCHER" => Ok(self.telkomsel.clone()),
            _ => Err(ProviderError::UnknownProvider(provider_name.to_string())),
        }
    }
    
    /// Check voucher using the correct provider
    pub async fn check_voucher(&self, provider_name: &str, barcode: &str) -> Result<CheckResponse, ProviderError> {
        let provider = self.get_provider(provider_name)?;
        provider.check_voucher(barcode).await
    }
    
    /// Redeem voucher using the correct provider
    pub async fn redeem_voucher(&self, provider_name: &str, msisdn: &str, serial_number: &str) -> Result<RedeemResponse, ProviderError> {
        let provider = self.get_provider(provider_name)?;
        provider.redeem_voucher(msisdn, serial_number).await
    }
}

impl Default for ProviderRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ProviderRouter {
    fn clone(&self) -> Self {
        Self {
            byu: self.byu.clone(),
            smartfren: self.smartfren.clone(),
            telkomsel: self.telkomsel.clone(),
        }
    }
}
