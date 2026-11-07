//! autofirc-portal: kernel.sh cloud-browser client for portal automation.
//!
//! Drives browser sessions through Kernel's REST API: create a headful
//! browser (live view + replay), execute a deterministic Playwright
//! playbook server-side via `/browsers/{id}/playwright/execute`, then
//! delete the browser. Used by the reconcile worker to file eBRCs through
//! the DGFT portal UI (`autofirc-dgft`'s mock portal in the demo) when the
//! JSON API path is unavailable or `DGFT_PORTAL_MODE=browser` is set.
//!
//! `PortalAgent::from_env` returns `None` unless `KERNEL_API_KEY` is set;
//! every caller treats the browser path as optional.

use anyhow::Context;

pub type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// A browser-agent client against kernel.sh's hosted browsers.
#[derive(Debug, Clone)]
pub struct PortalAgent {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

/// Everything the portal filing needs, carried from the ledger row.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EbrcFiling {
    pub iec: String,
    pub irm_numbers: Vec<String>,
    pub invoice_number: String,
    pub invoice_date: String,
    pub invoice_value_inr: String,
    pub gstin: String,
    pub sac_code: String,
    pub mode_of_export: u8,
    pub purpose_code: String,
    /// Signed download URLs rendered by PdfStore; the playbook downloads
    /// them inside Kernel's infra and attaches them to the file inputs.
    pub invoice_pdf_url: String,
    pub lut_pdf_url: String,
}

/// Outcome of one automated portal run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PortalRun {
    pub session_id: String,
    pub status: String,
    /// Extracted artifacts: the eBRC number, plus any extra scraped values.
    pub artifacts: Vec<String>,
    /// Kernel live-view/replay URL for the session, when headful.
    pub live_view_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct CreateBrowserResponse {
    session_id: String,
    #[serde(default)]
    browser_live_view_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ExecuteResponse {
    #[serde(default)]
    result: serde_json::Value,
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

    async fn create_browser(&self) -> anyhow::Result<CreateBrowserResponse> {
        let resp = self
            .http
            .post(format!("{}/browsers", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({ "headless": false }))
            .send()
            .await?
            .error_for_status()
            .context("kernel: create browser")?;
        Ok(resp.json().await?)
    }

    async fn execute(&self, session_id: &str, code: &str) -> anyhow::Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!(
                "{}/browsers/{}/playwright/execute",
                self.base_url, session_id
            ))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({ "code": code }))
            .send()
            .await?
            .error_for_status()
            .context("kernel: playwright execute")?;
        let out: ExecuteResponse = resp.json().await?;
        Ok(out.result)
    }

    async fn delete_browser(&self, session_id: &str) {
        let _ = self
            .http
            .delete(format!("{}/browsers/{}", self.base_url, session_id))
            .bearer_auth(&self.api_key)
            .send()
            .await;
    }

    /// File an eBRC through a DGFT-style portal UI in a cloud browser.
    /// Always tears the browser down, success or failure.
    pub async fn file_ebrc_via_portal(
        &self,
        portal_url: &str,
        filing: &EbrcFiling,
    ) -> anyhow::Result<PortalRun> {
        let browser = self.create_browser().await?;
        let session_id = browser.session_id.clone();
        let live_view_url = browser.browser_live_view_url.clone();
        let code = render_playbook(portal_url, filing);
        let result = self.execute(&session_id, &code).await;
        self.delete_browser(&session_id).await;

        let result = result?;
        let text = result
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| result.to_string());
        let ebrc = extract_ebrc(&text)
            .with_context(|| format!("no eBRC number in portal result: {text}"))?;
        Ok(PortalRun {
            session_id,
            status: "ISSUED".into(),
            artifacts: vec![ebrc],
            live_view_url,
        })
    }
}

/// Playwright JS executed server-side inside the kernel browser. Data is
/// injected as a JSON literal; the flow mirrors the portal pages in
/// `autofirc-dgft::portal` (login → IRM clubbing → certify → scrape eBRC).
fn render_playbook(portal_url: &str, filing: &EbrcFiling) -> String {
    let data = serde_json::json!({
        "portal": portal_url.trim_end_matches('/'),
        "filing": filing,
    });
    format!(
        r#"const D = {data};
const base = D.portal;
// 1. Login
await page.goto(base + '/portal', {{ waitUntil: 'domcontentloaded' }});
await page.fill('#iec', D.filing.iec);
await page.fill('#password', 'autofirc-demo');
await Promise.all([
  page.waitForURL('**/portal/dashboard**'),
  page.click('#login-submit'),
]);
// 2. IRM repository: tick the clubbing checkboxes and proceed
await page.goto(base + '/portal/irms', {{ waitUntil: 'domcontentloaded' }});
for (const irm of D.filing.irm_numbers) {{
  await page.check(`input[name="irm"][value="${{irm}}"]`);
}}
await Promise.all([
  page.waitForURL('**/portal/certify**'),
  page.click('#proceed-certify'),
]);
// 3. Download the compliance PDFs and attach them to the file inputs
const fs = await import('fs');
async function download(url, dest) {{
  const resp = await fetch(url);
  if (!resp.ok) throw new Error('download failed: ' + resp.status);
  fs.writeFileSync(dest, Buffer.from(await resp.arrayBuffer()));
}}
await download(D.filing.invoice_pdf_url, '/tmp/invoice.pdf');
await download(D.filing.lut_pdf_url, '/tmp/lut.pdf');
// 4. Fill the self-certification form
await page.fill('#invoice_number', D.filing.invoice_number);
await page.fill('#invoice_date', D.filing.invoice_date);
await page.fill('#invoice_value_inr', D.filing.invoice_value_inr);
await page.fill('#gstin', D.filing.gstin);
await page.fill('#sac_code', D.filing.sac_code);
await page.selectOption('#mode_of_export', String(D.filing.mode_of_export));
await page.fill('#purpose_code', D.filing.purpose_code);
await page.setInputFiles('#invoice_pdf', '/tmp/invoice.pdf');
await page.setInputFiles('#lut_pdf', '/tmp/lut.pdf');
await page.check('#declaration');
// 5. Submit and scrape the issued eBRC number
await Promise.all([
  page.waitForSelector('#ebrc-number', {{ timeout: 15000 }}),
  page.click('#submit-ebrc'),
]);
return await page.textContent('#ebrc-number');
"#,
        data = data
    )
}

/// Pull `EBRC-YYYY-NNNN` out of scraped page text.
pub fn extract_ebrc(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 6 <= bytes.len() {
        if bytes[i..].starts_with(b"EBRC-") {
            let mut end = i + 5;
            while end < bytes.len()
                && (bytes[end].is_ascii_digit() || bytes[end] == b'-')
                && end - i < 14
            {
                end += 1;
            }
            let candidate = &text[i..end];
            // Expect EBRC-2026-0001 shape: two dash-separated groups after prefix.
            if candidate.matches('-').count() == 2 && candidate.len() >= 12 {
                return Some(candidate.to_string());
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn disabled_without_api_key() {
        // KERNEL_API_KEY is not set in the test environment.
        assert!(PortalAgent::from_env().is_none());
    }

    #[test]
    fn extract_ebrc_from_scraped_text() {
        assert_eq!(
            extract_ebrc("EBRC-2026-0007"),
            Some("EBRC-2026-0007".to_string())
        );
        assert_eq!(
            extract_ebrc("Certificate issued: EBRC-2026-0042. Forward to your bank."),
            Some("EBRC-2026-0042".to_string())
        );
        assert_eq!(extract_ebrc("no certificate here"), None);
    }

    fn filing() -> EbrcFiling {
        EbrcFiling {
            iec: "0123456789".into(),
            irm_numbers: vec!["IRM-1".into()],
            invoice_number: "INV/2026-27/0001".into(),
            invoice_date: "2026-11-06".into(),
            invoice_value_inr: "41500.00".into(),
            gstin: "29ABCDE1234F1Z5".into(),
            sac_code: "998314".into(),
            mode_of_export: 1,
            purpose_code: "P0802".into(),
            invoice_pdf_url: "https://example.test/invoice.pdf".into(),
            lut_pdf_url: "https://example.test/lut.pdf".into(),
        }
    }

    #[test]
    fn playbook_injects_portal_url_and_filing() {
        let js = render_playbook("https://dgft.example.test/", &filing());
        assert!(js.contains(r#""portal":"https://dgft.example.test""#));
        assert!(js.contains("base + '/portal'"));
        assert!(js.contains("IRM-1"));
        assert!(js.contains("INV/2026-27/0001"));
        assert!(js.contains("setInputFiles"));
        assert!(js.contains("#ebrc-number"));
    }

    #[tokio::test]
    async fn filing_orchestrates_create_execute_delete() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "session_id": "sess-1",
                "browser_live_view_url": "https://live.kernel.test/sess-1"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/browsers/sess-1/playwright/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": "EBRC-2026-0001"
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/browsers/sess-1"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let agent = PortalAgent {
            http: reqwest::Client::new(),
            base_url: server.uri(),
            api_key: "test-key".into(),
        };
        let run = agent
            .file_ebrc_via_portal("https://dgft.example.test", &filing())
            .await
            .unwrap();
        assert_eq!(run.status, "ISSUED");
        assert_eq!(run.artifacts, vec!["EBRC-2026-0001"]);
        assert_eq!(
            run.live_view_url.as_deref(),
            Some("https://live.kernel.test/sess-1")
        );
    }

    #[tokio::test]
    async fn filing_errors_when_result_has_no_ebrc() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "session_id": "sess-2"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/browsers/sess-2/playwright/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": "Filing rejected: unknown IRM"
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/browsers/sess-2"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let agent = PortalAgent {
            http: reqwest::Client::new(),
            base_url: server.uri(),
            api_key: "test-key".into(),
        };
        let err = agent
            .file_ebrc_via_portal("https://dgft.example.test", &filing())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no eBRC number"), "{err}");
    }
}
