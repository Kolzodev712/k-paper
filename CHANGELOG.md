# Changelog

## [Unreleased]

## [0.1.1] — 2026-11-27

### Changed

- Standalone Compose publishes `127.0.0.1:19999` by default.
- Public README repositioned for independent use; fault docs point at DEV-GUIDE.

### Added

- `cargo run --example external_http_client` (REST + trade WebSocket; no websocat).

### Fixed

- Procedural/scripted examples surface failed `place_order` instead of ignoring errors.
