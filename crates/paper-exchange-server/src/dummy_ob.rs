//! Backward-compatible re-exports; use [`crate::market_engine`] for new code.

pub use crate::market_engine::{
    default_initial_mid, spawn_market_engine as spawn_dummy_ob, InstrumentMarketSpec,
    MarketEngineConfig as DummyObConfig, MarketEngineConfig,
};
