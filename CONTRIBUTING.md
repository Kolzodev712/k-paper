# Contributing

## Development

- Toolchain: `rust-toolchain.toml` (`1.98.1`)
- `just fmt` / `just clippy-strict` / `just test`
- Docker: `docker compose up --build` (ships `config/` into the image)

Do not commit secrets. Lab fault routes (`PAPER_LAB_FAULTS=1`) are for local isolation only.

## License

Apache License 2.0 — see `LICENSE` and `NOTICE`.
