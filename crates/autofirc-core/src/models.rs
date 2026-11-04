use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxnStatus {
    PendingAnalysis,
    Enriched,
    ManualReview,
    CompliancePacketReady,
    PetitionAuthorized,
    BankPetitioned,
    DgftRetry,
    FullyReconciled,
}

impl TxnStatus {
    /// The only legal forward transitions of the ledger state machine.
    pub fn can_transition_to(self, next: TxnStatus) -> bool {
        use TxnStatus::*;
        matches!(
            (self, next),
            (PendingAnalysis, Enriched)
                | (PendingAnalysis, ManualReview)
                | (Enriched, ManualReview)
                | (Enriched, CompliancePacketReady)
                | (ManualReview, PendingAnalysis) // merchant re-runs classification
                | (CompliancePacketReady, PetitionAuthorized)
                | (PetitionAuthorized, BankPetitioned)
                | (BankPetitioned, FullyReconciled)
                | (BankPetitioned, DgftRetry)
                | (DgftRetry, BankPetitioned) // retry re-enters petitioned flow
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// PayPal webhook event id — primary key, enforces idempotency.
    pub id: String,
    pub capture_id: String,
    pub order_id: String,
    pub client_name: String,
    pub amount_inr_paise: i64,
    pub invoice_date: NaiveDate,
    pub inr_vostro: bool,
    pub purpose_code: Option<crate::purpose::PurposeCode>,
    pub mode_of_export: Option<u8>,
    pub sac_code: Option<String>,
    pub ai_confidence: Option<u8>,
    pub ebrc_number: Option<String>,
    pub status: TxnStatus,
    pub enrichment: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_machine_allows_only_legal_transitions() {
        assert!(TxnStatus::PendingAnalysis.can_transition_to(TxnStatus::Enriched));
        assert!(TxnStatus::DgftRetry.can_transition_to(TxnStatus::BankPetitioned));
        assert!(!TxnStatus::PendingAnalysis.can_transition_to(TxnStatus::FullyReconciled));
        assert!(!TxnStatus::FullyReconciled.can_transition_to(TxnStatus::PendingAnalysis));
        assert!(!TxnStatus::ManualReview.can_transition_to(TxnStatus::CompliancePacketReady));
    }
}
