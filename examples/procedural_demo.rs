//! Example: run the paper exchange with a procedural (random-walk) orderbook.
//! Generates a sequence of orderbook snapshots with price moving randomly, then places a few orders.

use paper_exchange_core::config::{Config, FillModelConfig, InitialBalance};
use paper_exchange_core::exchange::PaperExchange;
use paper_exchange_core::types::*;
use rand::Rng;
use rust_decimal::Decimal;
use std::str::FromStr;

fn decimal(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        instruments: vec![Instrument {
            id: InstrumentId::new("BTC-USDT"),
            symbol: "BTC-USDT".to_string(),
            base_asset: Asset("BTC".to_string()),
            quote_asset: Asset("USDT".to_string()),
            tick_size: decimal("0.01"),
            lot_size: decimal("0.001"),
            min_notional: decimal("10"),
            instrument_type: None,
        }],
        initial_balances: vec![
            InitialBalance {
                asset: Asset("USDT".to_string()),
                amount: decimal("1000000"),
            },
            InitialBalance {
                asset: Asset("BTC".to_string()),
                amount: decimal("100"),
            },
        ],
        fill_model: FillModelConfig::default(),
        max_trade_history: 10_000,
    };

    let (mut exchange, _market_rx, _private_rx) = PaperExchange::new(config);

    let mut rng = rand::thread_rng();
    let mut mid = 50000.0_f64;
    let spread = 50.0;
    let step = 100.0;
    let instrument_id = InstrumentId::new("BTC-USDT");

    for i in 0..20 {
        mid += (rng.gen::<f64>() - 0.5) * 2.0 * step;
        let bid = (mid - spread / 2.0).max(1000.0);
        let ask = mid + spread / 2.0;
        let snapshot = OrderbookSnapshot {
            instrument_id: instrument_id.clone(),
            bids: vec![BookLevel {
                price: Decimal::try_from(bid).unwrap(),
                quantity: Decimal::try_from(rng.gen::<f64>() * 2.0 + 0.5).unwrap(),
            }],
            asks: vec![BookLevel {
                price: Decimal::try_from(ask).unwrap(),
                quantity: Decimal::try_from(rng.gen::<f64>() * 2.0 + 0.5).unwrap(),
            }],
            sequence: i,
            timestamp: chrono::Utc::now(),
        };
        exchange.inject_orderbook(snapshot)?;
    }

    let _ = exchange.place_order(OrderRequest {
        instrument_id: instrument_id.clone(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: decimal("0.1"),
        limit_price: Some(Decimal::try_from(mid + 200.0).unwrap()),
        client_order_id: None,
    });

    eprintln!("Procedural demo: injected 20 random-walk snapshots and placed one limit buy.");
    Ok(())
}
