//! Configuration: instruments, initial balances, fill model params.
//! No file I/O in core; callers pass parsed Config.

use crate::types::{Asset, Instrument};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
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
}
