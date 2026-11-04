#!/usr/bin/env bash
# AutoFIRC sandbox demo: runs the pipeline against the REAL PayPal sandbox.
# Prerequisites: docs/paypal-sandbox-setup.md (app creds + webhook + tunnel).
set -euo pipefail
cd "$(dirname "$0")/.."

for var in PAYPAL_CLIENT_ID PAYPAL_CLIENT_SECRET PAYPAL_WEBHOOK_ID; do
  if [ -z "${!var:-}" ]; then
    echo "✗ $var not set — see docs/paypal-sandbox-setup.md" >&2
    exit 1
  fi
done

cargo build --workspace --bins 2>/dev/null

export BIND_ADDR="${BIND_ADDR:-127.0.0.1:8080}"
export DGFT_BASE_URL="${DGFT_BASE_URL:-http://127.0.0.1:4010}"
export PAYPAL_BASE_URL="${PAYPAL_BASE_URL:-https://api-m.sandbox.paypal.com}"
API="http://$BIND_ADDR"
export AUTOFIRC_URL="$API"

PIDS=()
cleanup() { kill "${PIDS[@]}" 2>/dev/null || true; }
trap cleanup EXIT

echo "▸ starting mock DGFT (4010) + server ($BIND_ADDR) against PayPal sandbox"
MOCK_DGFT_ADDR=127.0.0.1:4010 ./target/debug/mock-dgft & PIDS+=($!)
./target/debug/autofirc-server & PIDS+=($!)

for i in $(seq 1 30); do
  curl -sf -o /dev/null "$API/api/ledger" && break
  sleep 1
done

cat <<EOF

✓ AutoFIRC is live against the PayPal sandbox.

  1. Expose this server (if PayPal can't reach you yet):
       ssh -R 80:localhost:${BIND_ADDR##*:} nokey@localhost.run
     …and make sure the webhook in the developer portal points at
       https://<public-host>/api/webhooks/paypal  (events: PAYMENT.CAPTURE.COMPLETED)

  2. Open the demo store:  $API/checkout
     Pay with a PERSONAL sandbox account (developer portal → Sandbox → Accounts).

  3. Watch the pipeline:   autofirc watch
     or the dashboard:    cd web && npm run dev

EOF

# Keep everything up; stream ledger changes until Ctrl-C.
./target/debug/autofirc watch --interval 3
