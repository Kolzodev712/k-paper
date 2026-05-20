# Client integration overview

How to connect trading and market-data components to k-paper. For setup, profiles, and YAML, see **[DEV-GUIDE.md](DEV-GUIDE.md)**. For message schemas, see the API docs below.

## Endpoints

| Path | Use for |
|------|---------|
| `ws://<host>:<port>/ws/trade` | **Trading** — place/cancel/amend, executions, balances, positions, acks, errors. Prefer this over `/ws`. |
| `ws://<host>:<port>/ws/market` | **Market data only** — orderbook snapshots and public trades. Commands are ignored. |
| `ws://<host>:<port>/ws` | All-in-one (market + trading). Same as `/ws/trade` for order flow; also receives orderbook/trades. |
| `http://<host>:<port>/balances` | REST bootstrap: balances |
| `http://<host>:<port>/open_orders` | REST: open orders (`?instrument_id=` optional) |
| `http://<host>:<port>/trades` | REST: fill history (`?instrument_id=`, `?limit=` optional) |
| `http://<host>:<port>/exchange_info` | REST: symbols, tick/lot/min notional |

Replace `<host>:<port>` with the bind address printed at startup (e.g. `127.0.0.1:19999` when `PAPER_EXCHANGE_PORT=19999`).

## Recommended split (production-style bots)

1. **Market data engine** → connect to `/ws/market`, maintain orderbook + trade tape.
2. **Execution / OMS** → connect to `/ws/trade`, send orders, read `execution` / acks / `error` on the **same** socket.
3. **Reconciliation** → on startup and periodically: `GET /balances`, `GET /open_orders`, `GET /trades`; treat REST as source of truth if WS missed messages.

Do not wait for executions on `/ws/market` — they are only on `/ws/trade` (or `/ws`).

## API reference

- [API-EXECUTION.md](API-EXECUTION.md) — WebSocket trading commands and server messages
- [API-MARKET-DATA.md](API-MARKET-DATA.md) — orderbook and public trade messages
- [DEV-GUIDE.md](DEV-GUIDE.md) — how to run the server (lab vs dev, scenarios, config)
