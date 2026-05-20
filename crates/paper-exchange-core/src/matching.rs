//! Fill model: when orders fill against the injected orderbook. v1: immediate price-cross.

use crate::config::FillModelConfig;
use crate::orderbook::InstrumentOrderbook;
use crate::types::{Order, OrderType, Side};
use rust_decimal::Decimal;

#[derive(Clone, Debug)]
pub struct Fill {
    pub price: Decimal,
    pub quantity: Decimal,
    pub fee: Decimal,
}

/// Compute fill for an order against the book. v1: no resting orders; incoming crosses book.
pub fn try_fill(
    order: &Order,
    book: Option<&InstrumentOrderbook>,
    config: &FillModelConfig,
) -> Option<Fill> {
    // Scripted: fills only when scenario injects trades / explicit events, not on book cross.
    if matches!(config.kind, crate::config::FillModelKind::Scripted) {
        return None;
    }

    let book = book?;
    let remaining = order.quantity - order.filled_qty;
    if remaining <= Decimal::ZERO {
        return None;
    }

    let (fill_price, fill_qty) = match order.order_type {
        OrderType::Limit => {
            let limit = order.price;
            match order.side {
                Side::Buy => {
                    let best_ask = book.best_ask()?;
                    if best_ask > limit {
                        return None;
                    }
                    let slippage = if config.slippage_bps > 0 {
                        limit * Decimal::from(config.slippage_bps) / Decimal::from(10000)
                    } else {
                        Decimal::ZERO
                    };
                    let price = (best_ask + slippage).min(limit);
                    let available = book.best_ask_qty().unwrap_or(Decimal::ZERO);
                    let qty = remaining.min(available);
                    if qty <= Decimal::ZERO {
                        return None;
                    }
                    (price, qty)
                }
                Side::Sell => {
                    let best_bid = book.best_bid()?;
                    if best_bid < limit {
                        return None;
                    }
                    let slippage = if config.slippage_bps > 0 {
                        limit * Decimal::from(config.slippage_bps) / Decimal::from(10000)
                    } else {
                        Decimal::ZERO
                    };
                    let price = (best_bid - slippage).max(limit);
                    let available = book.best_bid_qty().unwrap_or(Decimal::ZERO);
                    let qty = remaining.min(available);
                    if qty <= Decimal::ZERO {
                        return None;
                    }
                    (price, qty)
                }
            }
        }
        OrderType::Market => match order.side {
            Side::Buy => {
                let best_ask = book.best_ask()?;
                let slippage = best_ask * Decimal::from(config.slippage_bps) / Decimal::from(10000);
                let price = best_ask + slippage;
                let available = book.best_ask_qty().unwrap_or(Decimal::ZERO);
                let qty = remaining.min(available);
                if qty <= Decimal::ZERO {
                    return None;
                }
                (price, qty)
            }
            Side::Sell => {
                let best_bid = book.best_bid()?;
                let slippage = best_bid * Decimal::from(config.slippage_bps) / Decimal::from(10000);
                let price = best_bid - slippage;
                let available = book.best_bid_qty().unwrap_or(Decimal::ZERO);
                let qty = remaining.min(available);
                if qty <= Decimal::ZERO {
                    return None;
                }
                (price, qty)
            }
        },
    };

    let notional = fill_price * fill_qty;
    let fee = notional * Decimal::from(config.fee_bps) / Decimal::from(10000);

    Some(Fill {
        price: fill_price,
        quantity: fill_qty,
        fee,
    })
}
