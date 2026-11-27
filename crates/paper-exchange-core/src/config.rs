//! Configuration: instruments, initial balances, fill model params.
//! No file I/O in core; callers pass parsed Config.

use crate::linear::LinearContract;
use crate::types::{Asset, Instrument};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillModelKind {
    ImmediateCross,
    TradeThrough,
    Scripted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FillModelConfig {
    pub kind: FillModelKind,
    pub slippage_bps: u32,
    pub fee_bps: u32,
    pub partial_fills: bool,
}

impl Default for FillModelConfig {
    fn default() -> Self {
        Self {
            kind: FillModelKind::ImmediateCross,
            slippage_bps: 0,
            fee_bps: 0,
            partial_fills: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitialBalance {
    pub asset: Asset,
    pub amount: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub instruments: Vec<Instrument>,
    pub initial_balances: Vec<InitialBalance>,
    pub fill_model: FillModelConfig,
    /// Max fills kept for REST `/trades` (0 = unlimited). Defaults to 10 000 when absent from JSON.
    #[serde(default = "default_max_trade_history")]
    pub max_trade_history: usize,
    /// When set, instruments with `instrument_type: futures` use one-way USDT-linear accounting.
    #[serde(default)]
    pub linear: Option<LinearContract>,
}

fn default_max_trade_history() -> usize {
    10_000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            instruments: Vec::new(),
            initial_balances: Vec::new(),
            fill_model: FillModelConfig::default(),
            max_trade_history: default_max_trade_history(),
            linear: None,
        }
    }
}
