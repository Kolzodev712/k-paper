//! One-way USDT-linear perpetual accounting for the Increment 7 test contract.
//!
//! Intentional Paper/Bybit differences are listed in `docs/PROP_TRADING_INTEGRATION_GUIDE.md`.
//! Quantity is signed: long > 0, flat = 0, short < 0 (unexpected and rejected on reduce-only exits).

use crate::error::ExchangeError;
use crate::types::{Instrument, InstrumentType, Side};
use rust_decimal::Decimal;

/// Explicit linear-contract parameters. Not silent Bybit defaults.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LinearContract {
    /// Isolated one-way leverage applied to notional for initial margin.
    pub leverage: u32,
    /// Taker fee in basis points of fill notional, charged in USDT.
    pub taker_fee_bps: u32,
    /// Funding rate applied by `apply_funding` (0 = no funding). Not a Bybit interval clock.
    #[serde(default)]
    pub funding_rate: Decimal,
}

impl Default for LinearContract {
    fn default() -> Self {
        Self {
            leverage: 10,
            taker_fee_bps: 6,
            funding_rate: Decimal::ZERO,
        }
    }
}

pub fn instrument_is_linear(instrument: &Instrument) -> bool {
    matches!(instrument.instrument_type, Some(InstrumentType::Futures))
}

pub fn initial_margin(notional: Decimal, leverage: u32) -> Result<Decimal, ExchangeError> {
    if leverage == 0 {
        return Err(ExchangeError::Validation(
            "linear leverage must be greater than zero".into(),
        ));
    }
    Ok(notional / Decimal::from(leverage))
}

pub fn taker_fee(notional: Decimal, fee_bps: u32) -> Decimal {
    notional * Decimal::from(fee_bps) / Decimal::from(10_000u32)
}

/// Signed delta this fill would apply to the one-way position.
pub fn signed_fill_qty(side: &Side, quantity: Decimal) -> Decimal {
    match side {
        Side::Buy => quantity,
        Side::Sell => -quantity,
    }
}

/// Reject an exit that would create or enlarge a short.
pub fn check_reduce_only(
    current_qty: Decimal,
    side: &Side,
    quantity: Decimal,
) -> Result<(), ExchangeError> {
    let next = current_qty + signed_fill_qty(side, quantity);
    if current_qty >= Decimal::ZERO && next < Decimal::ZERO {
        return Err(ExchangeError::Validation(
            "reduce-only would open or enlarge a short".into(),
        ));
    }
    if current_qty <= Decimal::ZERO && next > Decimal::ZERO && current_qty != Decimal::ZERO {
        return Err(ExchangeError::Validation(
            "reduce-only would reverse an unexpected short".into(),
        ));
    }
    if current_qty == Decimal::ZERO {
        return Err(ExchangeError::Validation(
            "reduce-only rejected: no position to reduce".into(),
        ));
    }
    if current_qty > Decimal::ZERO && matches!(side, Side::Buy) {
        return Err(ExchangeError::Validation(
            "reduce-only buy rejected on a long".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Asset, InstrumentId};

    fn inst() -> Instrument {
        Instrument {
            id: InstrumentId::new("BTC-USDT"),
            symbol: "BTC-USDT".into(),
            base_asset: Asset("BTC".into()),
            quote_asset: Asset("USDT".into()),
            tick_size: Decimal::new(1, 2),
            lot_size: Decimal::new(1, 3),
            min_notional: Decimal::new(10, 0),
            instrument_type: Some(InstrumentType::Futures),
        }
    }

    #[test]
    fn margin_is_notional_over_leverage() {
        let im = initial_margin(Decimal::new(50000, 0), 10).unwrap();
        assert_eq!(im, Decimal::new(5000, 0));
    }

    #[test]
    fn reduce_only_cannot_open_a_short() {
        let err =
            check_reduce_only(Decimal::new(1, 3), &Side::Sell, Decimal::new(2, 3)).unwrap_err();
        assert!(err.to_string().contains("short"));
    }

    #[test]
    fn reduce_only_can_flatten_a_long() {
        check_reduce_only(Decimal::new(1, 3), &Side::Sell, Decimal::new(1, 3)).unwrap();
    }

    #[test]
    fn linear_flag_follows_instrument_type() {
        let mut i = inst();
        assert!(instrument_is_linear(&i));
        i.instrument_type = None;
        assert!(!instrument_is_linear(&i));
    }
}
