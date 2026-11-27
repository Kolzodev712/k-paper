# k-paper

Standalone **paper exchange**: synthetic or scripted market data, order lifecycle, simple matching, single in-memory account, HTTP + WebSocket APIs. Use it as a library, as the `k-paper` server process, or in Docker — **without** the `trading_system` repo.

- **License:** Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
- **Copyright:** © 2025–2026 T.L.P. Arc Entertainment Ltd.
- **Design:** [proposals/paper-exchange-v1.md](proposals/paper-exchange-v1.md)
- **Crates:** `paper-exchange-core`, `paper-exchange-server`, `paper-exchange-scenarios`; root package builds the `k-paper` binary.

## Boundaries

Controlled laboratory venue — **not** Bybit API parity, **not** production/mainnet. Restarting the server **drops** the in-memory account. Increment status: [docs/PROP_TRADING_INTEGRATION_GUIDE.md](docs/PROP_TRADING_INTEGRATION_GUIDE.md) and [trading_system PROGRESSION](https://github.com/Kolzodev712/trading_system/blob/main/docs/PROGRESSION.md).

## Install and run (library / binary)

```bash
cargo build
cargo run                    # k-paper server (delegates to paper-exchange-server)
cargo run -p paper-exchange-server
cargo test -p paper-exchange-core
cargo test --all -- --test-threads=1
```

**Dev profile** (synthetic orderbook, loopback):

```bash
export PAPER_EXCHANGE_PROFILE=dev
export PAPER_EXCHANGE_PORT=19999   # default
cargo run
# Market: ws://127.0.0.1:19999/ws/market
# Trade:  ws://127.0.0.1:19999/ws/trade
```

Full run modes, YAML, and scenarios: **[docs/DEV-GUIDE.md](docs/DEV-GUIDE.md)**.

## Docker (loopback)

```bash
docker compose up --build
```

Image listens on **`127.0.0.1:19999`** (see [docker-compose.yml](docker-compose.yml), [Dockerfile](Dockerfile)): `PAPER_EXCHANGE_DUMMY_OB=1`, config under `/app/config`. Override mount/env for custom `instruments.yaml`.

## HTTP / WebSocket

| Surface | Doc |
| --- | --- |
| Endpoint map | [docs/CLIENT-INSTRUCTIONS.md](docs/CLIENT-INSTRUCTIONS.md) |
| Trading WS | [docs/API-EXECUTION.md](docs/API-EXECUTION.md) |
| Market WS | [docs/API-MARKET-DATA.md](docs/API-MARKET-DATA.md) |

Command replies are sent on the **same WebSocket** that submitted the command (`/ws/trade`). REST (`/balances`, `/open_orders`, `/exchange_info`, …) is for bootstrap and reconciliation.

**Minimal external client (no trading_system):**

```bash
# Server must be running (dev profile)
./examples/external_http_client.sh
```

## Scenarios and faults

- **Timed overlays:** `PAPER_EXCHANGE_SCENARIO` presets — table in [docs/DEV-GUIDE.md](docs/DEV-GUIDE.md).
- **Static JSON:** `config/scenario_01.json`; `cargo run --example scripted_scenario`.
- **Lab faults** (`fail_open_orders`, dropped acks, partial fills, …): HTTP hooks on the server — **documented in [docs/DEV-GUIDE.md](docs/DEV-GUIDE.md)** and cross-repo increment notes; do not duplicate fault semantics here.

**Reset faults:** `POST /test/reset_lab_faults` (see DEV-GUIDE / PROP integration guide).

## Examples

| Example | Command |
| --- | --- |
| Scripted scenario (in-process) | `cargo run --example scripted_scenario` |
| Procedural OB demo | `cargo run --example procedural_demo` |
| REST + WS smoke | `./examples/external_http_client.sh` |

## Limitations

- Single paper account; user orders do not form a multi-party CLOB.
- Matching uses the **injected** book, not full central-limit semantics.
- Linear USDT perp semantics require explicit `instrument_type` + `linear` fixture in YAML.
- No durable ledger across process restart.

## Integrated lab

For runner + UI + k-paper together, use [trading-system-showcase/lab](../trading-system-showcase/lab/README.md) (sibling checkout).