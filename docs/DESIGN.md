# Design log

Short design notes and TODOs. Full plan: [proposals/paper-exchange-v1.md](../proposals/paper-exchange-v1.md).

## Locked decisions

- **Time:** Use `chrono` for all timestamps. For real-time: `chrono::Utc::now()`. For deterministic scripted runs: sim time as `chrono::DateTime<Utc>` or `NaiveDateTime` advanced from event timestamps; exchange tags all events with current sim time.
- **IDs:** `OrderId` and `TradeId` are UUID v4 (exchange-generated). `ClientOrderId` is an optional string provided by the client. `InstrumentId` is a string wrapper. `AccountId` reserved for future multi-tenant.
- **Idempotency:** If the client resubmits the same `client_order_id`, the exchange **rejects** with a validation error (duplicate client_order_id). Alternative: return existing order; we chose reject for clarity.
- **Reserve:** On place order: lock (reserve) **quote** for a buy order (quantity * price) and **base** for a sell order (quantity). Release on cancel in full; on partial fill, release proportionally.
- **Wire format (Phase 3):** JSON over WebSocket for client-server messages.

## Scenario format (Phase 2)

Scripted scenarios are JSON files with an `events` array. Each event has a type `t` and a payload:

- **orderbook:** `instrument_id`, `bids` (array of `[price, qty]` strings), `asks` (same). Replaces the book for that instrument.
- **trade:** `instrument_id`, `price`, `quantity`, `side` ("Buy" or "Sell"). Injects a public trade and runs matching.
- **place_order:** `instrument_id`, `side`, `quantity`, `limit_price` (optional). Places a limit order (order_type defaults to Limit).

Events are applied in order. See `config/scenario_01.json` and `examples/scripted_scenario.rs`.

## TODO

- [ ] Refine API and data formats after first implementation pass.
