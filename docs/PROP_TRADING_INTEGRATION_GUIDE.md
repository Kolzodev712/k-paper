# Prop trading integration guide for K-paper

**Status:** Increment 6 Paper recovery hooks are implemented locally at k-paper `ed9568b`, matching `trading_system` `c16a7be`. UI proto was unchanged (`028ea16`). Client-order-ID lookup covers working and terminal orders; `POST /test/suppress_place_ack` drops the next place ACK while keeping the order. Isolated `paper-exchange-core` tests passed (6/6), including `client_order_id_lookup_includes_canceled` and `suppress_place_ack_keeps_working_order`. Stop for review before Increment 7. Futures and the full deterministic fault matrix remain future work.

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
| `/ws/market`, `/ws/trade`, `/balances`, `/open_orders`, `/trades`, `/exchange_info`, `GET /orders/by_client_id/:id` (working and terminal) | No authoritative REST position snapshot. Server restart still loses the account. `404` on client-id lookup is inconclusive, not a rejection. |
| Static startup market scenarios and timed price overlays; seeded synthetic market engine | `POST /test/suppress_place_ack` is a one-shot lab fault for a lost place ACK. Other faults (WS disconnect, duplicate/out-of-order private events, REST/WS disagreement, complete scripted partial-fill) remain future work. |
| Existing Spot/levels integration in `trading_system` | The Paper client there has only one in-flight place/cancel/amend of each kind. It is a useful single-strategy constraint, not a concurrent-request simulator. |

Current `runtime.rs` defaults to port **19999**. The checked-in `config/instruments.yaml` contains a `market` section, so the market engine can start even if `PAPER_EXCHANGE_PROFILE=lab`; setting the profile alone does not guarantee deterministic, static-only data. A static scenario file without `PAPER_EXCHANGE_DUMMY_OB=1` suppresses that engine. Do not use a random dev book as evidence for the deterministic qualification gate.

## Increment 5B — connectivity preflight

The primary plan's 5R and 5A local gates are accepted for this limited preflight. The Spot/levels runner must emit authoritative Paper/local venue and execution-mode status to the browser; `UNKNOWN` defaults cannot satisfy this smoke test.

Use the existing Spot/levels sample and [trading-system local guide](https://github.com/Kolzodev712/trading_system/blob/master/docs/LOCAL_DEV_GUIDE.md). Start K-paper on a known port, then the runner and browser UI. Verify:

1. `/ws/market` supplies a valid book and the market-data adapter consumes it.
2. `/ws/trade` accepts a command and emits its reply on the same connection.
3. REST balances, open orders, trades and exchange info are mapped through the existing adapters and reconciled by the runner.
4. The UI connects to that runner and labels the environment Paper/local.

**5B result (2026-09-28):** K-paper Dev on `:19999` exposed five symbols and six balances; `/open_orders` and `/trades` were empty. `/ws/market` delivered a BTC-USDT book, and `/ws/trade` returned `get_account_snapshot` on the same socket. The default levels runner connected both streams and bootstrap-reconciled six balances, zero orders and zero trades without discrepancy; the browser showed live **PAPER LOCAL + EXCHANGE** from runner status. Trading was off, so no order was posted. Only bootstrap reconciliation was observed. This closes connectivity only; populated order/trade mapping and ambiguous-submission behavior belong to the next gates.

## Increment 6 — recover ambiguous submissions

Add the narrow Paper-side query and fault hooks needed by the real runner to test an accepted order whose ACK is lost. Assign a stable client order ID; lookup by that ID must cover working and terminal orders, independent of `/open_orders`. Keep the Paper process alive while restarting the runner and verify the same exchange order is discovered, imported and reconciled once. A missing open order or temporarily inconclusive lookup must not be treated as rejection or permit replacement exposure. Make the fault deterministic and scoped to the test; preserve ordinary `/ws/trade` and REST behavior. Pin matching producer/consumer revisions and report actual order, reservation and recovery state. Bybit pagination is implemented and tested in `trading_system`, not K-paper. Stop after the focused recovery gate.

**Increment 6 result:** `GET /orders/by_client_id/:client_order_id` returns working or terminal orders (canceled rows stay in the client-id index). `POST /test/suppress_place_ack` with `{ "clientOrderId": "..." }` accepts the next matching place without emitting the quantity-0 ACK. `cargo test -p paper-exchange-core`: 6 passed, 0 failed, including lookup-after-cancel and suppressed-ACK retention. Full runner-restart-against-live-Paper is still an Increment 9A integration case; isolated recovery tests live in `trading_system`.

## Increment 7 — minimum linear futures contract

Design only the one-way USDT-linear BTCUSDT behavior required to exercise Candidate #2. Document the chosen accounting rules and exact test fixtures before implementing them:

- Authoritative signed position (flat/long, with unexpected short detectable), entry basis, collateral/margin, realized/unrealized PnL, fees and controlled funding. A sell that reduces a long must not require a spot BTC balance. Define funding, fee and margin parameters explicitly in fixtures; do not silently borrow a Bybit assumption.
- Place/fill/partial-fill/cancel/exit transitions update order, account and position consistently; private position events and an independent REST position snapshot agree. Enforce reduce-only so an exit cannot create or enlarge a short.
- Preserve the Increment 6 client-order-ID lookup and duplicate-ID behavior across the new futures order lifecycle. Absence from `/open_orders` never proves an accepted order was rejected; recovery must work after runner restart while K-paper remains alive.
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
