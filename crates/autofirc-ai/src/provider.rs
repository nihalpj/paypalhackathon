use crate::classify::Classifier;
use anyhow::Context;
use rig::completion::CompletionRequest;
use std::sync::Arc;

pub type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// Build a [`Classifier`] from environment configuration.
///
/// - `AI_PROVIDER`: `gemini` (default) | `nebius` | `openai` | `anthropic` | `openrouter` | `groq` | `offline`
/// - `AI_MODEL`: provider-specific model id (defaults per provider below)
/// - The provider's API key env var (`GEMINI_API_KEY`, `NEBIUS_API_KEY`, ...)
pub fn classifier_from_env() -> anyhow::Result<Classifier> {
    let provider = std::env::var("AI_PROVIDER").unwrap_or_else(|_| "gemini".into());
    let model_id = std::env::var("AI_MODEL").ok();
    match provider.as_str() {
        "nebius" => {
            // Nebius Token Factory: plain OpenAI-style chat completions,
            // called directly (rig 0.46's OpenAI dialect prefers the
            // Responses API, which Token Factory does not serve).
            let api_key = std::env::var("NEBIUS_API_KEY").context("NEBIUS_API_KEY")?;
            let base_url = std::env::var("NEBIUS_BASE_URL").unwrap_or_else(|_| {
                "https://api.tokenfactory.eu-north1.nebius.com/v1".into()
            });
            let model = model_id
                .unwrap_or_else(|| "nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B".into());
            let http = reqwest::Client::new();
            Ok(Classifier::new(Arc::new(move |prompt, _schema| {
                let http = http.clone();
                let api_key = api_key.clone();
                let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
                let model = model.clone();
                Box::pin(async move {
                    let resp = http
                        .post(url)
                        .bearer_auth(api_key)
                        .json(&serde_json::json!({
                            "model": model,
                            // temperature 0: classification must be
                            // deterministic — identical transactions must
                            // classify identically.
                            "temperature": 0.0,
                            "messages": [{"role": "user", "content": prompt}],
                        }))
                        .send()
                        .await?
                        .error_for_status()?;
                    let body: serde_json::Value = resp.json().await?;
                    let text = body["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or_default();
                    Ok(strip_code_fences(text))
                })
            })))
        }
        "openai" => {
            use rig::providers::openai;
            let model = openai::OpenAI::from_env()
                .context("OPENAI_API_KEY")?
                .completion(model_id.unwrap_or_else(|| "gpt-4o".into()));
            Ok(Classifier::new(Arc::new(move |prompt, schema| {
                let model = model.clone();
                Box::pin(async move {
                    let req = CompletionRequest::new(prompt)
                        .output_schema(Some(schema))
                        .temperature(0.1);
                    Ok(model.call(req).await?.text())
                })
            })))
        }
        "anthropic" => {
            use rig::providers::anthropic;
            let model = anthropic::Anthropic::from_env()
                .context("ANTHROPIC_API_KEY")?
                .completion(model_id.unwrap_or_else(|| "claude-sonnet-4-5".into()));
            Ok(Classifier::new(Arc::new(move |prompt, schema| {
                let model = model.clone();
                Box::pin(async move {
                    let req = CompletionRequest::new(prompt)
                        .output_schema(Some(schema))
                        .temperature(0.1);
                    Ok(model.call(req).await?.text())
                })
            })))
        }
        // offline — deterministic, network-free classifier for demos and
        // CI: inspects the prompt and emits valid guardrail-shaped JSON.
        "offline" => Ok(Classifier::new(Arc::new(|prompt, _schema| {
            Box::pin(async move { Ok(offline_complete(&prompt)) })
        }))),
        // gemini (default) — also the branch for openrouter/groq-style setups:
        // point OPENAI-compatible gateways at AI_PROVIDER=openai + their envs.
        _ => {
            use rig::providers::gemini;
            let model = gemini::Gemini::from_env()
                .context("GEMINI_API_KEY")?
                .completion(model_id.unwrap_or_else(|| "gemini-2.5-flash".into()));
            Ok(Classifier::new(Arc::new(move |prompt, schema| {
                let model = model.clone();
                Box::pin(async move {
                    let req = CompletionRequest::new(prompt)
                        .output_schema(Some(schema))
                        .temperature(0.1);
                    Ok(model.call(req).await?.text())
                })
            })))
        }
    }
}

/// Models sometimes wrap JSON in markdown fences; `validate` needs bare JSON.
fn strip_code_fences(s: &str) -> String {
    let t = s.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim().to_owned()
}

/// Deterministic offline completion: valid structured output derived from
/// prompt keywords, so the full pipeline (classification, backstop, query
/// parser) runs in demos/CI with no LLM key and no network.
fn offline_complete(prompt: &str) -> String {
    // NL ledger query path: echo the question back as a text filter.
    if prompt.contains("natural-language ledger query") {
        let q = prompt.split("QUERY:\n").nth(1).unwrap_or("").trim();
        return serde_json::json!({"text": q}).to_string();
    }
    let hay = prompt.to_lowercase();
    if hay.contains("shipping_address") {
        serde_json::json!({
            "purpose_code": "P0103", "mode_of_export": 1, "sac_code": "0",
            "confidence_score": 96,
            "reasoning": "offline demo: physical-goods signals present; advance payment for goods"
        })
    } else if hay.contains("general business services") || hay.contains("scope tbd") {
        serde_json::json!({
            "purpose_code": "P1006", "mode_of_export": 1, "sac_code": "998311",
            "confidence_score": 70,
            "reasoning": "offline demo: ambiguous scope; below confidence threshold"
        })
    } else if hay.contains("design") {
        serde_json::json!({
            "purpose_code": "P0802", "mode_of_export": 1, "sac_code": "998391",
            "confidence_score": 95,
            "reasoning": "offline demo: design services export"
        })
    } else {
        serde_json::json!({
            "purpose_code": "P0802", "mode_of_export": 1, "sac_code": "998314",
            "confidence_score": 97,
            "reasoning": "offline demo: software/consulting services export"
        })
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offline_classifier() -> Classifier {
        Classifier::new(Arc::new(|prompt, _schema| {
            Box::pin(async move { Ok(offline_complete(&prompt)) })
        }))
    }

    #[tokio::test]
    async fn offline_provider_classifies_services() {
        let out = offline_classifier()
            .classify(&serde_json::json!({"order": {"purchase_units": [{"description": "Remote software consulting"}]}}))
            .await
            .unwrap();
        assert!(!out.needs_review);
        assert_eq!(out.classification.purpose_code, "P0802");
    }

    #[tokio::test]
    async fn offline_provider_flags_ambiguous_scope() {
        let out = offline_classifier()
            .classify(&serde_json::json!({"order": {"purchase_units": [{"description": "General business services retainer (scope TBD)"}]}}))
            .await
            .unwrap();
        assert!(out.needs_review); // 70 < 85 threshold
    }

    #[test]
    fn strip_code_fences_unwraps_markdown() {
        assert_eq!(strip_code_fences("{\"a\":1}"), "{\"a\":1}");
        assert_eq!(strip_code_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
    }

    #[tokio::test]
    async fn nebius_branch_posts_chat_completion() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "```json\n{\"purpose_code\":\"P0802\",\"mode_of_export\":1,\"sac_code\":\"998314\",\"confidence_score\":95,\"reasoning\":\"software consulting\"}\n```"}}]
            })))
            .mount(&server)
            .await;
        unsafe {
            std::env::set_var("AI_PROVIDER", "nebius");
            std::env::set_var("NEBIUS_API_KEY", "test-key");
            std::env::set_var("NEBIUS_BASE_URL", server.uri());
        }
        let classifier = classifier_from_env().unwrap();
        let out = classifier
            .classify(&serde_json::json!({"order": {"purchase_units": [{"description": "Remote software consulting"}]}}))
            .await
            .unwrap();
        assert!(!out.needs_review);
        assert_eq!(out.classification.purpose_code, "P0802");
        unsafe {
            std::env::remove_var("AI_PROVIDER");
            std::env::remove_var("NEBIUS_API_KEY");
            std::env::remove_var("NEBIUS_BASE_URL");
        }
    }

    #[test]
    fn offline_query_echoes_text_filter() {
        let out = offline_complete(
            "You convert a merchant's natural-language ledger query into a JSON filter.\nQUERY:\nfind globex",
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["text"], "find globex");
    }
}
