//! Configuration: instruments, initial balances, fill model params.
//! No file I/O in core; callers pass parsed Config.

use crate::error::ExchangeError;
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

impl FillModelConfig {
    /// Reject config surface that is accepted by serde but not implemented by matching.
    ///
    /// `trade_through` and `partial_fills` are reserved names; silently running
    /// `immediate_cross` instead would make a lab run lie about what it simulated.
    pub fn validate(&self) -> Result<(), ExchangeError> {
        if matches!(self.kind, FillModelKind::TradeThrough) {
            return Err(ExchangeError::Validation(
                "fill_model.kind `trade_through` is not implemented; use `immediate_cross` or `scripted`"
                    .to_string(),
            ));
        }
        if self.partial_fills {
            return Err(ExchangeError::Validation(
                "fill_model.partial_fills is not implemented; set `partial_fills: false`"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

impl Config {
    /// Check that the config only uses implemented features. Called by `PaperExchange::new`
    /// and by the server when loading YAML.
    pub fn validate(&self) -> Result<(), ExchangeError> {
        self.fill_model.validate()
    }
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
