use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRequest {
    pub api_key: String,
    pub client_id: String,
    pub client_secret_salt: String,
    pub client_secret_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: u64,
}

/// An Inward Remittance Message as uploaded by the AD bank.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrmDto {
    pub irm_number: String,
    pub bank_name: String,
    pub account_number: String,
    pub currency: String,
    pub amount: String,
    pub remittance_date: String,
    pub remitter_name: String,
}

/// eBRC self-certification payload (subset of the DGFT schema, including the
/// fields mandatory since 2025/2026: mode of export + GST invoice details).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EbrcRequest {
    pub iec: String,
    pub irm_numbers: Vec<String>,
    pub invoice_number: String,
    pub invoice_date: String,
    pub invoice_value_inr: String,
    pub gstin: String,
    pub sac_code: String,
    pub mode_of_export: u8,
    pub purpose_code: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EbrcResponse {
    pub ebrc_number: String,
    pub status: String,
}
