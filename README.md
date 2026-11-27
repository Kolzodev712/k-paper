# k-paper

Standalone paper exchange: orderbook, order lifecycle, matching, account state, and streams. Usable as an in-process library or through its HTTP/WebSocket server.

- **License:** Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
- **Copyright:** © 2025–2026 T.L.P. Arc Entertainment Ltd.
- **Design:** [proposals/paper-exchange-v1.md](proposals/paper-exchange-v1.md) — preliminary design plan (v1); implement from there.
- **Layout:** Workspace with `paper-exchange-core` (library), `paper-exchange-scenarios`, and `paper-exchange-server`.

## Current scope

The core, HTTP/WebSocket server, market scenarios, **one-way USDT-linear BTCUSDT fixtures**, client-id lookup, and lab fault hooks (including `fail_open_orders`) exist. This is a controlled laboratory, not Bybit API parity and not production/mainnet. Paper process restart still loses the in-memory account.

Increment status and next gates live in the [trading_system progression doc](https://github.com/Kolzodev712/trading_system/blob/main/docs/PROGRESSION.md). This repo's increment notes: [docs/PROP_TRADING_INTEGRATION_GUIDE.md](docs/PROP_TRADING_INTEGRATION_GUIDE.md). Spot/levels smoke (`sample_config/k-paper` in the runner) is not the linear Candidate #2 Paper lab.

## Build and run

```bash
cargo build
cargo run              # run main system binary (k-paper)
cargo run -p paper-exchange-server   # same, via server crate
cargo test -p paper-exchange-core
cargo test --all -- --test-threads=1
```

The main executable is **k-paper** at the repo root (`src/main.rs`). It delegates to the server; `cargo run` starts the paper exchange.

**Documentation:** [docs/DEV-GUIDE.md](docs/DEV-GUIDE.md) — how to run (lab vs dev), configure YAML, scenarios, REST/WS integration, and pitfalls. Increment notes: [prop-trading integration guide](docs/PROP_TRADING_INTEGRATION_GUIDE.md). API message schemas: [docs/CLIENT-INSTRUCTIONS.md](docs/CLIENT-INSTRUCTIONS.md), [docs/API-EXECUTION.md](docs/API-EXECUTION.md), [docs/API-MARKET-DATA.md](docs/API-MARKET-DATA.md).

## Crates

| Crate | Role |
|-------|------|
| **k-paper** (root) | Main system binary; runs the server. |
| `paper-exchange-core` | Orderbook, orders, fill model, account, streams; no I/O beyond channels. |
| `paper-exchange-server` | Wraps core; HTTP/WebSocket server and sessions. |
| `paper-exchange-scenarios` | Timed market-price overlays and presets. |
