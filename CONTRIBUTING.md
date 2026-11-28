# Contributing

k-paper is a **standalone** exchange simulator. You can contribute useful improvements without adopting `trading_system`.

## Ground rules

- Prefer small, focused pull requests with tests.
- Do not commit secrets or expose lab fault routes in shared deployments by default.
- AI-assisted patches are welcome only if you reviewed and can explain the change.
- Matching, account-state, and fault-injection changes need clear tests.

## Getting started

1. Toolchain: `rust-toolchain.toml` (`1.98.1`).
2. Fork and branch from `master`.
3. Pick a `good first issue` / `help wanted` issue, or open a proposal issue first.

```bash
cargo build
cargo test -p paper-exchange-core
cargo test --all -- --test-threads=1
just fmt && just clippy-strict   # if just is installed
cargo run --example external_http_client   # needs a running server
```

Docker: `docker compose up --build` (publishes `127.0.0.1:19999` by default).

## Pull requests

Describe the problem, approach, tests run, and any API/behavior changes.

## Related projects

- https://github.com/Kolzodev712/trading_system
- https://github.com/Kolzodev712/trading-system-showcase

## Reporting problems

- Issues: GitHub Issues (templates when available). Open contributions: https://github.com/Kolzodev712/k-paper/labels/good%20first%20issue
- Security: [SECURITY.md](SECURITY.md) — GitHub private vulnerability reporting.
- Conduct: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

Apache License 2.0 — see `LICENSE` and `NOTICE`.
