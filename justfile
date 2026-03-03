# Justfile for k-paper (paper exchange)
# Run `just` to see all available commands

# Default recipe: show all available commands
default:
    @just --list

# Code formatting
fmt:
    # Format all Rust code
    cargo fmt --all

fmt-check:
    # Check if code is formatted (for CI)
    cargo fmt --all -- --check

# Linting
clippy:
    # Run clippy with all targets and features
    cargo clippy --all-targets --all-features

clippy-fix:
    # Run clippy and automatically apply fixes
    cargo clippy --fix --allow-dirty --all-targets --all-features

clippy-strict:
    # Run clippy with warnings as errors (for CI)
    cargo clippy --all-targets --all-features -- -D warnings

# Security audit
audit:
    # Run cargo audit to check for security vulnerabilities
    cargo audit

# Testing
test:
    # Run all unit tests
    cargo test --lib --all-features

test-integration:
    # Run integration tests (tests/*.rs)
    cargo test --tests --all-features -- --test-threads=1

test-all:
    # Run all tests (unit + integration)
    cargo test --all-features -- --test-threads=1

test-bin:
    # Run main system binary (k-paper)
    cargo run --bin k-paper

# Compilation checks
check:
    # Check that all packages compile without building
    cargo check --all-targets --all-features

build:
    # Build all packages in debug mode
    cargo build --all-targets --all-features

build-release:
    # Build all packages in release mode
    cargo build --release --all-targets --all-features

build-server:
    # Build server package in release mode
    cargo build --release --package paper-exchange-server

build-bin:
    # Build main binary (k-paper) in release mode
    cargo build --release --package k-paper --bin k-paper

# Full CI check (what CI runs)
ci:
    # Run all CI checks locally
    just fmt-check
    just clippy-strict
    just check
    just test
    just test-integration

# Quick check (format, clippy, test)
quick:
    # Quick local checks before committing
    just fmt-check
    just clippy
    just test

# Clean build artifacts
clean:
    # Remove all build artifacts
    cargo clean

clean-all:
    # Remove all build artifacts and target directories
    cargo clean
    @find . -name "target" -type d -exec rm -rf {} + 2>/dev/null || true
