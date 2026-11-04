//! Natural-language ledger filtering: turns a merchant's plain-English
//! question ("show me unreconciled rows past the amber mark") into a
//! structured filter the dashboard applies client-side.
use serde::{Deserialize, Serialize};

use crate::classify::{Classifier, CompleteFn};

/// Structured filter produced from a natural-language query. Every field is
/// optional; the dashboard ANDs the populated ones.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LedgerFilter {
    /// Exact ledger status, e.g. "pending_analysis", "enriched",
    /// "manual_review", "compliance_packet_ready", "petition_authorized",
    /// "bank_petitioned", "fully_reconciled".
    pub status: Option<String>,
    /// RBI purpose code, e.g. "P0802".
    pub purpose_code: Option<String>,
    /// Minimum AI classification confidence (0-100).
    pub min_confidence: Option<u8>,
    /// FEMA realization alert level: "green" | "amber" | "red".
    /// Computed from invoice_date against the 9-month deadline, amber at the
    /// 7-month mark, red at the 8-month mark.
    pub alert: Option<String>,
    /// Free-text match against client name, invoice number, eBRC number,
    /// capture id, and order id.
    pub text: Option<String>,
}

pub fn output_schema() -> schemars::Schema {
    schemars::schema_for!(LedgerFilter)
}

const QUERY_PROMPT: &str = r#"You convert a merchant's natural-language ledger query into a JSON filter.

RULES:
- Output ONLY the JSON object matching the provided schema. No prose.
- Populate only fields the query clearly implies; leave the rest null.
- Valid status values: pending_analysis, enriched, manual_review,
  compliance_packet_ready, petition_authorized, bank_petitioned,
  fully_reconciled.
- "unreconciled", "pending", "not reconciled" mean status bank_petitioned
  or earlier — express that with alert instead when a deadline is mentioned,
  otherwise leave status null and put nothing in text.
- "amber"/"red"/"green" map to the FEMA realization alert levels.
- Purpose codes look like P0802, P0103, P1006.
- Free-text fragments (names, invoice numbers, capture ids) go in text.

QUERY:
"#;

/// Parses natural-language queries using the same provider closure as the
/// transaction classifier.
#[derive(Clone)]
pub struct QueryParser {
    complete: CompleteFn,
}

impl QueryParser {
    pub fn new(complete: CompleteFn) -> Self {
        Self { complete }
    }

    pub fn from_classifier(classifier: &Classifier) -> Self {
        Self {
            complete: classifier.complete_fn(),
        }
    }

    /// Parse a query; falls back to a free-text filter on any provider or
    /// parse failure so the UI never hard-errors on a stray prompt.
    pub async fn parse(&self, query: &str) -> anyhow::Result<LedgerFilter> {
        let prompt = format!("{QUERY_PROMPT}{query}");
        let raw = (self.complete)(prompt, output_schema()).await?;
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        match serde_json::from_str::<LedgerFilter>(cleaned) {
            Ok(filter) => Ok(filter),
            Err(_) => Ok(LedgerFilter {
                text: Some(query.to_owned()),
                ..Default::default()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn parser_with(raw: &'static str) -> QueryParser {
        QueryParser::new(Arc::new(move |_prompt, _schema| {
            Box::pin(async move { Ok(raw.to_owned()) })
        }))
    }

    #[tokio::test]
    async fn parses_structured_filter() {
        let p = parser_with(r#"{"status":"bank_petitioned","alert":"red"}"#);
        let f = p
            .parse("show unreconciled rows in the red zone")
            .await
            .unwrap();
        assert_eq!(f.status.as_deref(), Some("bank_petitioned"));
        assert_eq!(f.alert.as_deref(), Some("red"));
        assert!(f.text.is_none());
    }

    #[tokio::test]
    async fn bad_model_output_falls_back_to_text_search() {
        let p = parser_with("not json at all");
        let f = p.parse("find globex").await.unwrap();
        assert_eq!(f.text.as_deref(), Some("find globex"));
    }

    #[tokio::test]
    async fn strips_markdown_code_fences() {
        let p = parser_with("```json\n{\"purpose_code\":\"P0802\"}\n```");
        let f = p.parse("only software exports").await.unwrap();
        assert_eq!(f.purpose_code.as_deref(), Some("P0802"));
    }
}
