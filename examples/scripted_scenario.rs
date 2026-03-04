//! Example: run the paper exchange with a scripted scenario (orderbook + trades + optional orders).
//! Scenario format: JSON with "events" array; each event has "t" (type) and payload.
//! Types: "orderbook" (instrument_id, bids, asks), "trade" (instrument_id, price, quantity, side),
//! "place_order" (OrderRequest), "cancel_order" (order_id or client_order_id).

use paper_exchange_core::config::{Config, FillModelConfig, InitialBalance};
use paper_exchange_core::exchange::PaperExchange;
use paper_exchange_core::types::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::fs;
use std::str::FromStr;

fn decimal(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
}

#[derive(Debug, Deserialize)]
struct ScenarioEvent {
    t: String,
    #[serde(default)]
    instrument_id: Option<String>,
    #[serde(default)]
    bids: Option<Vec<Vec<String>>>,
    #[serde(default)]
    asks: Option<Vec<Vec<String>>>,
    #[serde(default)]
    price: Option<String>,
    #[serde(default)]
    quantity: Option<String>,
    #[serde(default)]
    side: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    events: Vec<ScenarioEvent>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        instruments: vec![paper_exchange_core::types::Instrument {
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
    };

    let (mut exchange, mut market_rx, mut private_rx) = PaperExchange::new(config);

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/scenario_01.json".to_string());
    let content = fs::read_to_string(&path).unwrap_or_else(|_| {
        r#"{"events":[{"t":"orderbook","instrument_id":"BTC-USDT","bids":[["50000","1"]],"asks":[["50100","0.5"]]},{"t":"place_order","instrument_id":"BTC-USDT","side":"Buy","order_type":"Limit","quantity":"0.2","limit_price":"50100"}]}"#.to_string()
    });

    let scenario: Scenario = serde_json::from_str(&content)?;

    for ev in scenario.events {
        match ev.t.as_str() {
            "orderbook" => {
                let instrument_id = InstrumentId::new(ev.instrument_id.unwrap_or_default());
                let bids: Vec<BookLevel> = ev
                    .bids
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|p| {
                        if p.len() >= 2 {
                            Some(BookLevel {
                                price: decimal(&p[0]),
                                quantity: decimal(&p[1]),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                let asks: Vec<BookLevel> = ev
                    .asks
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|p| {
                        if p.len() >= 2 {
                            Some(BookLevel {
                                price: decimal(&p[0]),
                                quantity: decimal(&p[1]),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                let snapshot = OrderbookSnapshot {
                    instrument_id: instrument_id.clone(),
                    bids,
                    asks,
                    sequence: 0,
                    timestamp: chrono::Utc::now(),
                };
                exchange.inject_orderbook(snapshot)?;
            }
            "trade" => {
                let trade = PublicTrade {
                    instrument_id: InstrumentId::new(ev.instrument_id.unwrap_or_default()),
                    price: decimal(&ev.price.unwrap_or_default()),
                    quantity: decimal(&ev.quantity.unwrap_or_default()),
                    side: if ev.side.as_deref() == Some("Sell") {
                        Side::Sell
                    } else {
                        Side::Buy
                    },
                    timestamp: chrono::Utc::now(),
                    trade_id: TradeId::new(),
                };
                exchange.inject_trade(trade)?;
            }
            "place_order" => {
                let req = OrderRequest {
                    instrument_id: InstrumentId::new(ev.instrument_id.unwrap_or_default()),
                    side: if ev.side.as_deref() == Some("Sell") {
                        Side::Sell
                    } else {
                        Side::Buy
                    },
                    order_type: OrderType::Limit,
                    quantity: decimal(&ev.quantity.unwrap_or_default()),
                    limit_price: ev.price.map(|p| decimal(&p)),
                    client_order_id: None,
                };
                if let Ok(order) = exchange.place_order(req) {
                    eprintln!("  placed order {} -> {:?}", order.order_id, order.status);
                }
            }
            _ => {}
        }
    }

    while let Ok(ev) = private_rx.try_recv() {
        eprintln!("  private: seq {} {:?}", ev.sequence, ev.payload);
    }
    while let Ok(ev) = market_rx.try_recv() {
        eprintln!("  market: seq {} {:?}", ev.sequence, ev.payload);
    }

    eprintln!("Scenario from {} finished.", path);
    Ok(())
}
