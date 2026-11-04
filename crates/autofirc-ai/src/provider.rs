use crate::classify::Classifier;
use anyhow::Context;
use rig::completion::CompletionRequest;
use std::sync::Arc;

pub type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// Build a [`Classifier`] from environment configuration.
///
/// - `AI_PROVIDER`: `gemini` (default) | `openai` | `anthropic` | `openrouter` | `groq`
/// - `AI_MODEL`: provider-specific model id (defaults per provider below)
/// - The provider's API key env var (`GEMINI_API_KEY`, `OPENAI_API_KEY`, ...)
pub fn classifier_from_env() -> anyhow::Result<Classifier> {
    let provider = std::env::var("AI_PROVIDER").unwrap_or_else(|_| "gemini".into());
    let model_id = std::env::var("AI_MODEL").ok();
    match provider.as_str() {
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
