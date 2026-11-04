---
name: autofirc
description: Drive a merchant's AutoFIRC cross-border compliance pipeline (PayPal → FEMA/eBRC reconciliation) from the command line. Use when listing PayPal export payments, checking FEMA realization deadlines, filtering the ledger in natural language, authorizing bank petitions, or downloading GST invoice/LUT documents.
whenToUse: When the user asks about PayPal export receivables, FEMA/eBRC/FIRA compliance status, RBI purpose codes, or wants to authorize bank petitions or fetch GST documents from an AutoFIRC server.
---

# AutoFIRC agent skill

The `autofirc` CLI talks to a running AutoFIRC server and prints JSON on
stdout. Default server: `http://localhost:8080` (override with
`--base-url` or `AUTOFIRC_URL`). Add `--pretty` for human-readable output.

## Commands

- `autofirc setup` — connectivity check + row counts by pipeline status.
- `autofirc list [--status <status>]` — all ledger rows.
  Statuses, in order: `pending_analysis` → `enriched` → `compliance_packet_ready`
  → `petition_authorized` → `bank_petitioned` → `fully_reconciled`
  (side branches: `manual_review`; terminal rejection: `domestic_rejected`).
- `domestic_rejected` rows are terminal: the payer is Indian, and PayPal
  India merchant accounts accept international payments only. Never petition
  them; explain the rejection to the user instead.
- `autofirc get <event_id>` — one row plus presigned invoice/LUT URLs.
- `autofirc query <question...>` — natural-language filter
  ("unreconciled rows past the amber deadline", "software exports above
  90 confidence"). Returns the parsed filter and the matching rows.
- `autofirc petition <event_id...>` — authorize the bank petition +
  DGFT reconciliation worker for rows in `compliance_packet_ready`.
- `autofirc documents <event_id>` — presigned download URLs.
- `autofirc watch [--interval 5]` — stream row changes as JSON lines.

## Row fields worth knowing

`amount_inr_paise` is integer paise (÷100 for ₹). `invoice_date` is
`YYYY-MM-DD`; the FEMA realization deadline is invoice_date + 9 months
(+12 when `inr_vostro`), alert amber at ≤62 days remaining, red at ≤31.
`ai_confidence` < 85 means the row was routed to `manual_review` — never
petition those; surface them to the user instead.

## Agent rules

1. Only `petition` rows whose status is `compliance_packet_ready`; the
   server skips anything else (check the `skipped` array in the response).
2. `petition` is a money-adjacent action: list the affected rows to the
   user and confirm before running it.
3. If `query` prints "NL parser unavailable", the server has no LLM key —
   use the returned `text` fallback filter and say so.
4. Errors are non-zero exits with a message on stderr; parse stdout only
   on success.
