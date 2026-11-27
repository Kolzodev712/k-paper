#!/usr/bin/env bash
# Compatibility wrapper — prefer the Rust example (no websocat required):
#   cargo run --example external_http_client
set -euo pipefail
ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"
exec cargo run --example external_http_client -- "$@"
