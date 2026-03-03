//! # paper-exchange-server
//!
//! Server that wraps paper-exchange-core: accepts client connections (WebSocket or gRPC),
//! session handling, serialization of exchange-native types. Phase 2.
//!
//! TODO: Re-export or use core types on the wire.
//! TODO: Session/auth (e.g. API key); multiplex market-data and private streams.

pub mod session;
pub mod transport;
pub mod wire;

/// Main entry point for the paper exchange system (called by root binary or server binary).
pub fn run() {
    // TODO: load config, create Exchange, start WebSocket listener
    eprintln!("k-paper: paper exchange server not yet implemented (Phase 2)");
}
