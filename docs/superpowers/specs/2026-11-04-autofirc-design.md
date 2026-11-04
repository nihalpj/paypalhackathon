# AutoFIRC v2.0 — Design Spec: Cross-Border Compliance Agent (Rust Edition)

Date: 2026-11-04 · Status: Approved · Source PRD: `TechinalPRD.md`

## 1. Problem

Indian digital-service exporters (freelancers, SMBs) receiving cross-border PayPal payments must manually: reconcile remittances against invoices, pick correct RBI FETERS purpose codes (P0103 misclassification creates unfulfillable EDPMS "ghost" obligations), generate GST Rule 46 invoices under LUT (Rule 96A: 1-year forex receipt deadline, 18% interest on default), petition AD banks for FIRA/FIRC, and self-certify eBRCs on the DGFT portal — now under FEMA 23(R)/2026-RB's compressed 9-month realization window (12 for INR/Vostro), in force 1 Oct 2026. Regulatory claims verified 2026-11-04 against RBI/DGFT primary sources (PA-CB circular 31 Oct 2023 + Master Direction 15 Sep 2025, ₹25 lakh/unit cap; Trade Notices 33/2023-24, 02/2025-26, 12/2024-25; GST details mandatory in eBRC w.e.f. 13 Jan 2026).

## 2. Product

AutoFIRC is an autonomous compliance agent bridging PayPal's rails and Indian regulation. It ingests PayPal webhooks, enriches transactions via PayPal APIs, uses an LLM to assign purpose codes / GATS export mode / SAC codes under strict guardrails, generates GST-compliant documents, tracks FEMA deadlines on a live dashboard, petitions the bank, and reconciles eBRCs via the DGFT API — or, when the API is unreachable, by operating the DGFT portal through a cloud browser agent.

Prize targets: Most Impactful, Best Use of PayPal + AI, Best Use of Agentic Commerce, Best Use of AG Grid.

## 3. Locked decisions

- **Rust core** (Cargo workspace), exposed as (a) Axum web backend + React/AG Studio dashboard and (b) `autofirc` CLI packaged as an agent skill for Claude Code / Kimi / OpenClaw.
- **AI**: `rig` (provider-agnostic open-source SDK). Default Gemini; `AI_PROVIDER` env switches providers.
- **Infra** (user-provided, in `.env`, gitignored): Supabase Postgres (Supavisor pooler 6543 runtime / direct 5432 migrations), **Upstash Redis (TLS) as the job queue** via `apalis` — substituted for QStash since Redis credentials were provided; the webhook-decoupling pattern is identical. Supabase Storage (S3-compatible, bucket `ai_checker`) for PDFs.
- **Sponsors (5, each fills a real need)**: AG Studio (dashboard), APIMatic Context Plugin (dev-time PayPal context), KERNEL (DGFT portal browser agent), Render (hosting/demo URL), Zapier MCP (bank-petition email + merchant alerts). Excluded: Bryntum, Postman, Elastic, Channel3, Astropods.

## 4. PayPal API surface

| Capability | API | Consumer |
|---|---|---|
| Auth | OAuth2 `POST /v1/oauth2/token` (client-credentials, cached) | `autofirc-paypal` |
| Invoicing | Invoicing v2 create-draft / send | Phase 1, demo seeder |
| Orders | Orders v2 create / show | Phase 2 enrichment (line items, buyer metadata) |
| Payments | `GET /v2/payments/captures/{id}` | INR settlement, FX rate, fees (FIRA packet) |
| Webhooks | `PAYMENT.CAPTURE.COMPLETED`; offline verification: raw-byte CRC32 + `transmission_id\|transmission_time\|webhook_id\|crc32` + RSA-SHA256 vs X.509 cert, `paypal.com` allowlist | Phase 1 ingestion |
| Webhook mgmt | `POST /v1/notifications/webhooks` | `autofirc setup` |
| Simulator | Sandbox webhook simulator / sandbox accounts | tests + demo |

## 5. User flow (4 phases)

**Phase 1 — Invoice & ingestion.** Merchant issues invoice (Invoicing v2) → foreign client pays on PayPal-hosted page → `PAYMENT.CAPTURE.COMPLETED` webhook → Axum route buffers raw bytes, verifies CRC32+RSA-SHA256 offline, inserts ledger row (webhook `id` PK = idempotency), enqueues to Redis, 200 OK < 2s → status `Pending Analysis`.

**Phase 2 — AI analysis & documents.** Worker: OAuth2 → Orders/Captures enrichment → `autofirc-ai` structured output `{purpose_code, mode_of_export, sac_code, confidence_score}`; hard rules: P0103 forbidden for services; remote digital services → Mode 1; P0802/P0803/P0807 by nomenclature (P0807 → SOFTEX warning). Confidence < 85 → `Manual Review` (purpose codes immutable, TBML guardrail). typst renders GST Rule 46 invoice (17 fields, LUT endorsement) + LUT declaration → Supabase Storage signed URLs → `Compliance Packet Ready`.

**Phase 3 — Dashboard & agentic command.** AG Studio ledger + custom widgets: FEMA countdown (amber 7mo / red 8mo), eBRC tiles, purpose-code chart. Studio Agent Framework conversational bar: NL → filter/layout → stage rows → **Authorize Bank Petition** (bulk). CLI mirrors all actions.

**Phase 4 — Bank petition, DGFT & reconciliation.** Email packet (invoice + LUT + capture receipt as FIRA) dispatched via Zapier MCP (SMTP/Mailhog fallback) to bank officer → EDPMS closure, IRMs to DGFT. `autofirc-dgft`: PBKDF2 (SHA-256, 65,536 iters, 32-byte salt) token handshake → fetch IRMs → clubbing rules (same bank/account/currency; advance ⇒ invoice date ≥ remittance date) → RSA-SHA256-signed eBRC payload (incl. Mode of Export + GST invoice details) → eBRC number → `Fully Reconciled & Compliant`. Failures → `DGFT Retry` with backoff. Demo: built-in mock DGFT server; KERNEL browser agent covers the real-portal path with live replay.

## 6. Architecture

```
crates/
  autofirc-core    # ledger models, purpose codes, Rule-46 fields, FEMA deadline engine,
                   # DGFT clubbing rules, status machine
  autofirc-ai      # rig classifier, guardrail prompt, confidence gate, AI_PROVIDER env
  autofirc-paypal  # webhook verify, OAuth2, Orders/Invoicing/Payments clients, rig tools
  autofirc-dgft    # PBKDF2 + RSA-SHA256 client, IRM match, eBRC submit, mock server
  autofirc-portal  # KERNEL browser-agent fallback for DGFT portal UI
  autofirc-pdf     # typst GST invoice + LUT PDFs
  autofirc-server  # Axum: webhook route, queue consumers, REST/WS API, Zapier MCP actions
  autofirc-cli     # `autofirc` binary + SKILL.md agent-skill packaging
web/               # Vite + React + TS: AG Studio dashboard, widgets, NL bar
product.html       # standalone product/flow page
docker-compose.yml # local Postgres + Mailhog fallback
```

Key NFRs: AES-256 at rest / TLS 1.3 in transit; webhook route < 2s (verify + enqueue); AI+PDF 4–12s fully async; idempotency on webhook id; LLM failure → `Manual Review`; secrets via env only.

## 7. Testing

- `cargo test`: core rule engines (FEMA windows, clubbing, purpose-code tables), crypto (CRC32/RSA-SHA256 fixture, PBKDF2 vector), PDF field completeness.
- `wiremock` integration: PayPal APIs, mock DGFT round-trip, Zapier MCP stub.
- E2E script: sandbox/simulator payment → all four statuses → eBRC; same via CLI skill.
- `cargo clippy -- -D warnings`, `cargo fmt --check`, `npm run build` in `web/`.

## 8. Build order

1. Spec + commit (this file). 2. `product.html` + workspace scaffold + CI. 3. `autofirc-core` + tests. 4. `autofirc-paypal` + fixtures. 5. `autofirc-server` ingestion → Redis → DB. 6. `autofirc-ai`. 7. `autofirc-pdf` + storage. 8. `web/` AG Studio dashboard. 9. `autofirc-dgft` + mock + `autofirc-portal` + Zapier MCP actions. 10. `autofirc-cli` + skill. 11. Render deploy. 12. Demo polish (seed data, README, MIT license, <3-min script).

## 9. Assumptions

- Real DGFT unreachable (IEC/IP whitelist) → production-faithful client + mock; KERNEL covers real portal.
- AG Studio on sponsor 45-day trial (dev-time only).
- PayPal sandbox only; production URLs behind config.
- Redis (provided) replaces QStash; queue semantics preserved via `apalis`.
- Storage bucket `ai_checker` is reused; objects namespaced under `autofirc/` prefix.
- docker-compose provides Postgres + Mailhog for fully-local judge runs.
