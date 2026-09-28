# k-paper

Standalone paper exchange: orderbook, order lifecycle, matching, account state, and streams. Usable as an in-process library or through its HTTP/WebSocket server.

- **License:** Proprietary. See [LICENSE](LICENSE).
- **Design:** [proposals/paper-exchange-v1.md](proposals/paper-exchange-v1.md) — preliminary design plan (v1); implement from there.
- **Layout:** Workspace with `paper-exchange-core` (library), `paper-exchange-scenarios`, and `paper-exchange-server`.

## Current scope and prop-trading work

The core, HTTP/WebSocket server, market scenarios and basic integration tests exist. The account/fill behavior is spot-style; the checked-in server is **not yet** a linear-perpetual futures simulator or a deterministic exchange/transport fault lab.

For Candidate #2 work, follow [docs/PROP_TRADING_INTEGRATION_GUIDE.md](docs/PROP_TRADING_INTEGRATION_GUIDE.md) together with the [primary trading-system roadmap](https://github.com/Kolzodev712/trading_system/blob/master/docs/PROP_TRADING_RESEARCH_AND_PRODUCTION_PLAN.md). That guide separates the existing Spot connectivity smoke test from the future one-way futures, recovery and fault-injection qualification gates.

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
