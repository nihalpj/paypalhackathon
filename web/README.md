# AutoFIRC merchant dashboard

Vite + React + TypeScript control surface for the AutoFIRC compliance agent. Shows the
payment ledger, AI classification (RBI purpose code, SAC, GATS mode), FEMA realization
deadlines, and drives the bank-petition / DGFT reconciliation flow.

## Run it

```bash
npm install
npm run dev     # dev server on http://localhost:5173, proxies /api -> http://localhost:8080
npm test        # vitest — FEMA deadline/alert unit tests
npm run build   # type-check + production build
```

The Rust backend must be running on `http://localhost:8080` (see the server crate).

## Features

- **Ledger grid** (AG Grid, dark quartz theme): client, INR amount (paise → rupees,
  Indian digit grouping), invoice date, purpose code, SAC, AI confidence bar
  (≥85 green / 60–84 amber / <60 red), FEMA deadline, alert badge, pipeline status
  chip, eBRC number. Rows in `compliance_packet_ready` are checkbox-selectable.
- **Natural-language search**: POSTs to `/api/query` and applies the returned filter
  client-side; falls back to plain text search with a note if no LLM key is configured.
- **Petition bank (n)**: authorizes the bank petition + DGFT reconciliation worker for
  all selected packet-ready rows, then refetches.
- **Row drawer**: full field detail, backstop note, presigned invoice/LUT downloads
  (graceful when storage isn't configured), and a 7-step pipeline stepper.
- **FEMA alerts**: deadline = invoice date + 9 months (12 for INR vostro accounts);
  reconciled rows are green, otherwise red ≤31 days, amber ≤62 days, green beyond.
  Logic lives in `src/lib/fema.ts` with unit tests in `src/lib/fema.test.ts`.
- Ledger polls every 15 s; skeleton loading state, empty state, and an error banner
  with retry.
