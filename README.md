# k-paper

Standalone paper exchange: orderbook, order lifecycle, matching, account state, and streams. Usable as an in-process library or through its HTTP/WebSocket server.

- **License:** Proprietary. See [LICENSE](LICENSE).
- **Design:** [proposals/paper-exchange-v1.md](proposals/paper-exchange-v1.md) — preliminary design plan (v1); implement from there.
- **Layout:** Workspace with `paper-exchange-core` (library), `paper-exchange-scenarios`, and `paper-exchange-server`.

## Current scope and prop-trading work

The core, HTTP/WebSocket server, market scenarios, **one-way USDT-linear BTCUSDT fixtures**, client-id lookup, and Increment 9A lab fault hooks (including `fail_open_orders`) exist. Increment 9A is accepted locally as the Paper qualification venue. Increment 10 (Bybit Demo) does not change this server.

This is a controlled laboratory, not Bybit API parity and not production/mainnet. Candidate #2 remains an operational baseline, not validated alpha. Paper process restart still loses the in-memory account.

For Candidate #2 work, follow [docs/PROP_TRADING_INTEGRATION_GUIDE.md](docs/PROP_TRADING_INTEGRATION_GUIDE.md) together with the [primary trading-system roadmap](https://github.com/Kolzodev712/trading_system/blob/master/docs/PROP_TRADING_RESEARCH_AND_PRODUCTION_PLAN.md). Spot/levels `sample_config/k-paper` remains a connectivity smoke path; linear C2→Paper is the Increment 9A qualification, not that smoke path.

## Build and run

```bash
cargo build
cargo run              # run main system binary (k-paper)
cargo run -p paper-exchange-server   # same, via server crate
cargo test -p paper-exchange-core
cargo test --all -- --test-threads=1
```

The main executable is **k-paper** at the repo root (`src/main.rs`). It delegates to the server; `cargo run` starts the paper exchange.

**Documentation:** [docs/DEV-GUIDE.md](docs/DEV-GUIDE.md) — how to run (lab vs dev), configure YAML, scenarios, REST/WS integration, and pitfalls. The [prop-trading integration guide](docs/PROP_TRADING_INTEGRATION_GUIDE.md) is the Cursor handoff. API message schemas: [docs/CLIENT-INSTRUCTIONS.md](docs/CLIENT-INSTRUCTIONS.md), [docs/API-EXECUTION.md](docs/API-EXECUTION.md), [docs/API-MARKET-DATA.md](docs/API-MARKET-DATA.md).

## Crates

| Crate | Role |
|-------|------|
| **k-paper** (root) | Main system binary; runs the server. |
| `paper-exchange-core` | Orderbook, orders, fill model, account, streams; no I/O beyond channels. |
| `paper-exchange-server` | Wraps core; HTTP/WebSocket server and sessions. |
| `paper-exchange-scenarios` | Timed market-price overlays and presets. |
