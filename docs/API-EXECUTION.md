# Execution API

WebSocket API for **orders and account state**: place, cancel, amend orders and receive executions, balance, and position updates. Use this when integrating an execution or order-management component.

**See [CLIENT-INSTRUCTIONS.md](CLIENT-INSTRUCTIONS.md) for endpoint overview. Use `/ws/trade` so execution reports and acks are always delivered on the same connection you use for orders.**

---

## Endpoint

```
ws://<host>:<port>/ws/trade
```

- **Port:** Set `PAPER_EXCHANGE_PORT` when starting the server (e.g. `19999`). Default `0` = OS-assigned.
- **Path:** Use **`/ws/trade`** for trading (orders, executions, acks, snapshot). Alternatively `/ws` provides the same behavior; `/ws/trade` makes it explicit that this connection is for trading and will receive all execution/ack messages.

---

## Wire format

- **Transport:** WebSocket, text frames.
- **Payload:** One JSON object per message. The `type` field identifies the message kind (client → server and server → client).
- **Client → Server:** Commands: `get_account_snapshot`, `place_order`, `cancel_order`, `cancel_by_client_order_id`, `amend_order`, `cancel_all_orders`.
- **Server → Client:** Events: `connected`, `execution`, `balance`, `position`, `CancelAck`, `CancelAllAck`, `AmendAck`, `error`. On `/ws/trade` (and `/ws`) the server pushes an account snapshot (balance/position) automatically when you connect; you can request it again anytime with `get_account_snapshot`. For place/cancel/amend/cancel_all, the server sends the corresponding ack (`execution`, `CancelAck`, `AmendAck`, `CancelAllAck`) **on the same WebSocket that sent the request** — so always read from the trading connection to get execution reports.

**Conventions:**

| Field / concept      | Format / notes |
|----------------------|----------------|
| `instrument_id`      | String (e.g. `"BTC-USDT"`) |
| `order_id`, `trade_id` | UUID strings |
| `client_order_id`    | Optional idempotency key. When sent, use a string (e.g. `"my-order-1"`). Use `null` to omit. |
| Quantities / prices  | Decimal strings or numbers (e.g. `"0.001"`, `"50000.01"`) |
| `timestamp`          | ISO 8601 with timezone |

---

## Client → Server: commands

### 1. Get account snapshot

**`type`: `"get_account_snapshot"`**

Request current balances and positions. The server responds with one `balance` message per asset and one `position` message per instrument (same as the snapshot sent automatically on connect). Use this if you did not receive the initial snapshot (e.g. timeout) or after reconnect.

No payload. Example:

```json
{"type": "get_account_snapshot"}
```

### 2. Place order

**`type`: `"place_order"`**

| Field             | Type   | Required | Description |
|-------------------|--------|----------|-------------|
| `request`         | object | yes      | Order parameters (see below) |

**`request` fields:**

| Field             | Type   | Required | Description |
|-------------------|--------|----------|-------------|
| `instrument_id`   | string | yes      | e.g. `"BTC-USDT"` |
| `side`            | string | yes      | `"Buy"` or `"Sell"` |
| `order_type`      | string | yes      | `"Limit"` or `"Market"` |
| `quantity`        | string/number | yes | Order size |
| `limit_price`     | string/number | for Limit | Price; omit or `null` for Market |
| `client_order_id` | string or null | no | Idempotency key: `"your-id"` or `null` |

**Example (limit buy):**

```json
{
  "type": "place_order",
  "request": {
    "instrument_id": "BTC-USDT",
    "side": "Buy",
    "order_type": "Limit",
    "quantity": "0.1",
    "limit_price": "50000",
    "client_order_id": "my-order-001"
  }
}
```

**Example (market sell):**

```json
{
  "type": "place_order",
  "request": {
    "instrument_id": "BTC-USDT",
    "side": "Sell",
    "order_type": "Market",
    "quantity": "0.05",
    "limit_price": null,
    "client_order_id": null
  }
}
```

---

### 3. Cancel by order ID

**`type`: `"cancel_order"`**

| Field      | Type   | Description |
|------------|--------|-------------|
| `order_id` | string | Exchange-assigned order ID (UUID) |

```json
{
  "type": "cancel_order",
  "order_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

---

### 4. Cancel by client order ID

**`type`: `"cancel_by_client_order_id"`**

| Field             | Type   | Description |
|-------------------|--------|-------------|
| `client_order_id` | string | Your idempotency key used on place_order |

```json
{
  "type": "cancel_by_client_order_id",
  "client_order_id": "my-order-001"
}
```

---

### 5. Amend order

**`type`: `"amend_order"`**

| Field         | Type   | Required | Description |
|---------------|--------|----------|-------------|
| `order_id`    | string | yes      | Exchange-assigned order ID (UUID) |
| `quantity`    | string | no       | New quantity; omit to leave unchanged |
| `limit_price` | string | no       | New limit price; omit to leave unchanged |

Only quantity and/or limit price can be amended.

```json
{
  "type": "amend_order",
  "order_id": "550e8400-e29b-41d4-a716-446655440000",
  "quantity": "0.2",
  "limit_price": "50100"
}
```

---

## Server → Client: events

### 1. Execution

**`type`: `"execution"`**

One fill (or partial fill) or a reject. Use `order_id` to link to your order. When `reject_reason` is non-null, the execution represents a rejection (e.g. validation or duplicate `client_order_id`).

| Field           | Type   | Description |
|-----------------|--------|-------------|
| `order_id`      | string | Order UUID |
| `trade_id`      | string | Trade UUID |
| `instrument_id` | string | Instrument |
| `side`          | string | `"Buy"` or `"Sell"` |
| `price`         | string | Fill/reject price |
| `quantity`      | string | Fill/reject quantity |
| `fee`           | string or null | Fee if applicable |
| `timestamp`     | string | ISO 8601 |
| `reject_reason` | string or null | Set when this is a reject |

**Example (fill):**

```json
{
  "type": "execution",
  "order_id": "550e8400-e29b-41d4-a716-446655440000",
  "trade_id": "660e8400-e29b-41d4-a716-446655440001",
  "instrument_id": "BTC-USDT",
  "side": "Buy",
  "price": "50000.00",
  "quantity": "0.1",
  "fee": null,
  "timestamp": "2025-03-03T12:00:00.200Z",
  "reject_reason": null
}
```

---

### 2. Balance

**`type`: `"balance"`**

One asset’s state (available, locked, total).

| Field        | Type   | Description |
|--------------|--------|-------------|
| `asset`      | string | e.g. `"USDT"`, `"BTC"` |
| `available`  | string | Available balance |
| `locked`     | string | Reserved (e.g. open orders) |
| `total`      | string | available + locked |

```json
{
  "type": "balance",
  "asset": "USDT",
  "available": "99950.00",
  "locked": "50.00",
  "total": "100000.00"
}
```

---

### 3. Position

**`type`: `"position"`**

Position in an instrument.

| Field           | Type   | Description |
|-----------------|--------|-------------|
| `instrument_id` | string | Instrument |
| `side`          | string | `"Buy"` or `"Sell"` |
| `quantity`      | string | Position size |
| `entry_price`   | string | Average entry price |
| `unrealized_pnl`| string or null | If supported |

```json
{
  "type": "position",
  "instrument_id": "BTC-USDT",
  "side": "Buy",
  "quantity": "0.5",
  "entry_price": "49800.00",
  "unrealized_pnl": null
}
```

---

### 4. Cancel all open orders

**`type`: `"cancel_all_orders"`**

No payload. Cancels every open order (status `New`/`PartiallyFilled`) in the single paper account.

```json
{ "type": "cancel_all_orders" }
```

The server responds with one `CancelAllAck` containing an `order_ids` array (possibly empty).

---

### 5. CancelAck

**`type`: `"CancelAck"`**

Sent on the same connection after a successful cancel (by order_id or client_order_id). Lets you confirm the cancel without relying on broadcast ordering.

| Field      | Type   | Description |
|------------|--------|-------------|
| `order_id` | string | Order ID (UUID) that was canceled |

```json
{ "type": "CancelAck", "order_id": "550e8400-e29b-41d4-a716-446655440000" }
```

---

### 6. AmendAck

**`type`: `"AmendAck"`**

Sent on the same connection after a successful amend. Lets you confirm the amend without relying on broadcast ordering.

| Field      | Type   | Description |
|------------|--------|-------------|
| `order_id` | string | Order ID (UUID) that was amended |

```json
{ "type": "AmendAck", "order_id": "550e8400-e29b-41d4-a716-446655440000" }
```

---

### 7. CancelAllAck

**`type`: `"CancelAllAck"`**

| Field       | Type        | Description |
|------------|-------------|-------------|
| `order_ids` | string[]    | Array of canceled order IDs (UUID strings). Can be empty. |

```json
{ "type": "CancelAllAck", "order_ids": ["550e8400-e29b-41d4-a716-446655440000"] }
```

---

### 8. Error

**`type`: `"error"`**

Server or validation error. Sent on the **same connection** that sent the command when:

- **place_order** fails (e.g. validation, duplicate `client_order_id`, insufficient balance) → one `error` message instead of an execution.
- **cancel_order** / **cancel_by_client_order_id** fails → one `error` message instead of `CancelAck`. Common reasons: `"order already filled"`, `"order already cancelled"`, `"order was rejected"`, `"order not found"`.
- **amend_order** fails → one `error` message instead of `AmendAck` (e.g. `"order not amendable (order already filled)"`).

| Field     | Type   | Description |
|-----------|--------|-------------|
| `message` | string | Error description |

```json
{
  "type": "error",
  "message": "order already filled"
}
```

Clients should handle `error` on the trading connection for every command that expects an ack or execution; the server guarantees one response (either the success message or `error`) per command.

---

## Integration checklist

1. Open WebSocket to `ws://<host>:<port>/ws/trade` (or `/ws`) for the connection that will send orders.
2. **Sending orders:** Serialize commands as JSON and send as WebSocket text frames on **that same connection**. Use `place_order`, `cancel_order`, `cancel_by_client_order_id`, `amend_order`, `cancel_all_orders` as above.
3. **Receiving:** On the **same** connection, parse each text message as JSON; dispatch by `type`: `execution`, `balance`, `position`, `CancelAck`, `AmendAck`, `CancelAllAck`, `error`. Update your order and account state accordingly. Do not rely on another connection (e.g. a market-only feed) for execution reports.
4. **Idempotency:** Send `client_order_id` as a string (e.g. `"your-id"`) on place_order. The server rejects duplicate `client_order_id`; safe to retry after reconnect.
5. **Order identity:** You learn `order_id` from execution messages. Use it for cancel and amend. Map `client_order_id` → `order_id` locally if you need to cancel by client id.
6. **Lifecycle:** There is no separate “order accepted” message. Acceptance is implied by execution(s) or balance/position updates; rejection by no execution and/or `error`.
7. **Reserve:** On place, quote (buy) or base (sell) is reserved; on fill or cancel it is released. Balance and position events reflect this.
