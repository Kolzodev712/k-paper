# Prop trading integration guide for K-paper

**Status:** Increment 5B is ready to begin after local 5R/5A acceptance in the primary plan. This is the existing Spot/levels connectivity preflight only; futures and deterministic fault capabilities remain future work.

**Primary plan:** [Prop Trading Productionization and Research Architecture Plan](https://github.com/Kolzodev712/trading_system/blob/master/docs/PROP_TRADING_RESEARCH_AND_PRODUCTION_PLAN.md), especially sections 6, 48–49 and Increments 5R, 5B, 6, 7 and 9A.

## Purpose and boundary

K-paper is the local exchange test venue for the real `trading_system` runner, market-data engine, risk manager, executor and reconciliation path. It answers whether the machine handles orders and failures correctly. Historical strategy economics remain in `research_engine` with an independent audit. A Paper PnL result cannot promote Candidate #2, which remains rejected as validated alpha and serves as an operational baseline.

The browser `trading-system-ui` speaks to the runner, never directly to K-paper. The runner owns readiness, risk and command results. K-paper owns exchange-side state, WS/REST behavior and fault injection.

Increment 5R in `trading_system` moves Candidate #2's concrete adapter into a thin composition app that starts the generic runner **in the same process**. The runner/`trading_engine` crates do not depend on model or research crates. The early Spot/levels smoke test below still uses the existing default runner; the later Candidate #2 futures qualification must use the new composition app and its unchanged runner-to-K-paper adapters. No separate strategy service or simulator-to-strategy API is involved.

## Read the existing contracts first

- [Developer guide](DEV-GUIDE.md), [execution API](API-EXECUTION.md), [market-data API](API-MARKET-DATA.md), [client instructions](CLIENT-INSTRUCTIONS.md).
- Core: `crates/paper-exchange-core/src/{exchange,account,order_manager,matching,types}.rs`.
- Server: `crates/paper-exchange-server/src/{runtime,transport,rest,scenario,market_engine}.rs` and `tests/integration.rs`.
- Consumer: `trading_system/src/{exchange_engine,market_data_engine,asset_engine}/src/exchanges/k_paper/`; the Paper execution client/driver are in `exchange_engine`.
- Config: `config/instruments.yaml` and `trading_system/sample_config/k-paper/bot.json`.

The producer and consumer protocol must change together. Record both repository revisions in every cross-repo run.

## Current baseline: do not overclaim it

| Available today | Important limit |
| --- | --- |
| Single in-memory account; orderbook, orders, simple crossing fills, balance reservation, execution/private events | Fill accounting is spot-style: buys consume quote and credit base; sells consume base and credit quote. Futures position accounting is not implemented by merely setting `instrument_type`. |
| `/ws/market`, `/ws/trade`, `/balances`, `/open_orders`, `/trades`, `/exchange_info` | No authoritative REST position snapshot or terminal-order lookup by client ID for ambiguous submissions. Server restart loses the account. |
| Static startup market scenarios and timed price overlays; seeded synthetic market engine | No accepted-but-lost ACK, controlled WS disconnect, duplicate/out-of-order private event, REST/WS disagreement or complete scripted partial-fill facility yet. |
| Existing Spot/levels integration in `trading_system` | The Paper client there has only one in-flight place/cancel/amend of each kind. It is a useful single-strategy constraint, not a concurrent-request simulator. |

Current `runtime.rs` defaults to port **19999**. The checked-in `config/instruments.yaml` contains a `market` section, so the market engine can start even if `PAPER_EXCHANGE_PROFILE=lab`; setting the profile alone does not guarantee deterministic, static-only data. A static scenario file without `PAPER_EXCHANGE_DUMMY_OB=1` suppresses that engine. Do not use a random dev book as evidence for the deterministic qualification gate.

## Increment 5B — connectivity preflight

The primary plan's 5R and 5A local gates are accepted for this limited preflight. The Spot/levels runner must emit authoritative Paper/local venue and execution-mode status to the browser; `UNKNOWN` defaults cannot satisfy this smoke test.

Use the existing Spot/levels sample and [trading-system local guide](https://github.com/Kolzodev712/trading_system/blob/master/docs/LOCAL_DEV_GUIDE.md). Start K-paper on a known port, then the runner and browser UI. Verify:

1. `/ws/market` supplies a valid book and the market-data adapter consumes it.
2. `/ws/trade` accepts a command and emits its reply on the same connection.
3. REST balances, open orders, trades and exchange info are mapped through the existing adapters and reconciled by the runner.
4. The UI connects to that runner and labels the environment Paper/local.

Record observed endpoints, versions and any failures. This is a connectivity smoke test; it does **not** qualify Candidate #2's BTCUSDT linear perpetual.

## Increments 6–7 — minimum futures and recovery contract

Design only the one-way USDT-linear BTCUSDT behavior required to exercise Candidate #2. Document the chosen accounting rules and exact test fixtures before implementing them:

- Authoritative signed position (flat/long, with unexpected short detectable), entry basis, collateral/margin, realized/unrealized PnL, fees and controlled funding. A sell that reduces a long must not require a spot BTC balance. Define funding, fee and margin parameters explicitly in fixtures; do not silently borrow a Bybit assumption.
- Place/fill/partial-fill/cancel/exit transitions update order, account and position consistently; private position events and an independent REST position snapshot agree. Enforce reduce-only so an exit cannot create or enlarge a short.
- Stable client order identity, duplicate-ID handling and an authoritative lookup by client ID that covers live **and terminal** orders. Absence from `/open_orders` never proves an accepted order was rejected. A lost reply must be recoverable after runner restart while K-paper remains alive.
- Adapt `trading_system` Paper REST/WS mappings and futures-specific `TradingClient` methods together. Readiness and risk still belong to the runner; do not put strategy-specific logic in this server.

Explicitly list simulator/Bybit differences in the qualification report. K-paper needs enough futures semantics to test the runner; it need not clone every Bybit feature.

## Increment 9A — deterministic fault laboratory

Provide a test-only scenario interface or harness with controlled sequencing, trigger conditions, simulated clock and stable IDs. Control randomness as well as timestamps/UUIDs. Scope and reset each fault per test session so it cannot affect later tests. Preserve normal `/ws/trade`, `/ws/market` and REST contracts for the client under test.

The required fixtures in plan section 49 cover normal ACK/fill, accepted order with delayed or missing reply, rejection, partial fills, cancel/fill race, duplicate and reordered events, private/public disconnect, stale or gapped market data, REST/WS disagreement, and runner restart with working order/position/unknown submission. For each fixture:

- drive the **actual** Candidate #2 composition app -> generic runner risk/execution path with controlled completed candles supplied by the runner's candle source and a separate Paper orderbook; a synthetic book is not a daily signal source;
- assert K-paper account/orders/positions independently from runner journal, reservations, effective exposure, ledger and gRPC status;
- prove no duplicate exposure and the intended entry block/recovery transition;
- record fixture/seed, K-paper and runner revisions, observed trace and result.

Automate the core cases without browser clicks; use the browser to observe representative cases and invoke operator controls. Restart the **runner** while K-paper retains state. An exchange-server restart needs a persistence/restore design before it can count as a continuity test.

## Verification and handoff

Run focused core/server tests and the integration tests (the current tests use fixed ports, so avoid parallel collisions). The repository documents `cargo test -p paper-exchange-core` and `cargo test --all -- --test-threads=1`. Then run the cross-repo qualification against the matching runner and UI versions. Report current versus newly implemented behavior and every unproven Bybit-specific difference.

Stop after each roadmap increment. The optional UI Simulation Lab is a later development tool: its commands must go through a runner-owned, Paper/local-only API and are not part of the first qualification gate.
