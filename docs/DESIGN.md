# Design log

Short design notes and TODOs. Full plan: [proposals/paper-exchange-v1.md](../proposals/paper-exchange-v1.md).

## TODO

- [ ] Lock sim-time type: `chrono::NaiveDateTime` vs `DateTime<Utc>`; document here.
- [ ] Decide: UUID vs monotonic for order_id / trade_id.
- [ ] Decide: idempotency for `client_order_id` (reject duplicate vs return existing).
- [ ] Wire format for server (Phase 2): JSON vs binary; document in proposal or here.
- [ ] Refine API and data formats after first implementation pass.
