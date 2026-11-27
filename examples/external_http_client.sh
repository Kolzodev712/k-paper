#!/usr/bin/env bash
# Minimal k-paper client: REST bootstrap + one place_order over WebSocket.
# No trading_system dependency. Requires: curl, and websocat (https://github.com/vi/websocat).
set -euo pipefail

HOST="${PAPER_HOST:-127.0.0.1}"
PORT="${PAPER_PORT:-19999}"
BASE="http://${HOST}:${PORT}"
WS="ws://${HOST}:${PORT}/ws/trade"

if ! command -v curl >/dev/null; then
  echo "curl is required" >&2
  exit 1
fi
if ! command -v websocat >/dev/null; then
  echo "websocat is required for place_order (install from https://github.com/vi/websocat)" >&2
  exit 1
fi

echo "== exchange_info =="
curl -fsS "${BASE}/exchange_info" | head -c 400
echo

echo "== balances (before) =="
curl -fsS "${BASE}/balances"
echo

PLACE='{"type":"place_order","request":{"instrument_id":"BTC-USDT","side":"Buy","order_type":"Limit","quantity":"0.001","limit_price":"50000","client_order_id":"ext-demo-1"}}'
echo "== place_order via ${WS} =="
# Read until we see execution or error (server may also send balance/position on connect).
if ! printf '%s\n' "$PLACE" | websocat -n1 -t "$WS" | tee /tmp/k-paper-ws-last.json | grep -E '"type"\s*:\s*"(execution|error)"' >/dev/null; then
  echo "no execution/error on trade socket" >&2
  exit 1
fi

echo "== open_orders =="
curl -fsS "${BASE}/open_orders?instrument_id=BTC-USDT"
echo

echo "== balances (after) =="
curl -fsS "${BASE}/balances"
echo

echo "OK: external client finished"