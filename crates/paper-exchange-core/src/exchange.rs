//! Main facade: event loop/tick, inject events, place/cancel/amend, output streams.
//!
//! TODO: PaperExchange (or Exchange) handle; inject_orderbook, inject_trade.
//! TODO: place_order, cancel_order, amend_order; subscribe to market-data and private streams.
//! TODO: Event ordering / tick model for determinism (single queue or timestamp-driven).
