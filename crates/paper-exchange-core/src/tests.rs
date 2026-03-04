//! Unit tests: order state, fill logic, balance, validation, determinism.

use crate::config::{Config, FillModelConfig, InitialBalance};
use crate::exchange::PaperExchange;
use crate::orderbook::Orderbook;
use crate::types::*;
use chrono::Utc;
use rust_decimal::Decimal;
use std::str::FromStr;

fn decimal(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn test_instrument() -> Instrument {
    Instrument {
        id: InstrumentId::new("BTC-USDT"),
        symbol: "BTC-USDT".to_string(),
        base_asset: Asset("BTC".to_string()),
        quote_asset: Asset("USDT".to_string()),
        tick_size: decimal("0.01"),
        lot_size: decimal("0.001"),
        min_notional: decimal("10"),
        instrument_type: None,
    }
}

fn test_config() -> Config {
    Config {
        instruments: vec![test_instrument()],
        initial_balances: vec![
            InitialBalance {
                asset: Asset("USDT".to_string()),
                amount: decimal("100000"),
            },
            InitialBalance {
                asset: Asset("BTC".to_string()),
                amount: decimal("10"),
            },
        ],
        fill_model: FillModelConfig::default(),
    }
}

#[test]
fn orderbook_apply_snapshot() {
    let mut ob = Orderbook::new();
    ob.ensure_instrument(InstrumentId::new("BTC-USDT"));
    let snapshot = OrderbookSnapshot {
        instrument_id: InstrumentId::new("BTC-USDT"),
        bids: vec![
            BookLevel {
                price: decimal("50000"),
                quantity: decimal("1"),
            },
            BookLevel {
                price: decimal("49900"),
                quantity: decimal("2"),
            },
        ],
        asks: vec![BookLevel {
            price: decimal("50100"),
            quantity: decimal("0.5"),
        }],
        sequence: 1,
        timestamp: Utc::now(),
    };
    let out = ob.apply_snapshot(snapshot);
    assert_eq!(out.bids.len(), 2);
    assert_eq!(out.asks.len(), 1);
    assert_eq!(
        ob.get(&InstrumentId::new("BTC-USDT")).unwrap().best_bid(),
        Some(decimal("50000"))
    );
    assert_eq!(
        ob.get(&InstrumentId::new("BTC-USDT")).unwrap().best_ask(),
        Some(decimal("50100"))
    );
}

#[test]
fn place_and_fill_limit_order() {
    let config = test_config();
    let (mut exchange, _market_rx, mut private_rx) = PaperExchange::new(config);

    exchange
        .inject_orderbook(OrderbookSnapshot {
            instrument_id: InstrumentId::new("BTC-USDT"),
            bids: vec![BookLevel {
                price: decimal("50000"),
                quantity: decimal("2"),
            }],
            asks: vec![BookLevel {
                price: decimal("50200"),
                quantity: decimal("1"),
            }],
            sequence: 1,
            timestamp: Utc::now(),
        })
        .unwrap();

    let order = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.5"),
            limit_price: Some(decimal("50100")),
            client_order_id: None,
        })
        .unwrap();

    assert_eq!(order.status, OrderStatus::New);

    exchange
        .inject_orderbook(OrderbookSnapshot {
            instrument_id: InstrumentId::new("BTC-USDT"),
            bids: vec![BookLevel {
                price: decimal("50000"),
                quantity: decimal("2"),
            }],
            asks: vec![BookLevel {
                price: decimal("50050"),
                quantity: decimal("1"),
            }],
            sequence: 2,
            timestamp: Utc::now(),
        })
        .unwrap();

    let mut saw_fill = false;
    while let Ok(ev) = private_rx.try_recv() {
        if let PrivateEvent::Execution(e) = ev.payload {
            if e.quantity > decimal("0") {
                assert_eq!(e.order_id, order.order_id);
                assert!(e.price <= decimal("50100"));
                saw_fill = true;
            }
        }
    }
    assert!(saw_fill);
}

#[test]
fn validation_rejects_unknown_instrument() {
    let config = test_config();
    let (mut exchange, _, _) = PaperExchange::new(config);

    let err = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("UNKNOWN"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("1"),
            limit_price: Some(decimal("50000")),
            client_order_id: None,
        })
        .unwrap_err();

    assert!(matches!(
        err,
        crate::error::ExchangeError::InvalidInstrument(_)
    ));
}

#[test]
fn duplicate_client_order_id_rejected() {
    let config = test_config();
    let (mut exchange, _, _) = PaperExchange::new(config);

    exchange
        .inject_orderbook(OrderbookSnapshot {
            instrument_id: InstrumentId::new("BTC-USDT"),
            bids: vec![],
            asks: vec![BookLevel {
                price: decimal("50100"),
                quantity: decimal("1"),
            }],
            sequence: 1,
            timestamp: Utc::now(),
        })
        .unwrap();

    let cid = ClientOrderId("my-id".to_string());
    let _ = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.1"),
            limit_price: Some(decimal("50100")),
            client_order_id: Some(cid.clone()),
        })
        .unwrap();

    let err = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.1"),
            limit_price: Some(decimal("50100")),
            client_order_id: Some(cid),
        })
        .unwrap_err();

    assert!(matches!(err, crate::error::ExchangeError::Validation(_)));
}
