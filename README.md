# AutoFIRC — autonomous FEMA/eBRC reconciliation for PayPal exporters

**Built for the "Build What's Next with PayPal and AI" hackathon.**

AutoFIRC is a compliance agent for Indian freelancers and SMB exporters who
get paid through PayPal. Every foreign remittance into India creates a chain
of statutory obligations — RBI purpose-code reporting, GST export invoices
under LUT, the FEMA 9-month realization deadline, and eBRC reconciliation on
the DGFT portal. Today a $500 PayPal payment triggers ~45 days of manual
paperwork per transaction. AutoFIRC closes that loop end-to-end:

```
PayPal payment ──webhook──▶ ingest ──▶ AI classify (purpose code, SAC, GATS mode)
     │                                  guardrail backstop → manual review
     ▼
GST Rule 46 invoice + LUT PDFs ──▶ merchant dashboard (FEMA deadline alerts)
     │  merchant authorizes
     ▼
bank petition (e-mail with compliance packet) ──▶ IRM lands at DGFT
     │
     ▼
auto-reconcile: club IRMs, submit signed eBRC self-certification ──▶ ✅ fully reconciled
```

## Why it matters

- **FEMA risk is real**: unreconciled export receivables past the realization
  window put the exporter on the RBI/EDPMS caution list. AutoFIRC turns the
  statutory deadline into a first-class, color-coded alert system
  (green → amber at 7 months → red at 8).
- **AI with a safety net**: the LLM proposes the RBI purpose code, SAC code,
  and GATS mode of export — but a deterministic backstop vets every answer.
  Confidence < 85, or a P0103 (goods advance) on a transaction with no
  physical-goods signals, routes the row to manual review. The model can
  never force a misdeclaration through.
- **Agent-native**: everything the dashboard does is also a JSON-emitting CLI
  (`autofirc`) plus a packaged agent skill, so Claude Code, Kimi, OpenClaw,
  or Hermes can drive a merchant's compliance programmatically.
- **Cross-border by design**: PayPal India merchant accounts accept
  international payments only, so AutoFIRC enforces the same rule — a
  deterministic guard rejects domestic (Indian-payer) transactions before
  any classification or document work, and alerts the merchant.

## Architecture

Rust workspace (`crates/`):

| Crate | Responsibility |
|---|---|
| `autofirc-core` | Compliance rule engine: FEMA deadlines, IRM clubbing rules, purpose codes |
| `autofirc-paypal` | Webhook signature verification (CRC32+RSA), OAuth2 Orders/Captures client, mock server |
| `autofirc-ai` | Guardrail classifier (Nebius Token Factory/Gemini/OpenAI/Anthropic/offline), NL ledger query parser |
| `autofirc-pdf` | GST Rule 46 export invoice + LUT document generation |
| `autofirc-dgft` | DGFT Open API client (PBKDF2 handshake, RSA-signed eBRC) + high-fidelity mock with a dgft.gov.in-style browser portal UI |
| `autofirc-portal` | kernel.sh cloud-browser client: Playwright execution drives the portal UI (login → clubbing → PDF upload → eBRC scrape), live view + replay |
| `autofirc-server` | axum API + apalis workers (ingest → classify → documents → reconcile) |
| `autofirc-cli` | Agent-friendly CLI, JSON-first output |

Infrastructure: Postgres (ledger; Render Postgres in the demo), Redis-compatible
queue (apalis), S3-compatible object storage (generated PDFs, presigned
downloads).

Frontend: `web/` — React + TypeScript dashboard on **AG Grid** with a
natural-language filter bar (LLM → structured filter), FEMA deadline
coloring, pipeline stepper, and one-click bank petitions.

## Quick start (judges: 5-minute offline demo)

Requires: Rust (nightly/2024 edition), Node 20+, and the three infra
credentials in `.env` (see `.env.example`). Everything else — PayPal, DGFT,
the LLM — is mocked locally.

```bash
cp .env.example .env    # fill DATABASE_URL, REDIS_URL, OBJECT_STORAGE_*
cargo build --workspace --bins
./scripts/demo.sh       # boots mocks + server, simulates payments,
                        # petitions the bank, reconciles to eBRC
```

Then open the dashboard:

```bash
cd web && npm install && npm run dev   # http://localhost:5173
```

The demo drives five scenarios: high-confidence software export (auto),
ambiguous scope (→ manual review), goods advance with shipping signals
(→ P0103, passes the backstop), EUR design services, and a small audit —
ending with eBRC issuance and `fully_reconciled` rows.

## Running against the real PayPal sandbox

Full walkthrough: **[docs/paypal-sandbox-setup.md](docs/paypal-sandbox-setup.md)**
(app creation, webhook subscription, sandbox buyer accounts, tunnels, and the
PayPal MCP server). Short version:

1. Create a sandbox app at developer.paypal.com → set `PAYPAL_CLIENT_ID`,
   `PAYPAL_CLIENT_SECRET`, `PAYPAL_WEBHOOK_ID` in `.env` (and unset
   `DEV_WEBHOOK_KEY_PEM` so real cert verification applies).
2. Subscribe a webhook for `PAYMENT.CAPTURE.COMPLETED` pointing at
   `https://<public-host>/api/webhooks/paypal` (tunnel instructions in the
   guide; Render blueprint included).
3. `./scripts/sandbox-demo.sh`, then open **http://localhost:8080/checkout** —
   a demo store page with real PayPal JS SDK buttons. Pay with a personal
   sandbox account; the capture webhook drives the whole pipeline against
   real sandbox order/settlement data.

### Agentic commerce: PayPal MCP

The repo ships `.mcp.json.example` (rename to `.mcp.json` to activate) pointing
any MCP client (Claude Code, Kimi, Cursor) at PayPal's official sandbox MCP
server (`https://mcp.sandbox.paypal.com/sse`). Combined with AutoFIRC's own
CLI skill, an agent can drive both sides of the money: PayPal payment state
via MCP tools, compliance state via `autofirc`.

## CLI + agent skill

```bash
autofirc setup                      # connectivity + status summary
autofirc list --status manual_review
autofirc query unreconciled rows past the amber deadline
autofirc petition EV-... EV-...     # authorize bank petitions
autofirc documents EV-...           # presigned invoice/LUT URLs
autofirc watch                      # stream pipeline changes
```

`skills/autofirc/SKILL.md` packages this as an installable agent skill.

## Sponsor tools used

- **AG Grid** — the merchant ledger grid (theming, selection-gated petition
  flow, custom cell renderers).
- **Render** — deployment blueprint (`render.yaml`): server, workers, and the
  static dashboard as one Blueprint apply.
- **Zapier (MCP)** — the bank petition e-mail and merchant alert channel;
  `ZAPIER_MCP_URL` receives structured action payloads.
- **APIMatic** — used during development to explore/validate the PayPal API
  surface via the PayPal MCP/context plugins.
- **KERNEL** — `autofirc-portal` files eBRCs through a cloud browser driving
  the mock DGFT portal UI (a dgft.gov.in twin): Playwright execution over
  REST, invoice/LUT upload, and a live-view/replay URL stored on each ledger
  row (`DGFT_PORTAL_MODE=browser|fallback|api`).

## Testing

```bash
cargo test --workspace          # 48 tests: rules, crypto, backstop, routes
cd web && npm test              # 12 tests: FEMA deadline engine (TS mirror)
```

## License

MIT — see [LICENSE](LICENSE).
