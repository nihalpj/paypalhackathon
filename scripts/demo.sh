#!/usr/bin/env bash
# AutoFIRC offline demo: drives the entire pipeline with zero external
# dependencies except the database, queue, and object storage from .env.
#
#   mock-paypal  → PayPal REST API (orders, captures, INR settlement)
#   mock-dgft    → DGFT Open API (PBKDF2 handshake, IRMs, eBRC)
#   autofirc-sim → signed demo webhook traffic
#
# Scenario suffixes: 1=software (auto), 2=ambiguous (manual review),
# 3=goods (P0103), 4=design EUR, 9=DOMESTIC TRAP (rejected, Indian payer),
# anything else=small audit.
#
# Usage: ./scripts/demo.sh [--count N]
set -euo pipefail
cd "$(dirname "$0")/.."

COUNT="${1:-5}"
[ "${1:-}" = "--count" ] && COUNT="${2:-5}"

cargo build --workspace --bins 2>/dev/null

KEY_FILE=/tmp/autofirc-dev-key.pem
[ -f "$KEY_FILE" ] || ./target/debug/autofirc-sim --keygen > "$KEY_FILE"

export DEV_WEBHOOK_KEY_PEM="$KEY_FILE"
export PAYPAL_WEBHOOK_ID=WH-DEMO
export PAYPAL_BASE_URL="${PAYPAL_BASE_URL:-http://127.0.0.1:4011}"
export DGFT_BASE_URL="${DGFT_BASE_URL:-http://127.0.0.1:4010}"
export AI_PROVIDER=offline
export BIND_ADDR="${BIND_ADDR:-127.0.0.1:8080}"
API="http://$BIND_ADDR"
export AUTOFIRC_URL="$API"

PIDS=()
cleanup() { kill "${PIDS[@]}" 2>/dev/null || true; }
trap cleanup EXIT

echo "▸ starting mocks + server ($BIND_ADDR)"
MOCK_PAYPAL_ADDR=127.0.0.1:4011 ./target/debug/mock-paypal & PIDS+=($!)
MOCK_DGFT_ADDR=127.0.0.1:4010   ./target/debug/mock-dgft   & PIDS+=($!)
./target/debug/autofirc-server & PIDS+=($!)

for i in $(seq 1 30); do
  curl -sf -o /dev/null $API/api/ledger && break
  sleep 1
done

echo "▸ simulating $COUNT inbound PayPal payments"
./target/debug/autofirc-sim --count "$COUNT" --base-url "$API"

echo "▸ waiting for the pipeline (classify → GST packet)"
for i in $(seq 1 30); do
  READY=$(./target/debug/autofirc list --status compliance_packet_ready | grep -c '"id"' || true)
  [ "$READY" -ge 1 ] && break
  sleep 2
done
./target/debug/autofirc --pretty list

echo "▸ authorizing bank petitions"
IDS=$(./target/debug/autofirc list --status compliance_packet_ready \
  | grep -o '"id":"EV-[^"]*"' | cut -d'"' -f4 | tr '\n' ' ')
# shellcheck disable=SC2086
./target/debug/autofirc petition $IDS

echo "▸ waiting for DGFT reconciliation (eBRC issuance)"
for i in $(seq 1 30); do
  DONE=$(./target/debug/autofirc list --status fully_reconciled | grep -c '"id"' || true)
  [ "$DONE" -ge 1 ] && break
  sleep 2
done

echo
echo "▸ final ledger"
./target/debug/autofirc --pretty list
echo
echo "✓ demo complete — dashboard: cd web && npm run dev"
