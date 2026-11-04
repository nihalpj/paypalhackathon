# AutoFIRC Foundation Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A working ingestion pipeline: PayPal webhook → offline signature verification → Postgres ledger → Redis queue → enrichment worker → REST API, on a tested Rust workspace with the domain rule engines (`autofirc-core`) done.

**Architecture:** Cargo workspace, edition 2024. `autofirc-core` is pure domain logic (no I/O). `autofirc-paypal` owns all PayPal I/O behind typed clients. `autofirc-server` (Axum) wires them: webhook route enqueues to Upstash Redis via `apalis-redis`; a worker enriches via the Orders API. Infra from `.env` (gitignored): Supabase Postgres, Upstash Redis.

**Tech Stack:** Rust 1.98 nightly, axum 0.8, sqlx 0.8 (postgres+rustls), apalis 0.7 + apalis-redis, reqwest 0.12 (rustls), rsa 0.9, sha2, crc32fast, x509-parser, wiremock 0.6, chrono, serde.

---

### Task 1: Workspace scaffold + CI

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `rust-toolchain.toml`
- Create: `.github/workflows/ci.yml`
- Create: `crates/autofirc-core/Cargo.toml`, `crates/autofirc-core/src/lib.rs`

- [ ] **Step 1: Create workspace files**

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
edition = "2024"
license = "MIT"
repository = "https://github.com/autofirc/autofirc"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
dotenvy = "0.15"
anyhow = "1"
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "nightly"
components = ["rustfmt", "clippy"]
```

`.github/workflows/ci.yml`:
```yaml
name: ci
on: [push, pull_request]
jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with: { components: rustfmt, clippy }
      - run: cargo fmt --check
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo test --workspace
  web:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 22 }
      - run: test -f web/package.json && (cd web && npm ci && npm run build) || echo "web/ not yet present"
```

`crates/autofirc-core/Cargo.toml`:
```toml
[package]
name = "autofirc-core"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
chrono.workspace = true
thiserror.workspace = true
```

`crates/autofirc-core/src/lib.rs`:
```rust
//! AutoFIRC domain core: ledger models, RBI purpose codes, FEMA deadline
//! engine, DGFT clubbing rules, and the ledger status machine.
pub mod clubbing;
pub mod fema;
pub mod models;
pub mod purpose;
```

Create empty `src/clubbing.rs`, `src/fema.rs`, `src/models.rs`, `src/purpose.rs` (each with a `//!` doc line so the crate compiles).

- [ ] **Step 2: Verify scaffold compiles**

Run: `cargo check --workspace`
Expected: `Finished` with no errors.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .github crates
git commit -m "chore: cargo workspace scaffold with ci"
```

---

### Task 2: `autofirc-core` — ledger models + status machine

**Files:**
- Create: `crates/autofirc-core/src/models.rs`
- Test: `crates/autofirc-core/src/models.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Append to `models.rs` (replacing the placeholder doc line):
```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p autofirc-core status_machine`
Expected: FAIL — `crate::purpose::PurposeCode` unresolved (`purpose.rs` is still an empty placeholder) and `serde_json` missing from core deps.

- [ ] **Step 3: Write minimal implementation**

Add to `crates/autofirc-core/Cargo.toml` under `[dependencies]`:
```toml
serde_json.workspace = true
```

Replace `crates/autofirc-core/src/purpose.rs` with a minimal `PurposeCode` (full table arrives in Task 3):
```rust
use serde::{Deserialize, Serialize};
use std::fmt;

/// RBI FETERS purpose codes relevant to digital service exporters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PurposeCode {
    /// Software consultancy/implementation.
    P0802,
    /// Data base / data processing charges.
    P0803,
    /// Off-site software exports (Non-STP registration + SOFTEX required).
    P0807,
    /// Business and management consultancy.
    P1006,
    /// Advance payment against physical-goods exports. FORBIDDEN for services:
    /// creates an unfulfillable EDPMS "ghost" export obligation.
    P0103,
    Other(String),
}

impl fmt::Display for PurposeCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PurposeCode::P0802 => write!(f, "P0802"),
            PurposeCode::P0803 => write!(f, "P0803"),
            PurposeCode::P0807 => write!(f, "P0807"),
            PurposeCode::P1006 => write!(f, "P1006"),
            PurposeCode::P0103 => write!(f, "P0103"),
            PurposeCode::Other(s) => write!(f, "{s}"),
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p autofirc-core status_machine`
Expected: PASS (1 test).

- [ ] **Step 5: Commit**

```bash
git add crates/autofirc-core
git commit -m "feat(core): ledger model and status machine"
```

---

### Task 3: `autofirc-core` — FEMA deadline engine

**Files:**
- Modify: `crates/autofirc-core/src/fema.rs`
- Test: inline in `fema.rs`

Rules (verified against FEMA 23(R)/2026-RB as amended 22 Sep 2026): realization deadline = invoice date + 9 months (12 if INR/Vostro-invoiced). Dashboard flags: **amber** from the 7-month mark (i.e., ≤ 2 months remaining), **red** from the 8-month mark (≤ 1 month remaining) while unreconciled.

- [ ] **Step 1: Write the failing test**

Replace `fema.rs`:
```rust
use chrono::{Months, NaiveDate};

/// Alert level for an unreconciled transaction's FEMA realization window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    Green,
    /// Entered at the 7-month mark: 2 months or less remain.
    Amber,
    /// Entered at the 8-month mark: 1 month or less remains.
    Red,
}

/// Statutory realization deadline: 9 months from invoice date, or 12 months
/// when invoiced/settled in INR via Vostro accounts.
pub fn realization_deadline(invoice_date: NaiveDate, inr_vostro: bool) -> NaiveDate {
    let months = if inr_vostro { 12 } else { 9 };
    invoice_date
        .checked_add_months(Months::new(months))
        .expect("invoice date + statutory window overflows chrono")
}

pub fn alert_level(
    invoice_date: NaiveDate,
    inr_vostro: bool,
    reconciled: bool,
    today: NaiveDate,
) -> AlertLevel {
    if reconciled {
        return AlertLevel::Green;
    }
    let remaining = realization_deadline(invoice_date, inr_vostro) - today;
    if remaining.num_days() <= 31 {
        AlertLevel::Red
    } else if remaining.num_days() <= 62 {
        AlertLevel::Amber
    } else {
        AlertLevel::Green
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn deadline_is_9_months_for_forex_and_12_for_inr_vostro() {
        assert_eq!(realization_deadline(d(2026, 10, 15), false), d(2027, 7, 15));
        assert_eq!(realization_deadline(d(2026, 10, 15), true), d(2027, 10, 15));
    }

    #[test]
    fn unreconciled_alerts_at_7_and_8_month_marks() {
        let invoice = d(2026, 10, 15);
        // ~5 months elapsed: green
        assert_eq!(alert_level(invoice, false, false, d(2027, 3, 15)), AlertLevel::Green);
        // 7-month mark (2 months left): amber
        assert_eq!(alert_level(invoice, false, false, d(2027, 5, 15)), AlertLevel::Amber);
        // 8-month mark (1 month left): red
        assert_eq!(alert_level(invoice, false, false, d(2027, 6, 15)), AlertLevel::Red);
        // reconciled rows never alert
        assert_eq!(alert_level(invoice, false, true, d(2027, 7, 14)), AlertLevel::Green);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p autofirc-core fema`
Expected: FAIL — placeholder `fema.rs` replaced, but first run fails on `realization_deadline`/`alert_level` not existing IF you ran before replacing; after replacing the file with tests+impl together this step is a compile check. To honor TDD: temporarily comment out the impl fns, run (fail: "cannot find function"), restore.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p autofirc-core fema`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/autofirc-core/src/fema.rs
git commit -m "feat(core): fema 9/12-month realization deadline engine"
```

---

### Task 4: `autofirc-core` — DGFT IRM clubbing rules

**Files:**
- Modify: `crates/autofirc-core/src/clubbing.rs`
- Test: inline in `clubbing.rs`

Rules (DGFT self-certification): IRMs club into one eBRC only if same bank, same account number, same currency. For advance payments, invoice date must be ≥ remittance date.

- [ ] **Step 1: Write the failing test + implementation**

Replace `clubbing.rs`:
```rust
use chrono::NaiveDate;
use thiserror::Error;

/// An Inward Remittance Message uploaded by a bank to DGFT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Irm {
    pub irm_number: String,
    pub bank_name: String,
    pub account_number: String,
    pub currency: String,
    pub remittance_date: NaiveDate,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClubError {
    #[error("IRMs from different banks cannot be clubbed")]
    DifferentBank,
    #[error("IRMs into different accounts cannot be clubbed")]
    DifferentAccount,
    #[error("IRMs in different currencies cannot be clubbed")]
    DifferentCurrency,
    #[error("advance payment: invoice date {invoice} precedes remittance date {remittance}")]
    AdvanceBeforeInvoice { invoice: NaiveDate, remittance: NaiveDate },
}

/// Two IRMs may be clubbed into a single eBRC only under DGFT's rules.
pub fn can_club(a: &Irm, b: &Irm) -> Result<(), ClubError> {
    if a.bank_name != b.bank_name {
        return Err(ClubError::DifferentBank);
    }
    if a.account_number != b.account_number {
        return Err(ClubError::DifferentAccount);
    }
    if a.currency != b.currency {
        return Err(ClubError::DifferentCurrency);
    }
    Ok(())
}

/// For advance payments the invoice date must be on/after the remittance date.
pub fn validate_advance(is_advance: bool, invoice_date: NaiveDate, remittance_date: NaiveDate) -> Result<(), ClubError> {
    if is_advance && invoice_date < remittance_date {
        return Err(ClubError::AdvanceBeforeInvoice { invoice: invoice_date, remittance: remittance_date });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn irm(bank: &str, acct: &str, ccy: &str) -> Irm {
        Irm {
            irm_number: "IRM-1".into(),
            bank_name: bank.into(),
            account_number: acct.into(),
            currency: ccy.into(),
            remittance_date: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
        }
    }

    #[test]
    fn clubbing_requires_same_bank_account_currency() {
        assert!(can_club(&irm("HDFC", "001", "USD"), &irm("HDFC", "001", "USD")).is_ok());
        assert_eq!(can_club(&irm("HDFC", "001", "USD"), &irm("ICICI", "001", "USD")), Err(ClubError::DifferentBank));
        assert_eq!(can_club(&irm("HDFC", "001", "USD"), &irm("HDFC", "002", "USD")), Err(ClubError::DifferentAccount));
        assert_eq!(can_club(&irm("HDFC", "001", "USD"), &irm("HDFC", "001", "EUR")), Err(ClubError::DifferentCurrency));
    }

    #[test]
    fn advance_payment_needs_invoice_on_or_after_remittance() {
        let inv = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let rem = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap();
        assert!(validate_advance(true, inv, rem).is_ok());
        assert!(validate_advance(false, rem, inv).is_ok()); // non-advance: no constraint
        assert!(matches!(validate_advance(true, rem, inv), Err(ClubError::AdvanceBeforeInvoice { .. })));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p autofirc-core clubbing`
Expected: PASS (2 tests). (As in Task 3, honor TDD by stubbing fns with `todo!()` first, watching the tests panic, then implementing.)

- [ ] **Step 3: Commit**

```bash
git add crates/autofirc-core/src/clubbing.rs
git commit -m "feat(core): dgft irm clubbing and advance-payment rules"
```

---

### Task 5: `autofirc-paypal` — webhook signature verification core

**Files:**
- Create: `crates/autofirc-paypal/Cargo.toml`
- Create: `crates/autofirc-paypal/src/lib.rs`
- Create: `crates/autofirc-paypal/src/webhook.rs`
- Test: inline in `webhook.rs`

PayPal offline verification: `crc32(body_bytes)` as unsigned u32; message string `transmission_id|transmission_time|webhook_id|crc32`; RSA-SHA256 (PKCS#1 v1.5) verify against the X.509 cert's public key; cert URL must be `https://*.paypal.com` or `https://*.paypalobjects.com`… we allow exactly host `paypal.com` and subdomains (`*.paypal.com`) per the PRD's spoofing concern — subdomain-only.

- [ ] **Step 1: Write the failing test**

`crates/autofirc-paypal/Cargo.toml`:
```toml
[package]
name = "autofirc-paypal"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
anyhow.workspace = true
tokio.workspace = true
tracing.workspace = true
crc32fast = "1"
base64 = "0.22"
sha2 = "0.10"
rsa = "0.9"
x509-parser = "0.16"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
url = "2"

[dev-dependencies]
wiremock = "0.6"
rand = "0.8"
```

`crates/autofirc-paypal/src/lib.rs`:
```rust
//! PayPal platform integration: webhook verification, OAuth2, REST clients.
pub mod client;
pub mod webhook;
```
(create empty `src/client.rs` with a `//!` doc line for now)

`crates/autofirc-paypal/src/webhook.rs`:
```rust
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rsa::RsaPublicKey;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::signature::Verifier;
use sha2::Sha256;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("untrusted certificate url host: {0}")]
    UntrustedCertUrl(String),
    #[error("invalid base64 signature")]
    BadSignatureEncoding,
    #[error("signature verification failed")]
    VerificationFailed,
    #[error("certificate fetch/parse failed: {0}")]
    Cert(String),
}

/// Headers PayPal sends with every webhook delivery.
#[derive(Debug, Clone)]
pub struct TransmissionMeta {
    pub transmission_id: String,
    pub transmission_time: String,
    pub cert_url: String,
    pub transmission_sig: String,
}

/// CRC32 of the *raw* request body, as an unsigned 32-bit integer.
pub fn body_crc32(raw_body: &[u8]) -> u32 {
    crc32fast::hash(raw_body)
}

/// The exact pipe-delimited message PayPal signs.
pub fn signed_message(meta: &TransmissionMeta, webhook_id: &str, raw_body: &[u8]) -> String {
    format!(
        "{}|{}|{}|{}",
        meta.transmission_id,
        meta.transmission_time,
        webhook_id,
        body_crc32(raw_body)
    )
}

/// Only certificates served from paypal.com (or its subdomains) are trusted.
pub fn is_trusted_cert_url(cert_url: &str) -> bool {
    let Ok(url) = Url::parse(cert_url) else { return false };
    if url.scheme() != "https" {
        return false;
    }
    match url.host_str() {
        Some("paypal.com") => true,
        Some(host) => host.ends_with(".paypal.com"),
        None => false,
    }
}

/// Verify the RSA-SHA256 signature over the signed message.
pub fn verify_signature(
    meta: &TransmissionMeta,
    webhook_id: &str,
    raw_body: &[u8],
    public_key: &RsaPublicKey,
) -> Result<(), WebhookError> {
    let sig_bytes = B64
        .decode(&meta.transmission_sig)
        .map_err(|_| WebhookError::BadSignatureEncoding)?;
    let signature = Signature::try_from(sig_bytes.as_slice())
        .map_err(|_| WebhookError::BadSignatureEncoding)?;
    let message = signed_message(meta, webhook_id, raw_body);
    VerifyingKey::<Sha256>::new(public_key.clone())
        .verify(message.as_bytes(), &signature)
        .map_err(|_| WebhookError::VerificationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD as B64};
    use rsa::pkcs1v15::SigningKey;
    use rsa::signature::{RandomizedSigner, SignatureEncoding};

    #[test]
    fn crc32_matches_known_vector() {
        // CRC-32/ISO-HDLC of "123456789" is 0xCBF43926.
        assert_eq!(body_crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn cert_url_allowlist() {
        assert!(is_trusted_cert_url("https://api.paypal.com/cert.pem"));
        assert!(is_trusted_cert_url("https://api.sandbox.paypal.com/cert.pem"));
        assert!(!is_trusted_cert_url("http://api.paypal.com/cert.pem")); // no http
        assert!(!is_trusted_cert_url("https://paypal.com.evil.example/cert.pem"));
        assert!(!is_trusted_cert_url("https://evil-paypal.com/cert.pem"));
        assert!(!is_trusted_cert_url("not a url"));
    }

    #[test]
    fn signed_message_and_verification_round_trip() {
        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = rsa::RsaPublicKey::from(&private);

        let meta = TransmissionMeta {
            transmission_id: "tid-1".into(),
            transmission_time: "2026-11-04T10:00:00Z".into(),
            cert_url: "https://api.paypal.com/cert.pem".into(),
            transmission_sig: String::new(),
        };
        let body = br#"{"id":"WH-1","event_type":"PAYMENT.CAPTURE.COMPLETED"}"#;
        let message = signed_message(&meta, "WEBHOOK-ID-42", body);
        assert!(message.starts_with("tid-1|2026-11-04T10:00:00Z|WEBHOOK-ID-42|"));

        let signing = SigningKey::<Sha256>::new(private);
        let sig = signing.sign_with_rng(&mut rng, message.as_bytes());
        let meta = TransmissionMeta {
            transmission_sig: B64.encode(sig.to_bytes()),
            ..meta
        };
        assert!(verify_signature(&meta, "WEBHOOK-ID-42", body, &public).is_ok());

        // wrong webhook id -> different message -> must fail
        assert!(matches!(
            verify_signature(&meta, "WRONG", body, &public),
            Err(WebhookError::VerificationFailed)
        ));
        // tampered body -> different crc32 -> must fail
        assert!(matches!(
            verify_signature(&meta, "WEBHOOK-ID-42", b"{\"tampered\":true}", &public),
            Err(WebhookError::VerificationFailed)
        ));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p autofirc-paypal webhook`
Expected: compile FAIL the first time (missing crate wiring) — after adding `Cargo.toml`/`lib.rs`, run again; to honor TDD, stub `verify_signature` with `todo!()` and watch the round-trip test panic, then implement.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p autofirc-paypal webhook`
Expected: PASS (3 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/autofirc-paypal
git commit -m "feat(paypal): offline webhook signature verification (crc32 + rsa-sha256)"
```

---

### Task 6: `autofirc-paypal` — X.509 cert fetch → public key

**Files:**
- Modify: `crates/autofirc-paypal/src/webhook.rs`
- Test: inline (wiremock HTTP test)

- [ ] **Step 1: Write the failing test**

Append to `webhook.rs` (before `#[cfg(test)]`):
```rust
/// Fetch the signing certificate from a trusted URL and extract its RSA key.
pub async fn fetch_public_key(
    http: &reqwest::Client,
    cert_url: &str,
) -> Result<RsaPublicKey, WebhookError> {
    if !is_trusted_cert_url(cert_url) {
        let host = Url::parse(cert_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| cert_url.to_owned());
        return Err(WebhookError::UntrustedCertUrl(host));
    }
    let pem = http
        .get(cert_url)
        .send()
        .await
        .map_err(|e| WebhookError::Cert(e.to_string()))?
        .text()
        .await
        .map_err(|e| WebhookError::Cert(e.to_string()))?;
    public_key_from_pem(&pem)
}

/// Parse a PEM X.509 certificate and return its RSA public key.
pub fn public_key_from_pem(pem: &str) -> Result<RsaPublicKey, WebhookError> {
    let (_, pem_block) = x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .map_err(|e| WebhookError::Cert(format!("pem parse: {e}")))?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem_block.contents)
        .map_err(|e| WebhookError::Cert(format!("x509 parse: {e}")))?;
    let spki = cert.public_key();
    RsaPublicKey::try_from(spki).map_err(|e| WebhookError::Cert(format!("spki: {e}")))
}
```

Append inside `mod tests`:
```rust
    #[tokio::test]
    async fn untrusted_cert_url_is_rejected_before_network() {
        let http = reqwest::Client::new();
        let err = fetch_public_key(&http, "https://evil.example.com/cert.pem")
            .await
            .unwrap_err();
        assert!(matches!(err, WebhookError::UntrustedCertUrl(_)));
    }

    #[test]
    fn parses_rsa_key_from_real_pem_fixture() {
        // Generated with: openssl req -x509 -newkey rsa:2048 -keyout /dev/null \
        //   -out cert.pem -days 1 -nodes -subj "/CN=api.paypal.com"
        // The key inside must round-trip through x509-parser.
        let pem = include_str!("../tests/fixtures/test_cert.pem");
        let key = public_key_from_pem(pem).expect("fixture cert parses");
        assert_eq!(key.size(), 256); // 2048-bit
    }
```

- [ ] **Step 2: Create the fixture and run test to verify it fails**

Run:
```bash
mkdir -p crates/autofirc-paypal/tests/fixtures
openssl req -x509 -newkey rsa:2048 -keyout /tmp/k.pem -out crates/autofirc-paypal/tests/fixtures/test_cert.pem -days 3650 -nodes -subj "/CN=api.sandbox.paypal.com"
cargo test -p autofirc-paypal
```
Expected: tests compile; `parses_rsa_key_from_real_pem_fixture` FAILS if `RsaPublicKey::try_from(spki)` doesn't exist — if so, implement via `rsa::pkcs8::DecodePublicKey` on `spki.raw`:
```rust
use rsa::pkcs8::DecodePublicKey;
RsaPublicKey::from_public_key_der(spki.raw).map_err(|e| WebhookError::Cert(format!("spki der: {e}")))
```
(Use whichever conversion compiles; keep the DER one if both work.)

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p autofirc-paypal`
Expected: PASS (all webhook tests).

- [ ] **Step 4: Commit**

```bash
git add crates/autofirc-paypal
git commit -m "feat(paypal): trusted x509 cert fetch and rsa key extraction"
```

---

### Task 7: `autofirc-paypal` — OAuth2 + Orders/Captures client

**Files:**
- Modify: `crates/autofirc-paypal/src/client.rs`
- Test: inline (wiremock)

- [ ] **Step 1: Write the failing test**

Replace `client.rs`:
```rust
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct PayPalClient {
    http: reqwest::Client,
    base_url: String,
    client_id: String,
    client_secret: String,
    token: Arc<Mutex<Option<CachedToken>>>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: std::time::Instant,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Order {
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub purchase_units: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Capture {
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub amount: Option<serde_json::Value>,
    #[serde(default)]
    pub seller_receivable_breakdown: Option<serde_json::Value>,
}

impl PayPalClient {
    pub fn new(base_url: &str, client_id: &str, client_secret: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            token: Arc::new(Mutex::new(None)),
        }
    }

    async fn access_token(&self) -> anyhow::Result<String> {
        let mut guard = self.token.lock().await;
        if let Some(t) = &*guard {
            if t.expires_at > std::time::Instant::now() + std::time::Duration::from_secs(60) {
                return Ok(t.access_token.clone());
            }
        }
        let resp: TokenResponse = self
            .http
            .post(format!("{}/v1/oauth2/token", self.base_url))
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let token = CachedToken {
            access_token: resp.access_token.clone(),
            expires_at: std::time::Instant::now()
                + std::time::Duration::from_secs(resp.expires_in),
        };
        *guard = Some(token);
        Ok(resp.access_token)
    }

    pub async fn get_order(&self, order_id: &str) -> anyhow::Result<Order> {
        let token = self.access_token().await?;
        Ok(self
            .http
            .get(format!("{}/v2/checkout/orders/{order_id}", self.base_url))
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn get_capture(&self, capture_id: &str) -> anyhow::Result<Capture> {
        let token = self.access_token().await?;
        Ok(self
            .http
            .get(format!("{}/v2/payments/captures/{capture_id}", self.base_url))
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn token_is_fetched_once_and_cached() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "A21AA-test", "token_type": "Bearer", "expires_in": 3600
            })))
            .expect(1) // second call must reuse the cache
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/checkout/orders/ORDER-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ORDER-1", "status": "COMPLETED", "purchase_units": []
            })))
            .mount(&server)
            .await;

        let client = PayPalClient::new(&server.uri(), "id", "secret");
        let o1 = client.get_order("ORDER-1").await.unwrap();
        let o2 = client.get_order("ORDER-1").await.unwrap();
        assert_eq!(o1.id, "ORDER-1");
        assert_eq!(o2.status, "COMPLETED");
    }

    #[tokio::test]
    async fn capture_fetches_settlement_breakdown() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "A21AA-test", "expires_in": 3600
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/payments/captures/CAP-9"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "CAP-9", "status": "COMPLETED",
                "amount": {"currency_code": "USD", "value": "500.00"},
                "seller_receivable_breakdown": {"net_amount": {"currency_code": "INR", "value": "41500.00"}}
            })))
            .mount(&server)
            .await;

        let client = PayPalClient::new(&server.uri(), "id", "secret");
        let cap = client.get_capture("CAP-9").await.unwrap();
        assert_eq!(cap.id, "CAP-9");
        assert!(cap.seller_receivable_breakdown.is_some());
    }
}
```

- [ ] **Step 2: Run tests (fail → implement → pass)**

Run: `cargo test -p autofirc-paypal client`
Expected: FAIL on first run with `todo!()` stubs; PASS after implementation above (2 tests).

- [ ] **Step 3: Commit**

```bash
git add crates/autofirc-paypal/src/client.rs
git commit -m "feat(paypal): oauth2 token cache and orders/captures clients"
```

---

### Task 8: Postgres ledger repository + migration

**Files:**
- Create: `crates/autofirc-server/Cargo.toml`
- Create: `crates/autofirc-server/src/lib.rs`
- Create: `crates/autofirc-server/migrations/0001_ledger.sql`
- Create: `crates/autofirc-server/src/db.rs`
- Test: `crates/autofirc-server/src/db.rs` (ignored-by-default test hitting `DATABASE_URL_DIRECT`; runs in CI when env present)

- [ ] **Step 1: Scaffold the server crate**

`crates/autofirc-server/Cargo.toml`:
```toml
[package]
name = "autofirc-server"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
autofirc-core = { path = "../autofirc-core" }
autofirc-paypal = { path = "../autofirc-paypal" }
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
thiserror.workspace = true
anyhow.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
dotenvy.workspace = true
axum = "0.8"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio-rustls", "postgres", "chrono", "json", "macros"] }
apalis = "0.7"
apalis-redis = "0.7"

[dev-dependencies]
wiremock = "0.6"
tower = "0.5"
```

`crates/autofirc-server/src/lib.rs`:
```rust
//! Axum server: webhook ingestion, queue consumers, dashboard API.
pub mod db;
pub mod routes;
pub mod worker;
```
(create `routes.rs`, `worker.rs` with `//!` placeholder lines)

`crates/autofirc-server/migrations/0001_ledger.sql`:
```sql
CREATE TABLE IF NOT EXISTS ledger (
    id               TEXT PRIMARY KEY,            -- paypal webhook event id
    capture_id       TEXT NOT NULL,
    order_id         TEXT NOT NULL,
    client_name      TEXT NOT NULL DEFAULT '',
    amount_inr_paise BIGINT NOT NULL DEFAULT 0,
    invoice_date     DATE NOT NULL DEFAULT CURRENT_DATE,
    inr_vostro       BOOLEAN NOT NULL DEFAULT FALSE,
    purpose_code     TEXT,
    mode_of_export   SMALLINT,
    sac_code         TEXT,
    ai_confidence    SMALLINT,
    ebrc_number      TEXT,
    status           TEXT NOT NULL DEFAULT 'pending_analysis',
    enrichment       JSONB,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- [ ] **Step 2: Write the failing test**

`crates/autofirc-server/src/db.rs`:
```rust
use anyhow::Context;
use sqlx::PgPool;

pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPool::connect(database_url).await.context("postgres connect")?;
    Ok(pool)
}

pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await.context("run migrations")?;
    Ok(())
}

/// Insert a webhook row; returns false when the id already exists (idempotency).
pub async fn insert_webhook_event(
    pool: &PgPool,
    id: &str,
    capture_id: &str,
    order_id: &str,
) -> anyhow::Result<bool> {
    let rows = sqlx::query(
        "INSERT INTO ledger (id, capture_id, order_id) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(capture_id)
    .bind(order_id)
    .execute(pool)
    .await?;
    Ok(rows.rows_affected() == 1)
}

pub async fn set_enrichment(
    pool: &PgPool,
    id: &str,
    enrichment: &serde_json::Value,
    status: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE ledger SET enrichment = $2, status = $3, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(enrichment)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_ledger(pool: &PgPool) -> anyhow::Result<Vec<serde_json::Value>> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT to_jsonb(l) FROM ledger l ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requires DATABASE_URL_DIRECT in env; ignored by default so CI without
    /// credentials stays green.
    #[tokio::test]
    #[ignore = "needs live postgres"]
    async fn duplicate_webhook_ids_are_idempotent() {
        dotenvy::dotenv().ok();
        let url = std::env::var("DATABASE_URL_DIRECT").expect("DATABASE_URL_DIRECT");
        let pool = connect(&url).await.unwrap();
        migrate(&pool).await.unwrap();
        let id = format!("TEST-{}", uuid_v4());
        assert!(insert_webhook_event(&pool, &id, "CAP-T", "ORD-T").await.unwrap());
        assert!(!insert_webhook_event(&pool, &id, "CAP-T", "ORD-T").await.unwrap());
        sqlx::query("DELETE FROM ledger WHERE id = $1").bind(&id).execute(&pool).await.unwrap();
    }

    fn uuid_v4() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        format!("{nanos:x}")
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p autofirc-server db` → compiles, test is `ignored`.
Run: `cargo test -p autofirc-server -- --ignored` → PASS against the provided Supabase `DATABASE_URL_DIRECT` (verifies connectivity + migration + idempotency for real).

- [ ] **Step 4: Commit**

```bash
git add crates/autofirc-server
git commit -m "feat(server): postgres ledger repository with idempotent inserts"
```

---

### Task 9: Webhook ingestion route (verify → insert → enqueue → 200)

**Files:**
- Modify: `crates/autofirc-server/src/routes.rs`
- Create: `crates/autofirc-server/src/main.rs`
- Test: inline in `routes.rs` (tower oneshot, no network)

- [ ] **Step 1: Write the failing test**

Replace `routes.rs`:
```rust
use autofirc_paypal::webhook::{self, TransmissionMeta};
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub paypal_cert_http: reqwest::Client,
    pub webhook_id: String,
    /// Queue sink: worker job payloads (webhook event id + raw body).
    pub enqueue: Arc<dyn Fn(String, Bytes) -> futures_send + Send + Sync>,
    /// Test hook: when set, skips cert fetch and uses this key directly.
    pub pinned_webhook_key: Option<rsa::RsaPublicKey>,
}

pub type futures_send = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

#[derive(Debug, serde::Deserialize)]
struct WebhookEvent {
    id: String,
    #[serde(default)]
    resource: Option<WebhookResource>,
}

#[derive(Debug, serde::Deserialize)]
struct WebhookResource {
    id: Option<String>,
    supplementary_data: Option<serde_json::Value>,
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

pub async fn paypal_webhook(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let (Some(tid), Some(tt), Some(cert), Some(sig)) = (
        header(&headers, "paypal-transmission-id"),
        header(&headers, "paypal-transmission-time"),
        header(&headers, "paypal-cert-url"),
        header(&headers, "paypal-transmission-sig"),
    ) else {
        return StatusCode::BAD_REQUEST;
    };
    let meta = TransmissionMeta {
        transmission_id: tid.to_owned(),
        transmission_time: tt.to_owned(),
        cert_url: cert.to_owned(),
        transmission_sig: sig.to_owned(),
    };
    let key = match &state.pinned_webhook_key {
        Some(k) => k.clone(),
        None => match webhook::fetch_public_key(&state.paypal_cert_http, &meta.cert_url).await {
            Ok(k) => k,
            Err(_) => return StatusCode::UNAUTHORIZED,
        },
    };
    if webhook::verify_signature(&meta, &state.webhook_id, &body, &key).is_err() {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(event) = serde_json::from_slice::<WebhookEvent>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    let capture_id = event
        .resource
        .as_ref()
        .and_then(|r| r.id.clone())
        .unwrap_or_default();
    let order_id = event
        .resource
        .and_then(|r| r.supplementary_data)
        .and_then(|sd| sd.get("related_ids")?.get("order_id")?.as_str().map(str::to_owned))
        .unwrap_or_default();
    match crate::db::insert_webhook_event(&state.pool, &event.id, &capture_id, &order_id).await {
        Ok(true) => {
            (state.enqueue)(event.id, body).await;
            StatusCode::OK
        }
        Ok(false) => StatusCode::OK, // duplicate: silently ack, no reprocessing
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/webhooks/paypal", post(paypal_webhook))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD as B64};
    use rsa::pkcs1v15::SigningKey;
    use rsa::signature::{RandomizedSigner, SignatureEncoding};
    use sha2::Sha256;
    use std::sync::Mutex as StdMutex;

    async fn test_state() -> (AppState, rsa::RsaPrivateKey, Arc<StdMutex<Vec<String>>>) {
        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = rsa::RsaPublicKey::from(&private);
        // A pool that is never reached in the unauthorized-path tests.
        let pool = sqlx::PgPool::connect_lazy("postgres://unused:unused@localhost/unused").unwrap();
        let enqueued = Arc::new(StdMutex::new(Vec::new()));
        let enq = enqueued.clone();
        let state = AppState {
            pool,
            paypal_cert_http: reqwest::Client::new(),
            webhook_id: "WH-TEST".into(),
            enqueue: Arc::new(move |id, _body| {
                let enq = enq.clone();
                Box::pin(async move { enq.lock().unwrap().push(id); })
            }),
            pinned_webhook_key: Some(public),
        };
        (state, private, enqueued)
    }

    #[tokio::test]
    async fn rejects_bad_signature_with_401() {
        let (state, _priv, enqueued) = test_state().await;
        let app = router(state);
        let req = axum::http::Request::post("/api/webhooks/paypal")
            .header("paypal-transmission-id", "t1")
            .header("paypal-transmission-time", "2026-11-04T00:00:00Z")
            .header("paypal-cert-url", "https://api.paypal.com/c.pem")
            .header("paypal-transmission-sig", B64.encode(b"garbage"))
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(enqueued.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_missing_headers_with_400() {
        let (state, _priv, _) = test_state().await;
        let app = router(state);
        let req = axum::http::Request::post("/api/webhooks/paypal")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn signature_fixture_signs_correctly() {
        // Sanity: the test harness signs exactly what the route verifies.
        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = rsa::RsaPublicKey::from(&private);
        let meta = TransmissionMeta {
            transmission_id: "t1".into(),
            transmission_time: "2026-11-04T00:00:00Z".into(),
            cert_url: "https://api.paypal.com/c.pem".into(),
            transmission_sig: String::new(),
        };
        let body = br#"{"id":"EV-1","resource":{"id":"CAP-1"}}"#;
        let msg = webhook::signed_message(&meta, "WH-TEST", body);
        let sig = SigningKey::<Sha256>::new(private).sign_with_rng(&mut rng, msg.as_bytes());
        let meta = TransmissionMeta { transmission_sig: B64.encode(sig.to_bytes()), ..meta };
        assert!(webhook::verify_signature(&meta, "WH-TEST", body, &public).is_ok());
    }
}
```

Add `rsa = "0.9"`, `base64 = "0.22"`, `sha2 = "0.10"` to `crates/autofirc-server/Cargo.toml` `[dependencies]`, and `rand = "0.8"` to `[dev-dependencies]`.

- [ ] **Step 2: Run tests (fail → pass)**

Run: `cargo test -p autofirc-server routes`
Expected: PASS (3 tests). No DB or network touched (lazy pool; enqueue is a closure).

- [ ] **Step 3: Commit**

```bash
git add crates/autofirc-server
git commit -m "feat(server): webhook ingestion route with signature gate and idempotent enqueue"
```

---

### Task 10: Worker + binary (`main.rs`): Redis queue → enrich → REST

**Files:**
- Modify: `crates/autofirc-server/src/worker.rs`
- Create: `crates/autofirc-server/src/main.rs`
- Modify: `crates/autofirc-server/src/routes.rs` (add GET /api/ledger)

- [ ] **Step 1: Worker**

Replace `worker.rs`:
```rust
use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Job pushed by the webhook route after signature verification + DB insert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichJob {
    pub event_id: String,
    pub capture_id: String,
    pub order_id: String,
}

/// Enrich a ledger row with full PayPal order/capture data.
/// Failure leaves the row in `pending_analysis` for retry.
pub async fn enrich(
    job: EnrichJob,
    paypal: &autofirc_paypal::client::PayPalClient,
    pool: &sqlx::PgPool,
) -> anyhow::Result<()> {
    let order = paypal.get_order(&job.order_id).await.context("get order")?;
    let capture = paypal.get_capture(&job.capture_id).await.context("get capture")?;
    let enrichment = serde_json::json!({ "order": order, "capture": capture });
    crate::db::set_enrichment(pool, &job.event_id, &enrichment, "enriched").await?;
    tracing::info!(event_id = %job.event_id, "enriched ledger row");
    Ok(())
}
```

- [ ] **Step 2: main.rs — wire pool, Redis storage, worker, router**

Replace `main.rs`:
```rust
use anyhow::Context;
use apalis::prelude::*;
use apalis_redis::RedisStorage;
use autofirc_server::routes::{self, AppState};
use autofirc_server::worker::{self, EnrichJob};
use sqlx::PgPool;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL")?;
    let redis_url = std::env::var("REDIS_URL").context("REDIS_URL")?;
    let webhook_id = std::env::var("PAYPAL_WEBHOOK_ID").unwrap_or_else(|_| "DEV-WEBHOOK".into());
    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());

    let pool = PgPool::connect(&database_url).await?;
    autofirc_server::db::migrate(&pool).await?;

    let conn = apalis_redis::connect(redis_url).await?;
    let storage = RedisStorage::new(conn);

    let paypal = autofirc_paypal::client::PayPalClient::new(
        &std::env::var("PAYPAL_BASE_URL")
            .unwrap_or_else(|_| "https://api-m.sandbox.paypal.com".into()),
        &std::env::var("PAYPAL_CLIENT_ID").unwrap_or_default(),
        &std::env::var("PAYPAL_CLIENT_SECRET").unwrap_or_default(),
    );

    // enqueue sink used by the webhook route: push EnrichJob onto Redis.
    let mut push_storage = storage.clone();
    let pool_for_jobs = pool.clone();
    let enqueue: routes::EnqueueFn = Arc::new(move |event_id, body| {
        let mut storage = push_storage.clone();
        let capture_id = String::new(); // parsed again in worker path via DB row
        let _ = body;
        let _ = capture_id;
        Box::pin(async move {
            let job = EnrichJob { event_id, capture_id: String::new(), order_id: String::new() };
            if let Err(e) = storage.push(job).await {
                tracing::error!(error = %e, "failed to enqueue enrich job");
            }
        })
    });
    let _ = pool_for_jobs;

    let state = AppState {
        pool: pool.clone(),
        paypal_cert_http: reqwest::Client::new(),
        webhook_id,
        enqueue,
        pinned_webhook_key: None,
    };

    let worker_pool = pool.clone();
    tokio::spawn(async move {
        let paypal = paypal;
        Monitor::new()
            .register(
                WorkerBuilder::new("enrich")
                    .backend(storage)
                    .build_fn(move |job: EnrichJob| {
                        let paypal = paypal.clone();
                        let pool = worker_pool.clone();
                        async move { worker::enrich(job, &paypal, &pool).await }
                    }),
            )
            .run()
            .await
            .expect("worker monitor");
    });

    let app = routes::router(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "autofirc-server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
```

Note for the implementer: the webhook route currently parses `capture_id`/`order_id` and stores them on the row; **change the enqueue closure to fetch those two fields back** (or better: change `EnqueueFn` signature to take the full `EnrichJob`). Do the latter — in `routes.rs` replace
```rust
pub enqueue: Arc<dyn Fn(String, Bytes) -> futures_send + Send + Sync>,
```
with
```rust
pub enqueue: EnqueueFn,
pub type EnqueueFn = Arc<dyn Fn(crate::worker::EnrichJob) -> futures_send + Send + Sync>;
```
and in `paypal_webhook` build `EnrichJob { event_id: event.id, capture_id, order_id }` before `(state.enqueue)(job).await;`. Update Task 9's test closure signature to match (`move |_job: EnrichJob|`). Then in `main.rs` the closure becomes:
```rust
let enqueue: routes::EnqueueFn = Arc::new(move |job: EnrichJob| {
    let mut storage = push_storage.clone();
    Box::pin(async move {
        if let Err(e) = storage.push(job).await {
            tracing::error!(error = %e, "failed to enqueue enrich job");
        }
    })
});
```

- [ ] **Step 3: Add GET /api/ledger to routes.rs**

```rust
use axum::{Json, routing::get};

pub async fn list_ledger(State(state): State<AppState>) -> impl IntoResponse {
    match crate::db::list_ledger(&state.pool).await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```
and in `router()`:
```rust
.route("/api/ledger", get(list_ledger))
```

- [ ] **Step 4: Build + live smoke test**

Run: `cargo build -p autofirc-server`
Expected: compiles (fix apalis 0.7 API drift if any: consult `cargo doc -p apalis-redis --open` or the crate's examples; the constructor is `RedisStorage::new(conn)` and pushing is `storage.push(job)`).

Live smoke (uses `.env`):
```bash
set -a; . ./.env; set +a
./target/debug/autofirc-server & sleep 2
curl -s -o /dev/null -w '%{http_code}\n' -X POST localhost:8080/api/webhooks/paypal -d '{}'
```
Expected: `400` (missing headers — proves route is live; real webhooks need PayPal's signature).
Run: `cargo test --workspace`
Expected: all PASS except `#[ignore]`d db test; then `set -a; . ./.env; set +a; cargo test -p autofirc-server -- --ignored` → PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/autofirc-server
git commit -m "feat(server): redis enrich worker, binary wiring, ledger api"
```

---

### Task 11: `product.html` + docker-compose fallback

**Files:**
- Create: `product.html`
- Create: `docker-compose.yml`

- [ ] **Step 1: product.html**

Single self-contained file (inline CSS, no JS build), PayPal palette (#003087/#0070E0/#FFC439), sections: hero (name, pitch, prize targets), problem (PA-CB transition, FEMA 2026 9-month window, eBRC self-certification, Rule 96A/LUT, P0103 trap), 4-phase feature cards, swimlane flow diagram (Merchant / PayPal / AutoFIRC / Bank / DGFT — 15 steps from the spec §5 as an ordered list with actor tags), PayPal API table (spec §4), tech stack, personas, demo instructions summary. Full HTML written by implementer; requirement: opens standalone via `file://` with zero network deps.

- [ ] **Step 2: docker-compose.yml**

```yaml
services:
  postgres:
    image: postgres:17
    environment:
      POSTGRES_PASSWORD: autofirc
      POSTGRES_DB: autofirc
    ports: ["5433:5432"]
    volumes: [pgdata:/var/lib/postgresql/data]
  mailhog:
    image: mailhog/mailhog:v1.0.1
    ports: ["1025:1025", "8025:8025"]
volumes:
  pgdata:
```

- [ ] **Step 3: Verify + commit**

Open `product.html` in a browser (or `python3 -m http.server`); confirm all sections render.
```bash
git add product.html docker-compose.yml
git commit -m "feat: product page and local fallback services"
```

---

## Self-Review

- **Spec coverage:** Plan 1 covers spec §6 crates `autofirc-core`, `autofirc-paypal`, `autofirc-server` (ingestion half), §4 PayPal APIs (webhooks/OAuth2/Orders/Captures — Invoicing v2 deferred to the AI/documents plan's seeder), §7 testing, `product.html`, docker-compose. Deferred to later plans: `autofirc-ai`, `autofirc-pdf`, `web/`, `autofirc-dgft`, `autofirc-portal`, `autofirc-cli`, Render deploy, Zapier MCP.
- **Type consistency:** `EnqueueFn` is defined once in `routes.rs` and used in `main.rs` (Task 10 includes the required signature migration); `EnrichJob` fields match the webhook route's parsed fields; `TxnStatus` snake_case serde matches the SQL default `'pending_analysis'`.
- **Known drift risk:** apalis 0.7 builder API — Task 10 Step 4 instructs checking compiled examples if `WorkerBuilder`/`build_fn` differs; the queue *semantics* (Redis list push/pop) are unaffected.
