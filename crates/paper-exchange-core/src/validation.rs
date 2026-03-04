//! Validation before accepting orders: symbol exists, tick/lot/min notional, balance check.

use crate::config::Config;
use crate::error::ExchangeError;
use crate::types::{OrderRequest, Side};
use rust_decimal::Decimal;
use std::ops::Rem;

pub fn validate_order_request(
    config: &Config,
    order: &OrderRequest,
    quote_available: Decimal,
    base_available: Decimal,
) -> Result<(), ExchangeError> {
    let instrument = config
        .instruments
        .iter()
        .find(|i| i.id.0 == order.instrument_id.0)
        .ok_or_else(|| ExchangeError::InvalidInstrument(order.instrument_id.0.clone()))?;

    // Tick size: price must be multiple of tick_size (or reject). We round to tick and check equality.
    if let Some(limit_price) = order.limit_price {
        if limit_price < Decimal::ZERO {
            return Err(ExchangeError::Validation("price must be positive".into()));
        }
        if instrument.tick_size > Decimal::ZERO {
            let remainder = limit_price.rem(instrument.tick_size);
            if remainder != Decimal::ZERO {
                return Err(ExchangeError::Validation(format!(
                    "price {} not multiple of tick_size {}",
                    limit_price, instrument.tick_size
                )));
            }
        }
    }

    // Lot size: quantity multiple of lot_size
    if order.quantity <= Decimal::ZERO {
        return Err(ExchangeError::Validation(
            "quantity must be positive".into(),
        ));
    }
    if instrument.lot_size > Decimal::ZERO {
        let remainder = order.quantity.rem(instrument.lot_size);
        if remainder != Decimal::ZERO {
            return Err(ExchangeError::Validation(format!(
                "quantity {} not multiple of lot_size {}",
                order.quantity, instrument.lot_size
            )));
        }
    }

    // Min notional: for limit orders price * quantity >= min_notional; for market we check at fill time
    let notional = order.limit_price.unwrap_or(Decimal::ZERO) * order.quantity;
    if order.limit_price.is_some() && notional < instrument.min_notional {
        return Err(ExchangeError::Validation(format!(
            "notional {} below min_notional {}",
            notional, instrument.min_notional
        )));
    }

    // Balance: for buy need quote (quantity * price) available; for sell need base (quantity) available
    match order.side {
        Side::Buy => {
            let required = order.quantity * order.limit_price.unwrap_or(Decimal::ZERO);
            if order.order_type == crate::types::OrderType::Market {
                // Market buy: we need at least some quote; exact check at fill time
                if quote_available <= Decimal::ZERO {
                    return Err(ExchangeError::InsufficientBalance);
                }
            } else if quote_available < required {
                return Err(ExchangeError::InsufficientBalance);
            }
        }
        Side::Sell => {
            if base_available < order.quantity {
                return Err(ExchangeError::InsufficientBalance);
            }
        }
    }

    Ok(())
}
