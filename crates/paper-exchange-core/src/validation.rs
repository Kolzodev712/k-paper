//! Validation before accepting orders: symbol exists, tick/lot/min notional, balance check.

use crate::config::Config;
use crate::error::ExchangeError;
use crate::linear::{check_reduce_only, initial_margin, instrument_is_linear, taker_fee};
use crate::types::{OrderRequest, Side};
use rust_decimal::Decimal;
use std::ops::Rem;

pub fn validate_order_request(
    config: &Config,
    order: &OrderRequest,
    quote_available: Decimal,
    base_available: Decimal,
    position_qty: Decimal,
) -> Result<(), ExchangeError> {
    let instrument = config
        .instruments
        .iter()
        .find(|i| i.id.0 == order.instrument_id.0)
        .ok_or_else(|| ExchangeError::InvalidInstrument(order.instrument_id.0.clone()))?;

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

    let notional = order.limit_price.unwrap_or(Decimal::ZERO) * order.quantity;
    if order.limit_price.is_some() && notional < instrument.min_notional {
        return Err(ExchangeError::Validation(format!(
            "notional {} below min_notional {}",
            notional, instrument.min_notional
        )));
    }

    if instrument_is_linear(instrument) {
        let linear = config.linear.as_ref().ok_or_else(|| {
            ExchangeError::Validation(
                "futures instrument requires explicit linear contract parameters".into(),
            )
        })?;
        if order.reduce_only {
            check_reduce_only(position_qty, &order.side, order.quantity)?;
        } else if matches!(order.side, Side::Sell) && position_qty <= Decimal::ZERO {
            return Err(ExchangeError::Validation(
                "one-way linear: cannot open or enlarge a short".into(),
            ));
        }
        let price = order.limit_price.unwrap_or(Decimal::ZERO);
        if !order.reduce_only {
            let im = initial_margin(order.quantity * price, linear.leverage)?;
            let fee = taker_fee(order.quantity * price, linear.taker_fee_bps);
            if order.order_type == crate::types::OrderType::Market {
                if quote_available <= Decimal::ZERO {
                    return Err(ExchangeError::InsufficientBalance);
                }
            } else if quote_available < im + fee {
                return Err(ExchangeError::InsufficientBalance);
            }
        }
        return Ok(());
    }

    match order.side {
        Side::Buy => {
            let required = order.quantity * order.limit_price.unwrap_or(Decimal::ZERO);
            if order.order_type == crate::types::OrderType::Market {
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
