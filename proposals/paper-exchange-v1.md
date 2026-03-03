# Standalone paper exchange — preliminary design plan (v1)

## 1. Purpose and scope

**What it is:** A standalone service or library that behaves like an exchange: it has an orderbook (or generates one), accepts order and cancel requests, maintains account state (balances, positions, open orders), applies a fill/matching model, and emits (1) market data (orderbook snapshots, public trades) and (2) private data (execution reports, balances, positions) to connected clients.

**What it is not:** It does not depend on any specific trading system's types (e.g. no TradingClient, no ExchangeUpdate). Its API is "exchange-like" and generic so that any client (this trading system, another system, or tests) can integrate via an adapter.

**Goals:**

- **Reusable:** usable from multiple consumers (this repo's adapter, other projects, future products).
- **Deterministic** when driven by scripted or seeded data (for tests and demos).
- **Configurable:** fill model, fees, slippage, optional fault injection.
- **Clear contract:** well-defined inputs (orders, cancels) and outputs (market data stream, execution stream).

---

## 2. Core responsibilities (in scope)

| Area | Responsibility |
|------|----------------|
| **Orderbook** | Maintain per-instrument orderbook state. Support (a) scripted updates (inject OB snapshots), (b) procedural generation (e.g. random walk), or (c) "synthetic" book derived from public trade stream. |
| **Public trades** | Emit a stream of public trades (price, qty, time, side). Can be scripted, generated from OB crossing, or replayed. |
| **Order lifecycle** | Accept place order (limit, market, optionally other types), cancel (by id), amend (cancel-replace or in-place if desired). Validate (symbol exists, size/price rules, optional risk checks). |
| **Matching / fill model** | Decide when orders fill: e.g. price-cross vs current book, or match against incoming "synthetic" flow, or scripted fill events. Emit execution reports (New, PartiallyFilled, Filled, Canceled, Rejected). |
| **Account state** | Per "account" or single account: balances (per asset), open orders, positions (if futures/CFD), reserved margin. Updates emitted on fill/cancel. |
| **Sessions / connectivity** | If server: accept client connections (e.g. WebSocket or gRPC), authenticate (e.g. API key or token), multiplex market data and private streams. If in-process: expose channels or callbacks. |

---

## 3. Out of scope (preliminary)

- Real asset custody, settlement, or regulatory reporting.
- Full CLOB matching engine with queue priority and complex order types (iceberg, TWAP, etc.) — start with simple limit/market and optional partial fills.
- Multi-venue routing or smart order routing (this is a single venue).
- Production-grade persistence (optional: simple snapshot/restore for replay or checkpointing).

---

## 4. Data model (exchange-native)

Define types that the paper exchange uses internally and exposes. Adapters in consuming systems (e.g. trading_system) map these to their own types.

**Instruments / symbols**

- **Instrument:** id, symbol (e.g. "BTC-USDT"), base asset, quote asset, tick size, lot size, min notional. Optional: instrument type (spot vs futures).

**Orderbook**

- **Book level:** price, quantity (and optionally order count for display).
- **Orderbook snapshot:** instrument_id, bids (sorted best first), asks, sequence/timestamp.
- **Incremental updates (optional for v1):** delta updates to reduce bandwidth.

**Orders (exchange side)**

- **Order request:** instrument_id, side (buy/sell), order_type (limit, market), quantity, limit_price (if limit), client_order_id (optional).
- **Order state:** order_id, client_order_id, instrument_id, side, type, quantity, price, filled_qty, status (New, PartiallyFilled, Filled, Canceled, Rejected), created_at, updated_at. Optional: account_id.

**Trades**

- **Public trade:** instrument_id, price, quantity, side, timestamp, trade_id. (What the "market" sees.)
- **Execution (private):** order_id, trade_id, instrument_id, side, price, quantity, fee (optional), timestamp. Tied to an account.

**Account**

- **Balance:** asset, available, locked (in orders), total. Optional: account_id.
- **Position (if futures):** instrument_id, side, quantity, entry_price, unrealized_pnl (optional). Optional: account_id.

**Streams (what the exchange emits)**

- **Market data stream:** orderbook snapshots (and/or incremental), public trades.
- **Private stream:** execution reports (order lifecycle + fills), balance updates, position updates. Scoped by account/session.

---

## 5. API surface: how clients interact

**Option A — In-process (library)**

- Exchange is a Rust crate (or similar). Clients obtain a handle (e.g. PaperExchange).
- **Input:** Method calls or channels: place_order(req), cancel_order(id), amend_order(id, req). Optionally: inject_orderbook(instrument_id, snapshot) or inject_trade(...) for scripted flow.
- **Output:** Callbacks or channels: subscribe to "market data" and "execution" streams. Events are exchange-native (orderbook, trade, execution, balance, position).
- **Use case:** Same process (e.g. integration tests), or same machine with minimal serialization.

**Option B — Server (network)**

- Exchange runs as a process. Clients connect over WebSocket(s) and/or gRPC.
- **Input:** Authenticated session; send order/cancel/amend as messages. Optionally admin or "scenario" endpoint to inject scripted data.
- **Output:** Two logical streams (can be one or two physical connections): (1) market data — orderbook + public trades; (2) private — execution, balance, position.
- **Use case:** Demos, multi-process tests, or future "paper trading as a service."

**Recommendation (preliminary):** Design the core as a library (in-process API). Add a thin server layer that wraps the core and exposes it over WebSocket (or gRPC) so the same code serves both in-process and remote use. First phase can be in-process only; server in phase 2.

---

## 6. Fill model (design choices)

**Simple (v1):**

- **Limit orders:** Fill when "market" crosses the limit: e.g. for a limit buy, fill when best_ask <= limit_price (using current orderbook or next injected trade). Optionally fill at mid or with a configurable spread.
- **Market orders:** Fill immediately at best bid/ask (or next available price) with optional slippage.
- **Partial fills (optional):** Configurable: e.g. fill in one shot or in chunks (e.g. 30% / 70%) on successive ticks.

**Advanced (later):**

- **Trade-through:** When "public trades" are replayed or injected, fill if a trade price crosses the order's limit (good for replay-based backtesting).
- **Queue / pro-rata:** Simulate queue position; partial fill by available size. More realistic but more complex.

**Configuration (preliminary):**

- fill_model: immediate_cross | trade_through | scripted.
- slippage_bps, fee_bps (optional).
- partial_fills: off | on (with optional chunk policy).

---

## 7. Market data generation

How does the orderbook (and public trade stream) get populated?

**Scripted (deterministic):**

- Tests or demos supply a sequence of (timestamp, orderbook_snapshot) and/or (timestamp, public_trade). Exchange replays them in order. Fully deterministic.

**Procedural:**

- Internal generator (e.g. random walk, sine wave, or configurable process) produces prices and optionally sizes. Good for demos and stress tests when you don't have a script.

**Replay (future):**

- Load recorded market data from file (or stream) and replay into the exchange. Same as scripted but from external data.

**v1 suggestion:** Implement scripted first (list of snapshots + trades with timestamps). Optionally add a simple procedural generator (e.g. random walk with configurable step). Replay from file can be a later phase.

---

## 8. Account and session model

**Single-tenant (v1):**

- One "account" per exchange instance: one set of balances, positions, open orders. Optional: configurable account_id for future multi-tenant.

**Multi-tenant (later):**

- Multiple accounts; each order and execution is tagged with account_id. Private stream filtered by account. Not required for first version.

**Sessions (if server):**

- Client connects with API key (or token). Exchange associates session with an account. All orders from that session apply to that account; private stream is for that account only.

---

## 9. Configuration (preliminary)

- **Instruments:** List of instruments (symbol, base, quote, tick size, lot size, min notional). Optional: instrument type (spot/futures).
- **Initial state:** initial_balances: list of (asset, amount) or (asset, available, locked). initial_positions (if futures): optional list of (instrument_id, side, quantity, entry_price).
- **Fill model:** fill_model, slippage_bps, fee_bps, partial_fills.
- **Optional (later):** inject_faults: reject rate, duplicate execution, out-of-order, disconnect simulation. ack_delay_ms, fill_delay_ms for latency simulation.

---

## 10. Repo and crate structure (preliminary)

**Repository:** e.g. paper-exchange or trading-sim-exchange (separate from trading_system).

**Suggested layout (Rust):**

- **crate exchange_core (or paper_exchange_core):** pure logic — orderbook state, order state, fill model, account state. No I/O beyond channels/callbacks. Testable in isolation.
- **crate exchange_server (optional, phase 2):** server that wraps core; WebSocket (or gRPC) transport, session handling, serialization of exchange-native types.
- **examples/ or bin/:** minimal runner that loads config, creates exchange, runs scripted scenario or procedural generator.

**Dependencies:** Keep minimal. No dependency on trading_system or trading_core. Use standard types (Decimal for prices/sizes, chrono or std::time for timestamps) or define small local types.

---

## 11. Implementation phases (preliminary)

**Phase 1 — Core (in-process)**

- Data types: Instrument, Orderbook, Order, Trade, Execution, Balance, Position.
- Orderbook state (single instrument first; then multi).
- Order lifecycle: place, cancel, amend; validation; store open orders.
- Fill model: immediate price-cross (limit + market).
- Account state: balances and optional positions; update on fill/cancel.
- Scripted input: inject orderbook snapshots and/or public trades (with timestamp or sequence).
- Output: channels (or callbacks) for market data stream and execution/balance/position stream.
- Config: instruments, initial balances, fill model params.
- Unit tests: fill logic, order state transitions, balance updates.

**Phase 2 — Deterministic runner and scripting**

- Runner: load scenario from config or file (e.g. JSON/YAML list of events).
- Drive exchange with scripted events; optionally assert on expected executions.
- Simple procedural generator (e.g. random walk OB) for demos.

**Phase 3 — Server (optional)**

- Wrap core in server process.
- WebSocket (or gRPC): subscribe to market data and private stream; send order/cancel/amend.
- Session/auth (e.g. API key).
- Integration test: client connects, places order, receives execution.

**Phase 4 — Adapter in trading_system**

- In trading_system repo: crate or module that connects to paper exchange (in-process or client to server).
- Implements TradingClient, maps exchange-native execution/balance/position to ExchangeUpdate.
- Market data source that subscribes to paper exchange market stream and pushes to MarketDataUpdate.
- Runner config: select "paper exchange" and wire adapter.

---

## 12. Verification and success criteria

- **Determinism:** Same scenario (scripted events + same orders) produces same execution sequence.
- **Contract:** Documented input/output types and semantics; adapter in trading_system can integrate without changing paper exchange for system-specific types.
- **Reusability:** Exchange runs and is testable without the trading_system repo.
- **Demo:** Run exchange with scripted or procedural data; connect trading_system adapter; place orders and see ack → fill in TUI.

---

## 13. Open questions (for design log or later)

- **Wire format:** If server, JSON vs binary (e.g. protobuf) for market data and execution.
- **Order ID and trade ID:** Exchange-generated IDs; format and uniqueness guarantees.
- **Time model:** Real-time vs simulated time (e.g. scripted events with timestamps; exchange advances "sim time" from events).
- **Persistence:** Snapshot/restore for replay or recovery; optional and likely post-v1.

This plan is preliminary; a design log in the paper-exchange repo (or in trading_system for the adapter phase) can refine each section and lock API and data formats before implementation.
