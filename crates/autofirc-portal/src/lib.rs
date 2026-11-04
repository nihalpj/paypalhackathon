//! autofirc-portal: KERNEL browser-agent fallback for portals without an API.
//!
//! The primary reconciliation path talks to the DGFT Open API directly
//! (`autofirc-dgft`). Some steps in the real export-compliance world still
//! live behind browser-only portals (legacy bank FIRA uploads, ICEGATE
//! lookups). This crate is the seam for those: a thin client over KERNEL's
//! hosted browser sessions that a worker can hand a portal playbook to.
//!
//! Status: scaffold. The interface below is what the reconcile worker will
//! call once a portal step needs browser automation; `PortalAgent::from_env`
//! returns `None` unless `KERNEL_API_KEY` is set, and every caller treats
//! the browser path as optional.

/// A browser-agent session against KERNEL's hosted browsers.
#[derive(Debug, Clone)]
pub struct PortalAgent {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

/// Outcome of one automated portal run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PortalRun {
    pub session_id: String,
    pub status: String,
    /// Extracted artifacts, e.g. a downloaded FIRA PDF URL or a portal
    /// acknowledgement number.
    pub artifacts: Vec<String>,
}

impl PortalAgent {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("KERNEL_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())?;
        Some(Self {
            http: reqwest::Client::new(),
            base_url: std::env::var("KERNEL_BASE_URL")
                .unwrap_or_else(|_| "https://api.kernel.sh".into()),
            api_key,
        })
    }

    /// Launch a browser session with a natural-language playbook.
    /// Not yet wired into the reconcile worker — the DGFT API path covers
    /// the demo. Kept deliberately small: one call in, one run out.
    pub async fn run_playbook(&self, playbook: &str) -> anyhow::Result<PortalRun> {
        let resp = self
            .http
            .post(format!("{}/v1/sessions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({ "playbook": playbook }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_without_api_key() {
        // KERNEL_API_KEY is not set in the test environment.
        assert!(PortalAgent::from_env().is_none());
    }
}
