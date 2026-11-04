//! Cross-border guard: PayPal India merchant accounts accept international
//! payments only, so domestic (India-to-India) payments must be rejected
//! before any classification or document generation happens.
use serde_json::Value;

/// Payer's country code from the enrichment payload, uppercased.
/// Reads the order's payer address first, then the capture's.
pub fn payer_country(enrichment: &Value) -> Option<String> {
    enrichment
        .pointer("/order/payer/address/country_code")
        .or_else(|| enrichment.pointer("/capture/payer/address/country_code"))
        .and_then(|v| v.as_str())
        .map(str::to_uppercase)
}

/// False only when the payer is explicitly Indian. A missing country is
/// treated as cross-border: sandbox and some live payloads omit the payer
/// address, and false rejections are worse than letting the pipeline run.
pub fn is_cross_border(enrichment: &Value) -> bool {
    payer_country(enrichment).as_deref() != Some("IN")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn indian_payer_is_domestic() {
        let e = json!({"order": {"payer": {"address": {"country_code": "IN"}}}});
        assert!(!is_cross_border(&e));
    }

    #[test]
    fn foreign_payers_are_cross_border() {
        for ccy in ["US", "GB", "DE", "CN"] {
            let e = json!({"order": {"payer": {"address": {"country_code": ccy}}}});
            assert!(is_cross_border(&e), "{ccy} should be cross-border");
        }
    }

    #[test]
    fn missing_country_defaults_to_cross_border() {
        assert!(is_cross_border(&json!({"order": {"payer": {}}})));
        assert!(is_cross_border(&json!({})));
    }

    #[test]
    fn country_match_is_case_insensitive_and_falls_back_to_capture() {
        let e = json!({"capture": {"payer": {"address": {"country_code": "in"}}}});
        assert_eq!(payer_country(&e).as_deref(), Some("IN"));
        assert!(!is_cross_border(&e));
    }
}
