//! Browser portal UI for the mock DGFT, styled after the real dgft.gov.in
//! eBRC flow: login → dashboard → IRM repository (clubbing) →
//! self-certification form with invoice/LUT upload → eBRC issued.
//! Shares `MockState` with the JSON API so both surfaces stay consistent.
use axum::{
    Form, Router,
    extract::{Multipart, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use std::sync::{Arc, Mutex};

use crate::mock::MockState;
use crate::types::IrmDto;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PortalDoc {
    pub filename: String,
    pub size: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PortalEbrc {
    pub ebrc_number: String,
    pub invoice_number: String,
    pub irm_numbers: Vec<String>,
    pub docs: Vec<PortalDoc>,
}

pub fn router(state: Arc<Mutex<MockState>>) -> Router<Arc<Mutex<MockState>>> {
    let _ = &state; // state is supplied by the parent router's .with_state()
    Router::new()
        .route("/portal", get(login_page))
        .route("/portal/login", post(login))
        .route("/portal/dashboard", get(dashboard))
        .route("/portal/irms", get(irms_page))
        .route("/portal/certify", get(certify_form).post(certify_submit))
        .route("/portal/ebrcs", get(ebrcs_page))
}

const TRICOLOR: &str =
    "background:linear-gradient(90deg,#FF9933 0%,#ffffff 50%,#138808 100%);height:4px;";

const CSS: &str = r#"
body{font-family:'Segoe UI',Arial,sans-serif;margin:0;background:#f4f6f9;color:#1c2b39}
.gov-bar{color:#000}
header{background:#003366;color:#fff;padding:14px 24px}
header .ministry{font-size:12px;opacity:.85;letter-spacing:.4px}
header h1{font-size:19px;margin:2px 0 0}
nav{background:#0b4da2;padding:8px 24px;font-size:13px}
nav a{color:#dceaff;margin-right:18px;text-decoration:none}
main{max-width:960px;margin:28px auto;padding:0 20px}
.card{background:#fff;border:1px solid #d7dee8;border-top:3px solid #003366;border-radius:6px;padding:22px;margin-bottom:18px}
label{display:block;font-size:13px;font-weight:600;margin:12px 0 4px}
input[type=text],input[type=password],input[type=date],input[type=number],select{width:100%;max-width:420px;padding:8px;border:1px solid #b9c4d1;border-radius:4px;font-size:14px}
button,.btn{background:#0b4da2;color:#fff;border:0;border-radius:4px;padding:10px 22px;font-size:14px;cursor:pointer;text-decoration:none;display:inline-block}
button:hover,.btn:hover{background:#003366}
table{width:100%;border-collapse:collapse;font-size:13.5px;background:#fff}
th{background:#003366;color:#fff;text-align:left;padding:8px 10px}
td{padding:8px 10px;border-bottom:1px solid #e2e8f0}
.mandate{font-size:12px;color:#8a6d00;background:#fff8e1;border:1px solid #ffe082;border-radius:4px;padding:8px 12px;margin-top:16px}
.tiles{display:flex;gap:16px;flex-wrap:wrap}
.tile{background:#fff;border:1px solid #d7dee8;border-left:4px solid #0b4da2;border-radius:6px;padding:18px;width:280px}
.tile h3{margin:0 0 6px;font-size:15px;color:#003366}
.tile p{font-size:13px;color:#5b6472;margin:0 0 10px}
.success{border-left:5px solid #138808;background:#f0fdf4;padding:18px;border-radius:6px}
#ebrc-number{font-size:30px;font-weight:700;color:#138808;letter-spacing:1px}
.err{border-left:5px solid #c52828;background:#fef2f2;padding:14px;border-radius:6px;color:#8a1c1c}
.meta{font-size:12.5px;color:#5b6472}
footer{margin-top:36px;padding:14px 24px;font-size:12px;color:#6b7480;border-top:1px solid #d7dee8;text-align:center}
"#;

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} · DGFT eBRC Portal</title><style>{CSS}</style></head>
<body><div class="gov-bar" style="{TRICOLOR}"></div>
<header><div class="ministry">भारत सरकार · Government of India · Ministry of Commerce &amp; Industry</div>
<h1>Directorate General of Foreign Trade — eBRC Portal</h1></header>
<nav><a href="/portal">Login</a><a href="/portal/dashboard">Dashboard</a><a href="/portal/irms">IRM Repository</a><a href="/portal/ebrcs">Issued eBRCs</a></nav>
<main>{body}</main>
<footer>eBRC self-certification is a legal declaration under FEMA 1999 and the Foreign Trade Policy. Mock environment — no real filings.</footer>
</body></html>"#
    ))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn login_page() -> Html<String> {
    page(
        "Login",
        r#"<div class="card"><h2>Exporter Login</h2>
<p class="meta">Sign in with your IEC-linked credentials to access eBRC self-certification.</p>
<form method="post" action="/portal/login">
<label for="iec">Importer Exporter Code (IEC)</label>
<input type="text" id="iec" name="iec" placeholder="0123456789" required>
<label for="password">Password</label>
<input type="password" id="password" name="password" required>
<div style="margin-top:18px"><button type="submit" id="login-submit">Sign In</button></div>
</form></div>"#,
    )
}

#[derive(serde::Deserialize)]
struct LoginForm {
    iec: String,
    password: String,
}

async fn login(Form(form): Form<LoginForm>) -> impl IntoResponse {
    if form.iec.trim().is_empty() || form.password.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            page(
                "Login failed",
                r#"<div class="err"><strong>Login failed.</strong> IEC and password are required. <a href="/portal">Try again</a>.</div>"#,
            ),
        )
            .into_response();
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        format!("dgft_session={}; Path=/; HttpOnly", esc(&form.iec))
            .parse()
            .unwrap(),
    );
    (headers, Redirect::to("/portal/dashboard")).into_response()
}

async fn dashboard() -> Html<String> {
    page(
        "Dashboard",
        r#"<div class="tiles">
<div class="tile"><h3>eBRC Issuance (Self-Certification)</h3>
<p>Match Inward Remittance Messages to export invoices and self-certify eBRC under Trade Notice 33/2023-24.</p>
<a class="btn" href="/portal/irms">Open IRM Repository</a></div>
<div class="tile"><h3>Issued eBRCs</h3>
<p>View electronic Bank Realization Certificates issued against your IEC.</p>
<a class="btn" href="/portal/ebrcs">View eBRCs</a></div>
</div>
<div class="mandate">Mandatory fields: <strong>Mode of Export of Services</strong> (w.e.f. 01.05.2025) and <strong>GST invoice details</strong> (w.e.f. 13.01.2026) must be furnished for every self-certification.</div>"#,
    )
}

async fn irms_page(State(state): State<Arc<Mutex<MockState>>>) -> Html<String> {
    let irms = state.lock().unwrap().list_irms();
    let rows = if irms.is_empty() {
        r#"<tr><td colspan="7" class="meta">No unmapped Inward Remittance Messages found for this IEC.</td></tr>"#
            .to_string()
    } else {
        irms.iter()
            .map(|i| {
                format!(
                    "<tr><td><input type=\"checkbox\" name=\"irm\" value=\"{}\"></td><td>{}</td><td>{}</td><td>{}</td><td>{} {}</td><td>{}</td><td>{}</td></tr>",
                    esc(&i.irm_number), esc(&i.irm_number), esc(&i.bank_name),
                    esc(&i.account_number), esc(&i.currency), esc(&i.amount),
                    esc(&i.remittance_date), esc(&i.remitter_name),
                )
            })
            .collect()
    };
    page(
        "IRM Repository",
        &format!(
            r#"<div class="card"><h2>Unmapped Inward Remittance Messages</h2>
<p class="meta">Select IRMs to club (same bank, account and currency only) and proceed to self-certification.</p>
<form method="get" action="/portal/certify">
<table><tr><th></th><th>IRM Number</th><th>AD Bank</th><th>Account</th><th>Amount</th><th>Remittance Date</th><th>Remitter</th></tr>
{rows}</table>
<div style="margin-top:16px"><button type="submit" id="proceed-certify">Proceed to Self-Certification</button></div>
</form></div>"#
        ),
    )
}

async fn certify_form(
    State(state): State<Arc<Mutex<MockState>>>,
    axum::extract::RawQuery(raw): axum::extract::RawQuery,
) -> impl IntoResponse {
    // Repeated checkbox params (?irm=A&irm=B) defeat serde_urlencoded; parse manually.
    let irm: Vec<String> = form_urlencoded::parse(raw.as_deref().unwrap_or("").as_bytes())
        .filter_map(|(k, v)| (k == "irm").then(|| v.into_owned()))
        .collect();
    if irm.is_empty() {
        return Redirect::to("/portal/irms").into_response();
    }
    let known: Vec<IrmDto> = {
        let s = state.lock().unwrap();
        irm.iter()
            .filter_map(|n| s.list_irms().into_iter().find(|i| &i.irm_number == n))
            .collect()
    };
    let hidden: String = irm
        .iter()
        .map(|n| format!(r#"<input type="hidden" name="irm" value="{}">"#, esc(n)))
        .collect();
    let summary: String = known
        .iter()
        .map(|i| {
            format!(
                "<tr><td>{}</td><td>{} {}</td><td>{}</td><td>{}</td></tr>",
                esc(&i.irm_number),
                esc(&i.currency),
                esc(&i.amount),
                esc(&i.remittance_date),
                esc(&i.remitter_name)
            )
        })
        .collect();
    page(
        "Self-Certification",
        &format!(
            r#"<div class="card"><h2>eBRC Self-Certification</h2>
<table><tr><th>IRM Number</th><th>Amount</th><th>Date</th><th>Remitter</th></tr>{summary}</table>
<form method="post" action="/portal/certify" enctype="multipart/form-data">
{hidden}
<label for="invoice_number">GST Invoice Number *</label>
<input type="text" id="invoice_number" name="invoice_number" placeholder="INV/2026-27/0001" required>
<label for="invoice_date">Invoice Date *</label>
<input type="date" id="invoice_date" name="invoice_date" required>
<label for="invoice_value_inr">Invoice Value (INR) *</label>
<input type="text" id="invoice_value_inr" name="invoice_value_inr" placeholder="41500.00" required>
<label for="gstin">GSTIN *</label>
<input type="text" id="gstin" name="gstin" placeholder="29ABCDE1234F1Z5" required>
<label for="sac_code">SAC Code *</label>
<input type="text" id="sac_code" name="sac_code" placeholder="998314" required>
<label for="mode_of_export">Mode of Export of Services (GATS) *</label>
<select id="mode_of_export" name="mode_of_export">
<option value="1">Mode 1 — Cross-Border Supply</option>
<option value="2">Mode 2 — Consumption Abroad</option>
<option value="3">Mode 3 — Commercial Presence</option>
<option value="4">Mode 4 — Presence of Natural Persons</option>
</select>
<label for="purpose_code">RBI Purpose Code *</label>
<input type="text" id="purpose_code" name="purpose_code" placeholder="P0802" required>
<label for="invoice_pdf">Upload Commercial Invoice (PDF) *</label>
<input type="file" id="invoice_pdf" name="invoice_pdf" accept="application/pdf" required>
<label for="lut_pdf">Upload LUT Declaration (PDF) *</label>
<input type="file" id="lut_pdf" name="lut_pdf" accept="application/pdf" required>
<div style="margin-top:14px"><label style="display:inline;font-weight:400">
<input type="checkbox" id="declaration" name="declaration" value="yes" required>
I hereby declare that the particulars furnished are true and correct and that the remittance represents realization of the export proceeds referenced above.</label></div>
<div style="margin-top:18px"><button type="submit" id="submit-ebrc">Submit &amp; Generate eBRC</button></div>
</form>
<div class="mandate">Mode of Export is mandatory w.e.f. 01.05.2025 · GST invoice details mandatory w.e.f. 13.01.2026 · Self-certification carries legal liability under FEMA 1999.</div>
</div>"#
        ),
    )
    .into_response()
}

async fn certify_submit(
    State(state): State<Arc<Mutex<MockState>>>,
    mut mp: Multipart,
) -> impl IntoResponse {
    let mut fields = std::collections::HashMap::<String, String>::new();
    let mut irm_numbers: Vec<String> = Vec::new();
    let mut docs: Vec<PortalDoc> = Vec::new();
    while let Ok(Some(field)) = mp.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if let Some(filename) = field.file_name().map(str::to_owned) {
            let data = field.bytes().await.unwrap_or_default();
            docs.push(PortalDoc {
                filename,
                size: data.len(),
            });
        } else {
            let value = field.text().await.unwrap_or_default();
            if name == "irm" {
                irm_numbers.push(value);
            } else {
                fields.insert(name, value);
            }
        }
    }

    let mode: u8 = fields
        .get("mode_of_export")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let required = [
        "invoice_number",
        "invoice_date",
        "invoice_value_inr",
        "gstin",
        "sac_code",
        "purpose_code",
    ];
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|k| fields.get(*k).map(|v| v.trim().is_empty()).unwrap_or(true))
        .collect();
    let has_invoice_pdf = docs.iter().any(|d| d.filename.to_lowercase().ends_with(".pdf") && !d.filename.is_empty());
    let err = if !missing.is_empty() {
        Some(format!("mandatory fields missing: {}", missing.join(", ")))
    } else if !(1..=4).contains(&mode) {
        Some("mode_of_export must be 1-4".to_string())
    } else if fields.get("declaration").map(|v| v.as_str()) != Some("yes") {
        Some("self-certification declaration is required".to_string())
    } else if docs.len() < 2 || !has_invoice_pdf {
        Some("invoice and LUT PDF uploads are mandatory".to_string())
    } else {
        None
    };
    if let Some(e) = err {
        return (
            StatusCode::BAD_REQUEST,
            page("Filing rejected", &format!(r#"<div class="err"><strong>Filing rejected:</strong> {} <a href="/portal/irms">Back to IRM repository</a></div>"#, esc(&e))),
        )
            .into_response();
    }

    let ebrc_number = {
        let mut s = state.lock().unwrap();
        let unknown: Vec<&String> = irm_numbers
            .iter()
            .filter(|n| !s.list_irms().iter().any(|i| &i.irm_number == *n))
            .collect();
        if !unknown.is_empty() {
            return (
                StatusCode::NOT_FOUND,
                page("Filing rejected", &format!(r#"<div class="err"><strong>Unknown IRM(s):</strong> {} — they may already be certified.</div>"#, unknown.iter().map(|n| esc(n)).collect::<Vec<_>>().join(", "))),
            )
                .into_response();
        }
        let n = s.next_ebrc_number();
        s.consume_irms(&irm_numbers);
        s.push_portal_ebrc(PortalEbrc {
            ebrc_number: n.clone(),
            invoice_number: fields["invoice_number"].clone(),
            irm_numbers: irm_numbers.clone(),
            docs: docs.clone(),
        });
        n
    };

    page(
        "eBRC Issued",
        &format!(
            r#"<div class="success"><h2 style="margin-top:0">eBRC Issued Successfully</h2>
<p>Electronic Bank Realization Certificate generated against IEC declaration:</p>
<div id="ebrc-number">{ebrc}</div>
<table style="margin-top:14px"><tr><th>Invoice</th><th>IRMs Consumed</th><th>Documents</th></tr>
<tr><td>{inv}</td><td>{irms}</td><td>{docs}</td></tr></table>
<p class="meta" style="margin-top:12px">Download and forward this certificate to your AD bank for EDPMS closure.</p>
<a class="btn" href="/portal/ebrcs">View Issued eBRCs</a></div>"#,
            ebrc = esc(&ebrc_number),
            inv = esc(&fields["invoice_number"]),
            irms = esc(&irm_numbers.join(", ")),
            docs = esc(
                &docs
                    .iter()
                    .map(|d| format!("{} ({} bytes)", d.filename, d.size))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
    )
    .into_response()
}

async fn ebrcs_page(State(state): State<Arc<Mutex<MockState>>>) -> Html<String> {
    let ebrcs = state.lock().unwrap().list_portal_ebrcs();
    let rows = if ebrcs.is_empty() {
        r#"<tr><td colspan="4" class="meta">No eBRCs issued via the portal yet.</td></tr>"#
            .to_string()
    } else {
        ebrcs.iter()
            .map(|e| {
                format!(
                    "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    esc(&e.ebrc_number),
                    esc(&e.invoice_number),
                    esc(&e.irm_numbers.join(", ")),
                    esc(&e
                        .docs
                        .iter()
                        .map(|d| format!("{} ({} bytes)", d.filename, d.size))
                        .collect::<Vec<_>>()
                        .join(", ")),
                )
            })
            .collect()
    };
    page(
        "Issued eBRCs",
        &format!(
            r#"<div class="card"><h2>Issued eBRCs</h2>
<table><tr><th>eBRC Number</th><th>Invoice</th><th>IRMs</th><th>Uploaded Documents</th></tr>
{rows}</table></div>"#
        ),
    )
}

#[cfg(test)]
mod tests {
    use crate::mock::MockDgft;
    use crate::types::IrmDto;

    async fn spawn() -> String {
        let mock = MockDgft::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, mock.router()).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn pdf_part(name: &str, filename: &str) -> reqwest::multipart::Part {
        reqwest::multipart::Part::bytes(b"%PDF-1.4 fake".to_vec())
            .file_name(filename.to_string())
            .mime_str("application/pdf")
            .unwrap()
            // reqwest assigns the field name at Form::part call site
    }

    #[tokio::test]
    async fn portal_flow_issues_ebrc_and_consumes_irms() {
        let base = spawn().await;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        // Bank transmits an IRM.
        http.post(format!("{base}/seed-irm"))
            .json(&IrmDto {
                irm_number: "IRM-P1".into(),
                bank_name: "HDFC".into(),
                account_number: "001".into(),
                currency: "USD".into(),
                amount: "500.00".into(),
                remittance_date: "2026-11-06".into(),
                remitter_name: "Globex Inc".into(),
            })
            .send()
            .await
            .unwrap();

        // Login renders and accepts credentials (sets session cookie).
        let login_page = http.get(format!("{base}/portal")).send().await.unwrap();
        assert!(login_page.status().is_success());
        assert!(login_page.text().await.unwrap().contains("Exporter Login"));
        let login = http
            .post(format!("{base}/portal/login"))
            .form(&[("iec", "0123456789"), ("password", "demo")])
            .send()
            .await
            .unwrap();
        assert_eq!(login.status(), 303);
        assert!(login.headers().get("set-cookie").is_some());

        // IRM repository lists the unmapped IRM.
        let irms = http.get(format!("{base}/portal/irms")).send().await.unwrap();
        assert!(irms.text().await.unwrap().contains("IRM-P1"));

        // Certify form renders for the selected IRM.
        let form = http
            .get(format!("{base}/portal/certify?irm=IRM-P1"))
            .send()
            .await
            .unwrap();
        assert!(form.text().await.unwrap().contains("eBRC Self-Certification"));

        // Submit with invoice + LUT uploads.
        let form = reqwest::multipart::Form::new()
            .text("irm", "IRM-P1")
            .text("invoice_number", "INV/2026-27/0001")
            .text("invoice_date", "2026-11-06")
            .text("invoice_value_inr", "41500.00")
            .text("gstin", "29ABCDE1234F1Z5")
            .text("sac_code", "998314")
            .text("mode_of_export", "1")
            .text("purpose_code", "P0802")
            .text("declaration", "yes")
            .part("invoice_pdf", pdf_part("invoice_pdf", "invoice.pdf"))
            .part("lut_pdf", pdf_part("lut_pdf", "lut.pdf"));
        let resp = http
            .post(format!("{base}/portal/certify"))
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("eBRC Issued Successfully"), "{body}");
        assert!(body.contains("EBRC-2026-"), "{body}");

        // IRM consumed; eBRC listed with document metadata.
        let irms: serde_json::Value = reqwest::get(format!("{base}/irms")).await.unwrap().json().await.unwrap();
        assert_eq!(irms.as_array().unwrap().len(), 0);
        let ebrcs = http.get(format!("{base}/portal/ebrcs")).send().await.unwrap();
        let body = ebrcs.text().await.unwrap();
        assert!(body.contains("EBRC-2026-"));
        assert!(body.contains("invoice.pdf"));
        assert!(body.contains("lut.pdf"));
    }

    #[tokio::test]
    async fn certify_rejects_missing_declaration_and_unknown_irm() {
        let base = spawn().await;
        let http = reqwest::Client::new();
        http.post(format!("{base}/seed-irm"))
            .json(&IrmDto {
                irm_number: "IRM-P2".into(),
                bank_name: "HDFC".into(),
                account_number: "001".into(),
                currency: "USD".into(),
                amount: "10.00".into(),
                remittance_date: "2026-11-06".into(),
                remitter_name: "Acme".into(),
            })
            .send()
            .await
            .unwrap();

        // No declaration checkbox → 400.
        let form = reqwest::multipart::Form::new()
            .text("irm", "IRM-P2")
            .text("invoice_number", "INV/1")
            .text("invoice_date", "2026-11-06")
            .text("invoice_value_inr", "100.00")
            .text("gstin", "29ABCDE1234F1Z5")
            .text("sac_code", "998314")
            .text("mode_of_export", "1")
            .text("purpose_code", "P0802")
            .part("invoice_pdf", pdf_part("invoice_pdf", "invoice.pdf"))
            .part("lut_pdf", pdf_part("lut_pdf", "lut.pdf"));
        let resp = http
            .post(format!("{base}/portal/certify"))
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        assert!(resp.text().await.unwrap().contains("declaration"));

        // Unknown IRM → 404.
        let form = reqwest::multipart::Form::new()
            .text("irm", "IRM-NOPE")
            .text("invoice_number", "INV/1")
            .text("invoice_date", "2026-11-06")
            .text("invoice_value_inr", "100.00")
            .text("gstin", "29ABCDE1234F1Z5")
            .text("sac_code", "998314")
            .text("mode_of_export", "1")
            .text("purpose_code", "P0802")
            .text("declaration", "yes")
            .part("invoice_pdf", pdf_part("invoice_pdf", "invoice.pdf"))
            .part("lut_pdf", pdf_part("lut_pdf", "lut.pdf"));
        let resp = http
            .post(format!("{base}/portal/certify"))
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }
}
