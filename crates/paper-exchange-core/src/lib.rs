//! # paper-exchange-core
//!
//! Core library for the standalone paper exchange: orderbook state, order lifecycle,
//! fill model, account state, and output streams. No I/O beyond channels/callbacks.
//!
//! See [paper-exchange-v1](../../../proposals/paper-exchange-v1.md) and layout plan for full design.

pub mod account;
pub mod config;
pub mod error;
pub mod exchange;
pub mod matching;
pub mod order_manager;
pub mod orderbook;
pub mod types;
pub mod validation;

#[cfg(test)]
mod tests;
