# Market Data API

WebSocket API for **orderbooks** and **public trades**. Use this when integrating a market data engine (e.g. book builder, trade tape, analytics). Read-only from the client: you connect and receive streamed data; no commands are sent for market data.

**See [CLIENT-INSTRUCTIONS.md](CLIENT-INSTRUCTIONS.md) for endpoint overview and when to use `/ws` vs `/ws/market` vs `/ws/trade`.**

---

## Endpoint

```
ws://<host>:<port>/ws/market
```

- **Port:** Set `PAPER_EXCHANGE_PORT` when starting the server (e.g. `19999`). Default `0` = OS-assigned.
- **Path:** Use **`/ws/market`** for market data only (orderbook + public_trade; no snapshot, no commands). Alternatively `/ws` also delivers market data but includes trading messages; prefer `/ws/market` for read-only feeds.

To get a continuous stream of synthetic data, start the server with:

```bash
export PAPER_EXCHANGE_DUMMY_OB=1
export PAPER_EXCHANGE_PORT=19999   # optional
cargo run
```

Without `PAPER_EXCHANGE_DUMMY_OB=1`, orderbook and public trade messages are only sent when something else injects them (e.g. a scripted scenario).

---

## Wire format

- **Transport:** WebSocket, text frames.
- **Payload:** One JSON object per message. All server → client messages include a `type` field.
- **Subscription:** None. Connect and you receive every orderbook and public trade the server produces. No subscribe/unsubscribe messages.

**Conventions (market data only):**

| Field            | Format / notes                                      |
|------------------|-----------------------------------------------------|
| `instrument_id`  | String (e.g. `"BTC-USDT"`)                          |
| `trade_id`       | UUID string                                         |
| Prices/quantities| Decimal strings or numbers (e.g. `"50000.01"`)      |
| `timestamp`      | ISO 8601 with timezone (e.g. `"2025-03-03T12:00:00Z"`) |

---

## Server → Client: message types

On **`/ws/market`** your client will only see these `type` values (no execution, balance, position, or trading acks).

### 1. Orderbook snapshot

**`type`: `"orderbook"`**

Full snapshot for one instrument. Replace your local book for that instrument with each message (snapshots are full replace, not deltas).

| Field           | Type   | Description |
|-----------------|--------|-------------|
| `instrument_id` | string | Instrument (e.g. `"BTC-USDT"`) |
| `bids`          | array  | Levels `{ "price": string, "quantity": string }`, descending by price |
| `asks`          | array  | Levels `{ "price": string, "quantity": string }`, ascending by price |
| `sequence`      | number | Monotonic per instrument; use for ordering / duplicate detection |
| `timestamp`     | string | Server time of the snapshot (ISO 8601) |

**Example:**

```json
{
  "type": "orderbook",
  "instrument_id": "BTC-USDT",
  "bids": [
    { "price": "49950.00", "quantity": "0.52" },
    { "price": "49900.00", "quantity": "0.48" }
  ],
  "asks": [
    { "price": "50050.00", "quantity": "0.51" },
    { "price": "50100.00", "quantity": "0.49" }
  ],
  "sequence": 42,
  "timestamp": "2025-03-03T12:00:00.000Z"
}
```

---

### 2. Public trade

**`type`: `"public_trade"`**

One trade that occurred (synthetic or from matching).

| Field           | Type   | Description |
|-----------------|--------|-------------|
| `instrument_id` | string | Instrument |
| `price`         | string | Trade price |
| `quantity`      | string | Trade size |
| `side`          | string | `"Buy"` or `"Sell"` |
| `timestamp`     | string | Trade time (ISO 8601) |
| `trade_id`      | string | Unique trade ID (UUID) |

**Example:**

```json
{
  "type": "public_trade",
  "instrument_id": "BTC-USDT",
  "price": "50000.00",
  "quantity": "0.05",
  "side": "Buy",
  "timestamp": "2025-03-03T12:00:00.100Z",
  "trade_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

---

## Integration checklist

1. Open WebSocket to `ws://<host>:<port>/ws/market`.
2. For each received text message, parse JSON and read `type`.
3. If `type === "orderbook"`: update your orderbook state for `instrument_id` (full replace). Optionally use `sequence` to ignore out-of-order or duplicate snapshots.
4. If `type === "public_trade"`: append to your trade tape or feed for that instrument.
5. Ignore or forward any other `type` (e.g. `execution`, `balance`, `position`, `error`) if you are only integrating market data.
6. On reconnect, treat the next orderbook snapshot as the new state; there is no resume-by-sequence.

Default dummy data is for one instrument (e.g. BTC-USDT). If more instruments are added on the server, handle any `instrument_id` you care about in the same way.
