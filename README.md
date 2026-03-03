# k-paper

Standalone paper exchange: orderbook, order lifecycle, matching, account state, and streams. Usable as an in-process library or (Phase 2) as a server.

- **License:** Proprietary. See [LICENSE](LICENSE).
- **Design:** [proposals/paper-exchange-v1.md](proposals/paper-exchange-v1.md) — preliminary design plan (v1); implement from there.
- **Layout:** Workspace with `paper-exchange-core` (library) and `paper-exchange-server` (Phase 2).

## TODO

- [ ] Implement core: types, orderbook, order_manager, validation, matching, account, exchange (see proposal phases).
- [ ] Add scripted scenario runner and deterministic tests.
- [ ] Phase 2: server (WebSocket/gRPC), session, wire format.
- [ ] Define repo rules (e.g. `.cursor/rules` or CONTRIBUTING) for contributors and AI.

## Build and run

```bash
cargo build
cargo run              # run main system binary (k-paper)
cargo run -p paper-exchange-server   # same, via server crate
cargo test -p paper-exchange-core
```

The main executable is **k-paper** at the repo root (`src/main.rs`). It delegates to the server; once Phase 2 is implemented, `cargo run` starts the paper exchange system.

## Crates

| Crate | Role |
|-------|------|
| **k-paper** (root) | Main system binary; runs the server. |
| `paper-exchange-core` | Orderbook, orders, fill model, account, streams; no I/O beyond channels. |
| `paper-exchange-server` | Wraps core; WebSocket server, sessions (Phase 2). |
