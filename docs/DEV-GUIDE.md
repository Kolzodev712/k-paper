# Developer guide — k-paper

How to run, configure, and integrate against the paper exchange server. For wire-level message formats, see [CLIENT-INSTRUCTIONS.md](CLIENT-INSTRUCTIONS.md), [API-EXECUTION.md](API-EXECUTION.md), and [API-MARKET-DATA.md](API-MARKET-DATA.md).

For increment status and next gates, see [PROP_TRADING_INTEGRATION_GUIDE.md](PROP_TRADING_INTEGRATION_GUIDE.md) and the [trading_system progression doc](https://github.com/Kolzodev712/trading_system/blob/master/docs/PROGRESSION.md). This guide describes how to run the server as it exists (Spot smoke and linear lab).

---

## What you are running

k-paper is a **single-account paper exchange**: one in-memory orderbook per instrument, simple price-cross matching, balance reservation, and WebSocket + REST APIs. It is meant as a **test double** for trading systems—not a production venue.

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────────┐
│ Market engine│────▶│ paper-exchange-  │────▶│ WebSocket broadcast │
│ (N pairs)   │     │ server (axum)    │     │ + REST queries      │
└─────────────┘     │        │         │     └─────────────────────┘
┌─────────────┐     │        ▼         │
│ Scenario    │────▶│ PaperExchange    │
│ overlay     │     │ (core library)   │
└─────────────┘     └──────────────────┘
```

- **Core** (`paper-exchange-core`): matching, balances, orders, no network I/O.
- **Server** (`paper-exchange-server`): loads YAML config, multi-instrument market engine, optional scenario overlay, exposes HTTP/WS.
- **Scenarios** (`paper-exchange-scenarios`): timed effects (`Drop`, `Rally`) that modulate the baseline engine.

---

## Quick start

### Build and run

```bash
cargo build
cargo run -p paper-exchange-server
# or from repo root:
cargo run
```

The process prints the bound address and URLs, for example:

```
paper exchange listening on 127.0.0.1:54321
Paper exchange URL: ws://127.0.0.1:54321/ws/trade
REST: http://127.0.0.1:54321/balances
```

### Dev mode (interactive, random orderbook)

Best for local development and manual testing.

```bash
export PAPER_EXCHANGE_PROFILE=dev
export PAPER_EXCHANGE_PORT=19999   # default if unset; override only when needed
cargo run -p paper-exchange-server
```

- Market data: `ws://127.0.0.1:19999/ws/market`
- Trading: `ws://127.0.0.1:19999/ws/trade`

Equivalent legacy flag: `export PAPER_EXCHANGE_DUMMY_OB=1`.

### Scenario overlay (baseline + shock)

Continuous random OB on **all instruments** in `instruments.yaml`, with timed effects (e.g. BTC drop at 30s):

```bash
export PAPER_EXCHANGE_PROFILE=dev
export PAPER_EXCHANGE_SCENARIO=flash_drop_v1
export PAPER_EXCHANGE_SEED=42
export PAPER_EXCHANGE_PORT=19999
cargo run -p paper-exchange-server
```

**Named presets** (`PAPER_EXCHANGE_SCENARIO=<name>`):

| Preset | Mechanics (what to test) |
|--------|---------------------------|
| `flash_drop_v1` | Single sharp BTC selloff (~5% over 10 ticks @ 30s) |
| `btc_rally_v1` | Single BTC rally (~3% @ 15s) |
| `dead_cat_bounce_v1` | Drop ~8% then partial rally ~4% — dead-cat bounce / re-risk timing |
| `risk_off_v1` | Staggered drops BTC → ETH → SOL — correlated risk-off / hedging |
| `bull_trap_v1` | Rally then reversal drop — failed breakout / chase-and-stop |

Or a timeline file with `"at_ms"` fields: `config/scenarios/flash_drop_timeline.json`.

The market engine **keeps running**; overlay adjusts effective mid for the affected pair over `over_ticks`.

### Lab static scenario (legacy fixtures)

One-shot JSON at startup (`"t":"orderbook"` events, no `at_ms`) — **no** market engine unless `PAPER_EXCHANGE_DUMMY_OB=1`:

```bash
export PAPER_EXCHANGE_PROFILE=lab
export PAPER_EXCHANGE_SCENARIO=config/scenario_01.json
cargo run -p paper-exchange-server
```

For **resting orders** (cancel/amend tests), set in `config/instruments.yaml`:

```yaml
fill_model:
  kind: scripted   # no automatic fill when limit crosses injected book
```

With `immediate_cross` (default), aggressive limits fill as soon as the book crosses your price.

The checked-in `config/instruments.yaml` includes a `market` section. With **no static scenario**, `runtime.rs` currently enables its market engine from that config even if `PAPER_EXCHANGE_PROFILE=lab`. For static-only deterministic tests, supply a static scenario and leave `PAPER_EXCHANGE_DUMMY_OB` off; verify the startup log says `market_engine=false`. Profile name alone is not a determinism guarantee.

---

## Runtime environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PAPER_EXCHANGE_CONFIG` | `config/instruments.yaml` | Path to exchange YAML (also searched next to crate if cwd has no file) |
| `PAPER_EXCHANGE_PROFILE` | `lab` if no dummy OB; `dev` if `PAPER_EXCHANGE_DUMMY_OB=1` | `lab` or `dev` |
| `PAPER_EXCHANGE_DUMMY_OB` | off | `1` or `true` — enable random-walk orderbook (implies dev) |
| `PAPER_EXCHANGE_SCENARIO` | — | Preset name (`flash_drop_v1`), overlay timeline JSON (`at_ms`), or static fixture JSON |
| `PAPER_EXCHANGE_SCENARIO_INTERVAL_MS` | `0` | Sleep between scenario events (ms) |
| `PAPER_EXCHANGE_SEED` | — | RNG seed for dummy OB (overrides YAML `dummy_ob.seed`) |
| `PAPER_EXCHANGE_HOST` | `127.0.0.1` | Bind host |
| `PAPER_EXCHANGE_PORT` | `19999` | Bind port (same every run unless overridden) |
| `PAPER_LAB_FAULTS` | off | `1` or `true` — mount the `/test/*` lab fault endpoints (404 otherwise). Set in Docker via `environment:` if the runner needs them |

Logging: standard `RUST_LOG` / tracing, e.g. `RUST_LOG=paper_exchange_server=debug`.

---

## Configuration file (`config/instruments.yaml`)

Loaded once at startup into core `Config`.

| Section | Purpose |
|---------|---------|
| `instruments` | Symbols, tick/lot/min notional, base/quote assets |
| `initial_balances` | Starting paper balances per asset |
| `fill_model` | How open orders match against the injected book |
| `max_trade_history` | Cap on in-memory fills exposed via REST `/trades` |
| `dummy_ob` | Parameters for dev profile random-walk book |

### Fill model kinds

| `kind` | Behavior |
|--------|----------|
| `immediate_cross` | Limit buy fills when best ask ≤ limit; limit sell when best bid ≥ limit (default) |
| `trade_through` | **Not implemented** — config load fails with an error |
| `scripted` | **No** automatic book-cross fills; use scenario injections or manual OB updates |

Other fields: `slippage_bps`, `fee_bps`, `partial_fills`. `partial_fills: true` is **not implemented** and is rejected at config load (use the `/test/partial_next_fill` lab hook for partial-fill scenarios).

### Dummy OB (dev)

Under `market:` (legacy `dummy_ob:`): `initial_mid` (per instrument), `step_ticks`, `spread_half_ticks`, `levels`, `interval_ms`, `seed`, `trade_probability`, `trade_max_qty`, `base_qty_per_level`.

`step_ticks` (max random-walk move per interval) and `spread_half_ticks` are in **ticks of each instrument** and every emitted price is tick-aligned. Set them globally or per entry under `instruments:`; when omitted they default per instrument to ~20 bps / ~10 bps of `initial_mid`. The old absolute `step` / `spread_half` keys are rejected at startup.

---

## Scripted scenarios

JSON file with an `events` array. Applied in order at server startup (async task).

| Event `t` | Fields | Effect |
|-----------|--------|--------|
| `orderbook` | `instrument_id`, `bids`, `asks` — each level `[price, qty]` strings | Replaces book; may trigger matching |
| `trade` | `instrument_id`, `price`, `quantity`, `side` | Public trade + matching pass |
| `place_order` | `instrument_id`, `side`, `quantity`, `limit_price` or `price`, optional `order_type`, `client_order_id` | Places order on paper account |

Example: [config/scenario_01.json](../config/scenario_01.json).

Standalone example (no server): `cargo run --example scripted_scenario`.

---

## Client integration

### WebSocket paths

See [CLIENT-INSTRUCTIONS.md](CLIENT-INSTRUCTIONS.md).

**Rule:** All command replies (`execution`, `CancelAck`, `error`, etc.) are sent on the **same WebSocket** that sent the command. Use `/ws/trade` for trading.

### REST reconciliation

Use HTTP on the same host/port as the server.

```bash
curl -s http://127.0.0.1:19999/balances
curl -s 'http://127.0.0.1:19999/open_orders?instrument_id=BTC-USDT'
curl -s 'http://127.0.0.1:19999/trades?limit=100'
curl -s http://127.0.0.1:19999/exchange_info
```

| Endpoint | Response |
|----------|----------|
| `GET /balances` | Array of `{ asset, available, locked, total }` (core shape) |
| `GET /open_orders` | CamelCase DTOs: `orderId`, `instrumentId`, `amount`, `filledAmount`, `status`, … |
| `GET /trades` | Fill history: `tradeId`, `orderId`, `price`, `quantity`, … |
| `GET /exchange_info` | `serverTime`, `symbols[]` with tick/lot/min notional |

Bootstrap pattern for a bot:

1. `GET /balances` + `GET /open_orders` + `GET /exchange_info`
2. Connect `ws://…/ws/trade` → receive snapshot balances/positions
3. Connect `ws://…/ws/market` if you need live book
4. Periodically re-run REST to correct drift

### Place order (minimal)

```json
{
  "type": "place_order",
  "request": {
    "instrument_id": "BTC-USDT",
    "side": "Buy",
    "order_type": "Limit",
    "quantity": "0.1",
    "limit_price": "48000",
    "client_order_id": "my-order-1"
  }
}
```

- `client_order_id`: string or `null`. Duplicate ids are **rejected** with `error`.
- Aggressive buy at `50000` in **dev** often fills immediately (best ask ≤ limit) → cancel may return `"order already filled"`.

### Common pitfalls

| Symptom | Cause | Mitigation |
|---------|--------|------------|
| `place_order` hangs / times out | Wrong endpoint or not reading same WS | Use `/ws/trade`; read loop on same connection |
| Cancel `"order already filled"` | Limit crossed dummy book | Lower buy limit / use `scripted` fill model / lab scenario |
| No orderbook messages | Lab without scenario or dummy OB | Set `SCENARIO` or `PROFILE=dev` |
| Parse errors on place | `client_order_id` as JSON array | Send string or `null` |
| Tests flaky | Parallel integration tests bind same port | `cargo test --all -- --test-threads=1` |

---

## Docker

`docker-compose.yml` sets `PAPER_EXCHANGE_DUMMY_OB=1` for dev-style feeds, copies/mounts `config/` into the image at `/app/config`, and sets `PAPER_EXCHANGE_CONFIG=/app/config/instruments.yaml`. Override the mount or env var to use a different instruments file.

---

## Tests and examples

```bash
cargo test -p paper-exchange-core
cargo test -p paper-exchange-server --test integration -- --test-threads=1
cargo test --all -- --test-threads=1
```

| Example | Command |
|---------|---------|
| Scripted scenario (in-process) | `cargo run --example scripted_scenario` |
| Procedural OB demo | `cargo run --example procedural_demo` |

---

## Architecture notes

- **Matching:** Incoming orders are checked against the **current injected orderbook** (not a full CLOB of user resting orders). User orders do not rest on the book for others to hit.
- **Account:** Single paper account; reserves quote on buy, base on sell; release on fill/cancel.
- **Streams:** Core emits market (orderbook, public trade) and private (execution, balance, position) events; server broadcasts to all WS subscribers and routes command replies to the requesting socket.
- **Profiles:** `lab` is intended for scenario-driven tests and `dev` for synthetic moving books; the checked-in `market` config can start the synthetic engine even under `lab` unless a static scenario suppresses it.

Design background: [paper-exchange-v1 proposal](../proposals/paper-exchange-v1.md).

---

## Document index

| Doc | Content |
|-----|---------|
| [DEV-GUIDE.md](DEV-GUIDE.md) | This file — run, configure, integrate |
| [CLIENT-INSTRUCTIONS.md](CLIENT-INSTRUCTIONS.md) | Endpoint map + split client layout |
| [API-EXECUTION.md](API-EXECUTION.md) | Trading WebSocket schema |
| [API-MARKET-DATA.md](API-MARKET-DATA.md) | Market WebSocket schema |
| [DESIGN.md](DESIGN.md) | Scenario format reference |
| [PROP_TRADING_INTEGRATION_GUIDE.md](PROP_TRADING_INTEGRATION_GUIDE.md) | Cross-repo Candidate #2 Paper handoff and qualification gates |
| [README.md](../README.md) | Repo overview and build |
