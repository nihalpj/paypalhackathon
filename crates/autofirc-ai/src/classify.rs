use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Confidence threshold below which a classification is routed to human
/// review instead of being applied.
pub const CONFIDENCE_THRESHOLD: u8 = 85;

/// Structured output the LLM must return.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Classification {
    pub purpose_code: String,
    pub mode_of_export: u8,
    pub sac_code: String,
    pub confidence_score: u8,
    pub reasoning: String,
}

/// Result of classification plus the deterministic review decision.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedTransaction {
    pub classification: Classification,
    /// True when the row must go to `Manual Review` (low confidence or a
    /// guardrail violation the deterministic backstop caught).
    pub needs_review: bool,
    /// Set when the backstop overrode/flagged the model's output.
    pub backstop_note: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ClassifyError {
    #[error("model returned invalid json: {0}")]
    InvalidJson(String),
    #[error("mode_of_export must be 1-4, got {0}")]
    BadMode(u8),
}

/// True when the enriched payload contains physical-goods signals (shipping
/// address on the order, "shipping" amounts, goods-oriented item categories).
pub fn looks_like_physical_goods(enrichment: &serde_json::Value) -> bool {
    let haystack = enrichment.to_string().to_lowercase();
    // Positive goods indicators only; "service" mentions are irrelevant here.
    haystack.contains("\"shipping_address\"")
        || haystack.contains("\"shipping\"")
        || haystack.contains("\"category\":\"physical_goods\"")
}

/// RBI FETERS purpose codes in the P01xx family are all physical-goods
/// export codes (P0101-P0109). They are statutory offences when applied to
/// service transactions.
pub fn is_goods_purpose_code(code: &str) -> bool {
    code.starts_with("P01")
}

/// Parse + validate raw model output against the deterministic guardrails.
/// This function never calls the network and is the safety net of the system:
/// even a jailbroken or hallucinating model cannot push P0103 onto a service
/// transaction through it.
pub fn validate(
    raw: &str,
    enrichment: &serde_json::Value,
) -> Result<ClassifiedTransaction, ClassifyError> {
    let classification: Classification =
        serde_json::from_str(raw).map_err(|e| ClassifyError::InvalidJson(e.to_string()))?;
    if !(1..=4).contains(&classification.mode_of_export) {
        return Err(ClassifyError::BadMode(classification.mode_of_export));
    }

    let mut needs_review = classification.confidence_score < CONFIDENCE_THRESHOLD;
    let mut backstop_note = None;

    if is_goods_purpose_code(&classification.purpose_code)
        && !looks_like_physical_goods(enrichment)
    {
        needs_review = true;
        backstop_note = Some(format!(
            "backstop: model proposed goods code {} on a transaction with no physical-goods \
             signals; routed to manual review per RBI/EDPMS guardrail",
            classification.purpose_code
        ));
    }

    Ok(ClassifiedTransaction {
        classification,
        needs_review,
        backstop_note,
    })
}

/// JSON schema handed to providers supporting native structured output.
pub fn output_schema() -> schemars::Schema {
    schemars::schema_for!(Classification)
}

pub type CompleteFn = Arc<
    dyn Fn(String, schemars::Schema) -> crate::provider::BoxFuture<anyhow::Result<String>>
        + Send
        + Sync,
>;

/// The classifier: a guardrail prompt + a provider completion closure.
#[derive(Clone)]
pub struct Classifier {
    complete: CompleteFn,
}

impl Classifier {
    pub fn new(complete: CompleteFn) -> Self {
        Self { complete }
    }

    /// The provider completion closure, for sibling engines (query parser)
    /// that share the same configured model.
    pub fn complete_fn(&self) -> CompleteFn {
        self.complete.clone()
    }

    /// Classify an enriched PayPal transaction. The system prompt carries the
    /// statutory guardrails; `validate` applies the deterministic backstop.
    pub async fn classify(
        &self,
        enrichment: &serde_json::Value,
    ) -> anyhow::Result<ClassifiedTransaction> {
        let prompt = format!(
            "{}\n\nTRANSACTION DATA:\n{}",
            crate::prompt::GUARDRAIL_PROMPT,
            serde_json::to_string_pretty(enrichment)?
        );
        let raw = (self.complete)(prompt, output_schema()).await?;
        Ok(validate(&raw, enrichment)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn service_enrichment() -> serde_json::Value {
        json!({
            "order": {"purchase_units": [{"description": "Remote software consulting, Oct 2026"}]},
            "capture": {"amount": {"currency_code": "USD", "value": "500.00"}}
        })
    }

    #[test]
    fn any_goods_code_on_services_is_caught_by_backstop() {
        // The live failure mode: identical "Remote software consulting" input
        // classified as P0802 once and P0102 another time.
        for code in ["P0101", "P0102", "P0103", "P0104", "P0108"] {
            let raw = format!(
                r#"{{"purpose_code":"{code}","mode_of_export":1,"sac_code":"998314","confidence_score":99,"reasoning":"hallucinated"}}"#
            );
            let out = validate(&raw, &service_enrichment()).unwrap();
            assert!(out.needs_review, "{code} on services must be flagged");
            assert!(
                out.backstop_note.as_ref().unwrap().contains(code),
                "backstop note must name the offending code {code}"
            );
        }
    }

    #[test]
    fn goods_code_allowed_with_physical_goods_signals() {
        let goods =
            json!({"order": {"purchase_units": [{"shipping": {}, "shipping_address": {}}]}});
        let raw = r#"{"purpose_code":"P0102","mode_of_export":1,"sac_code":"0","confidence_score":95,"reasoning":"goods export"}"#;
        let out = validate(raw, &goods).unwrap();
        assert!(!out.needs_review);
    }

    #[test]
    fn valid_service_classification_passes() {
        let raw = r#"{"purpose_code":"P0802","mode_of_export":1,"sac_code":"998314","confidence_score":97,"reasoning":"remote software consulting"}"#;
        let out = validate(raw, &service_enrichment()).unwrap();
        assert!(!out.needs_review);
        assert!(out.backstop_note.is_none());
        assert_eq!(out.classification.purpose_code, "P0802");
    }

    #[test]
    fn low_confidence_routes_to_review() {
        let raw = r#"{"purpose_code":"P1006","mode_of_export":1,"sac_code":"998311","confidence_score":60,"reasoning":"unclear scope"}"#;
        let out = validate(raw, &service_enrichment()).unwrap();
        assert!(out.needs_review);
    }

    #[test]
    fn p0103_on_services_is_caught_by_backstop() {
        let raw = r#"{"purpose_code":"P0103","mode_of_export":1,"sac_code":"998314","confidence_score":99,"reasoning":"hallucinated"}"#;
        let out = validate(raw, &service_enrichment()).unwrap();
        assert!(out.needs_review);
        assert!(out.backstop_note.unwrap().contains("P0103"));
    }

    #[test]
    fn p0103_allowed_with_physical_goods_signals() {
        let goods =
            json!({"order": {"purchase_units": [{"shipping": {}, "shipping_address": {}}]}});
        let raw = r#"{"purpose_code":"P0103","mode_of_export":1,"sac_code":"0","confidence_score":95,"reasoning":"advance for goods"}"#;
        let out = validate(raw, &goods).unwrap();
        assert!(!out.needs_review);
    }

    #[test]
    fn bad_mode_rejected_and_bad_json_errors() {
        let bad_mode = r#"{"purpose_code":"P0802","mode_of_export":7,"sac_code":"x","confidence_score":90,"reasoning":"r"}"#;
        assert!(matches!(
            validate(bad_mode, &service_enrichment()),
            Err(ClassifyError::BadMode(7))
        ));
        assert!(matches!(
            validate("not json", &service_enrichment()),
            Err(ClassifyError::InvalidJson(_))
        ));
    }

    #[test]
    fn schema_has_all_required_fields() {
        let schema = output_schema();
        let json = serde_json::to_value(&schema).unwrap();
        let text = json.to_string();
        for field in [
            "purpose_code",
            "mode_of_export",
            "sac_code",
            "confidence_score",
            "reasoning",
        ] {
            assert!(text.contains(field), "schema missing {field}");
        }
    }

    #[tokio::test]
    async fn classifier_combines_prompt_and_backstop() {
        let classifier = Classifier::new(Arc::new(|prompt, _schema| {
            assert!(prompt.contains("P0103")); // guardrail prompt reached the model
            assert!(prompt.contains("TRANSACTION DATA"));
            Box::pin(async {
                Ok(r#"{"purpose_code":"P0802","mode_of_export":1,"sac_code":"998314","confidence_score":92,"reasoning":"software consulting"}"#
                    .to_owned())
            })
        }));
        let out = classifier.classify(&service_enrichment()).await.unwrap();
        assert!(!out.needs_review);
    }
}
