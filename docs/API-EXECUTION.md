# Execution API

WebSocket API for **orders and account state**: place, cancel, amend orders and receive executions, balance, and position updates. Use this when integrating an execution or order-management component.

---

## Endpoint

```
ws://<host>:<port>/ws
```

- **Port:** Set `PAPER_EXCHANGE_PORT` when starting the server (e.g. `19999`). Default `0` = OS-assigned.
- **Path:** `/ws` only. No query parameters or auth. Single paper account; no login.

---

## Wire format

- **Transport:** WebSocket, text frames.
- **Payload:** One JSON object per message. The `type` field identifies the message kind (client → server and server → client).
- **Client → Server:** Commands: `place_order`, `cancel_order`, `cancel_by_client_order_id`, `amend_order`.
- **Server → Client:** Events: `execution`, `balance`, `position`, `error`.

**Conventions:**

| Field / concept      | Format / notes |
|----------------------|----------------|
| `instrument_id`      | String (e.g. `"BTC-USDT"`) |
| `order_id`, `trade_id` | UUID strings |
| `client_order_id`    | Optional; when sent, use a **one-element array** with your string, e.g. `["my-order-1"]`, for idempotency. Use `null` to let the server assign. |
| Quantities / prices  | Decimal strings or numbers (e.g. `"0.001"`, `"50000.01"`) |
| `timestamp`          | ISO 8601 with timezone |

---

## Client → Server: commands

### 1. Place order

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
| `client_order_id` | array or null | no | Idempotency key: `["your-id"]` or `null` |

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
    "client_order_id": ["my-order-001"]
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

### 2. Cancel by order ID

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

### 3. Cancel by client order ID

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

### 4. Amend order

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

### 4. Error

**`type`: `"error"`**

Server or validation error (e.g. invalid symbol, balance, duplicate `client_order_id`).

| Field     | Type   | Description |
|-----------|--------|-------------|
| `message` | string | Error description |

```json
{
  "type": "error",
  "message": "Duplicate client_order_id"
}
```

Note: Not every validation failure may be sent as an `error` message; some are only logged. Absence of an execution (and optional `error`) can indicate rejection.

---

## Integration checklist

1. Open WebSocket to `ws://<host>:<port>/ws`.
2. **Sending orders:** Serialize commands as JSON and send as WebSocket text frames. Use `place_order`, `cancel_order`, `cancel_by_client_order_id`, `amend_order` as above.
3. **Receiving:** Parse each text message as JSON; dispatch by `type`: `execution`, `balance`, `position`, `error`. Update your order and account state accordingly.
4. **Idempotency:** Send `client_order_id` as `["your-id"]` on place_order. The server rejects duplicate `client_order_id`; safe to retry after reconnect.
5. **Order identity:** You learn `order_id` from execution messages. Use it for cancel and amend. Map `client_order_id` → `order_id` locally if you need to cancel by client id.
6. **Lifecycle:** There is no separate “order accepted” message. Acceptance is implied by execution(s) or balance/position updates; rejection by no execution and/or `error`.
7. **Reserve:** On place, quote (buy) or base (sell) is reserved; on fill or cancel it is released. Balance and position events reflect this.
