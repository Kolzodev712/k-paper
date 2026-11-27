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
        max_trade_history: 10_000,
        linear: None,
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
    let (mut exchange, _market_rx, mut private_rx) =
        PaperExchange::new(config).expect("valid config");

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
            reduce_only: false,
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
    let (mut exchange, _, _) = PaperExchange::new(config).expect("valid config");

    let err = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("UNKNOWN"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("1"),
            limit_price: Some(decimal("50000")),
            client_order_id: None,
            reduce_only: false,
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
    let (mut exchange, _, _) = PaperExchange::new(config).expect("valid config");

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
            reduce_only: false,
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
            reduce_only: false,
        })
        .unwrap_err();

    assert!(matches!(err, crate::error::ExchangeError::Validation(_)));
}

#[test]
fn client_order_id_lookup_includes_canceled() {
    let config = test_config();
    let (mut exchange, _, _) = PaperExchange::new(config).expect("valid config");
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

    let cid = ClientOrderId("lookup-term".to_string());
    let placed = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.1"),
            limit_price: Some(decimal("40000")),
            client_order_id: Some(cid.clone()),
            reduce_only: false,
        })
        .unwrap();
    assert_eq!(placed.status, OrderStatus::New);
    assert!(exchange.get_order_by_client_id("lookup-term").is_some());

    exchange
        .cancel_order(CancelRequest::ByClientOrderId(cid))
        .unwrap();
    let found = exchange
        .get_order_by_client_id("lookup-term")
        .expect("terminal lookup");
    assert_eq!(found.status, OrderStatus::Canceled);
    assert!(exchange.get_open_orders(None).is_empty());
}

#[test]
fn suppress_place_ack_keeps_working_order() {
    let config = test_config();
    let (mut exchange, _, mut private_rx) = PaperExchange::new(config).expect("valid config");
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

    exchange.suppress_next_place_ack("lost-ack".into());
    let placed = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.1"),
            limit_price: Some(decimal("40000")),
            client_order_id: Some(ClientOrderId("lost-ack".into())),
            reduce_only: false,
        })
        .unwrap();
    assert_eq!(placed.status, OrderStatus::New);
    let mut saw_ack = false;
    while let Ok(ev) = private_rx.try_recv() {
        if let PrivateEvent::Execution(e) = ev.payload {
            if e.quantity == decimal("0") && e.order_id == placed.order_id {
                saw_ack = true;
            }
        }
    }
    assert!(!saw_ack, "place ACK must be suppressed");
    let found = exchange
        .get_order_by_client_id("lost-ack")
        .expect("working order retained");
    assert_eq!(found.status, OrderStatus::New);
}

#[test]
fn suppress_next_place_ack_any_without_client_id() {
    let config = test_config();
    let (mut exchange, _, mut private_rx) = PaperExchange::new(config).expect("valid config");
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

    exchange.suppress_next_place_ack_any();
    let placed = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.1"),
            limit_price: Some(decimal("40000")),
            client_order_id: Some(ClientOrderId("runner-generated".into())),
            reduce_only: false,
        })
        .unwrap();
    assert_eq!(placed.status, OrderStatus::New);
    let mut saw_ack = false;
    while let Ok(ev) = private_rx.try_recv() {
        if let PrivateEvent::Execution(e) = ev.payload {
            if e.quantity == decimal("0") && e.order_id == placed.order_id {
                saw_ack = true;
            }
        }
    }
    assert!(
        !saw_ack,
        "next-place ACK must be suppressed without a pre-known client id"
    );
    let found = exchange
        .get_order_by_client_id("runner-generated")
        .expect("working order retained");
    assert_eq!(found.status, OrderStatus::New);
}

fn linear_instrument() -> Instrument {
    Instrument {
        id: InstrumentId::new("BTC-USDT"),
        symbol: "BTC-USDT".to_string(),
        base_asset: Asset("BTC".to_string()),
        quote_asset: Asset("USDT".to_string()),
        tick_size: decimal("0.01"),
        lot_size: decimal("0.001"),
        min_notional: decimal("10"),
        instrument_type: Some(InstrumentType::Futures),
    }
}

fn linear_config() -> Config {
    Config {
        instruments: vec![linear_instrument()],
        initial_balances: vec![InitialBalance {
            asset: Asset("USDT".to_string()),
            amount: decimal("100000"),
        }],
        fill_model: FillModelConfig::default(),
        max_trade_history: 10_000,
        linear: Some(crate::linear::LinearContract {
            leverage: 10,
            taker_fee_bps: 6,
            funding_rate: Decimal::ZERO,
        }),
    }
}

fn linear_book(ask: &str, bid: &str) -> OrderbookSnapshot {
    OrderbookSnapshot {
        instrument_id: InstrumentId::new("BTC-USDT"),
        bids: vec![BookLevel {
            price: decimal(bid),
            quantity: decimal("2"),
        }],
        asks: vec![BookLevel {
            price: decimal(ask),
            quantity: decimal("2"),
        }],
        sequence: 1,
        timestamp: Utc::now(),
    }
}

#[test]
fn linear_open_buy_locks_usdt_margin_not_btc() {
    let (mut exchange, _, _) = PaperExchange::new(linear_config()).expect("valid config");
    exchange
        .inject_orderbook(linear_book("50000", "49900"))
        .unwrap();
    let order = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.001"),
            limit_price: Some(decimal("50000")),
            client_order_id: Some(ClientOrderId("lin-open".into())),
            reduce_only: false,
        })
        .unwrap();
    assert_eq!(order.status, OrderStatus::Filled);
    let (balances, positions) = exchange.get_snapshot_data();
    assert!(
        balances.iter().all(|b| b.asset.0 != "BTC"),
        "linear must not credit spot BTC"
    );
    let usdt = balances.iter().find(|b| b.asset.0 == "USDT").unwrap();
    assert_eq!(usdt.locked, decimal("5"));
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, decimal("0.001"));
}

#[test]
fn linear_reduce_only_exit_does_not_need_btc() {
    let (mut exchange, _, mut private_rx) =
        PaperExchange::new(linear_config()).expect("valid config");
    exchange
        .inject_orderbook(linear_book("50000", "49900"))
        .unwrap();
    exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.001"),
            limit_price: Some(decimal("50000")),
            client_order_id: Some(ClientOrderId("lin-open2".into())),
            reduce_only: false,
        })
        .unwrap();
    while private_rx.try_recv().is_ok() {}
    exchange
        .inject_orderbook(linear_book("51000", "50000"))
        .unwrap();
    let exit = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Sell,
            order_type: OrderType::Limit,
            quantity: decimal("0.001"),
            limit_price: Some(decimal("50000")),
            client_order_id: Some(ClientOrderId("lin-exit".into())),
            reduce_only: true,
        })
        .unwrap();
    assert_eq!(exit.status, OrderStatus::Filled);
    let (_, positions) = exchange.get_snapshot_data();
    assert!(positions.is_empty(), "flatten must clear the position");
    let mut saw_zero = false;
    while let Ok(ev) = private_rx.try_recv() {
        if let PrivateEvent::Position(p) = ev.payload {
            if p.quantity == Decimal::ZERO && p.instrument_id.0 == "BTC-USDT" {
                saw_zero = true;
            }
        }
    }
    assert!(
        saw_zero,
        "flatten must emit a qty=0 position for the runner"
    );
}

#[test]
fn linear_reduce_only_cannot_open_a_short() {
    let (mut exchange, _, _) = PaperExchange::new(linear_config()).expect("valid config");
    let err = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Sell,
            order_type: OrderType::Limit,
            quantity: decimal("0.001"),
            limit_price: Some(decimal("50000")),
            client_order_id: Some(ClientOrderId("lin-short".into())),
            reduce_only: true,
        })
        .unwrap_err();
    assert!(err.to_string().contains("reduce-only"));
}

#[test]
fn linear_one_way_sell_without_reduce_only_rejected_when_flat() {
    let (mut exchange, _, _) = PaperExchange::new(linear_config()).expect("valid config");
    let err = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Sell,
            order_type: OrderType::Limit,
            quantity: decimal("0.001"),
            limit_price: Some(decimal("50000")),
            client_order_id: Some(ClientOrderId("lin-naked".into())),
            reduce_only: false,
        })
        .unwrap_err();
    assert!(err.to_string().contains("short"));
}

#[test]
fn reject_next_place_does_not_book_an_order() {
    let (mut exchange, _, _) = PaperExchange::new(test_config()).expect("valid config");
    exchange.reject_next_place("lab_reject".into());
    let err = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.1"),
            limit_price: Some(decimal("40000")),
            client_order_id: Some(ClientOrderId("rej-1".into())),
            reduce_only: false,
        })
        .unwrap_err();
    assert!(err.to_string().contains("lab_reject"));
    assert!(exchange.get_order_by_client_id("rej-1").is_none());
    exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.1"),
            limit_price: Some(decimal("40000")),
            client_order_id: Some(ClientOrderId("rej-2".into())),
            reduce_only: false,
        })
        .unwrap();
    assert!(exchange.get_order_by_client_id("rej-2").is_some());
}

#[test]
fn reset_lab_faults_clears_one_shot_hooks() {
    let (mut exchange, _, _) = PaperExchange::new(test_config()).expect("valid config");
    exchange.suppress_next_place_ack_any();
    exchange.reject_next_place("x".into());
    exchange.duplicate_next_fill();
    exchange.reset_lab_faults();
    let placed = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.1"),
            limit_price: Some(decimal("40000")),
            client_order_id: Some(ClientOrderId("reset-1".into())),
            reduce_only: false,
        })
        .unwrap();
    assert_eq!(placed.status, OrderStatus::New);
}

#[test]
fn stale_book_ages_injected_snapshot() {
    let (mut exchange, mut market_rx, _) = PaperExchange::new(test_config()).expect("valid config");
    exchange.set_stale_book_age_secs(3_600);
    exchange
        .inject_orderbook(OrderbookSnapshot {
            instrument_id: InstrumentId::new("BTC-USDT"),
            bids: vec![BookLevel {
                price: decimal("50000"),
                quantity: decimal("1"),
            }],
            asks: vec![BookLevel {
                price: decimal("50100"),
                quantity: decimal("1"),
            }],
            sequence: 1,
            timestamp: Utc::now(),
        })
        .unwrap();
    let ev = market_rx.try_recv().expect("aged book");
    match ev.payload {
        MarketDataEvent::Orderbook(s) => {
            let age = (Utc::now() - s.timestamp).num_seconds();
            assert!(age >= 3_500, "age={age}");
        }
        other => panic!("expected orderbook, got {other:?}"),
    }
    exchange.reset_lab_faults();
    exchange
        .inject_orderbook(OrderbookSnapshot {
            instrument_id: InstrumentId::new("BTC-USDT"),
            bids: vec![BookLevel {
                price: decimal("50000"),
                quantity: decimal("1"),
            }],
            asks: vec![BookLevel {
                price: decimal("50100"),
                quantity: decimal("1"),
            }],
            sequence: 2,
            timestamp: Utc::now(),
        })
        .unwrap();
    let ev = market_rx.try_recv().expect("fresh book");
    match ev.payload {
        MarketDataEvent::Orderbook(s) => {
            let age = (Utc::now() - s.timestamp).num_seconds();
            assert!(age < 5, "age={age}");
        }
        other => panic!("expected orderbook, got {other:?}"),
    }
}

#[test]
fn partial_next_fill_leaves_working_remainder() {
    let (mut exchange, _, _) = PaperExchange::new(linear_config()).expect("valid config");
    exchange
        .inject_orderbook(linear_book("50000", "49900"))
        .unwrap();
    exchange.partial_next_fill(decimal("0.001"));
    let order = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.002"),
            limit_price: Some(decimal("50000")),
            client_order_id: Some(ClientOrderId("pf-1".into())),
            reduce_only: false,
        })
        .unwrap();
    assert_eq!(order.status, OrderStatus::PartiallyFilled);
    assert_eq!(order.filled_qty, decimal("0.001"));
    let (_, positions) = exchange.get_snapshot_data();
    assert_eq!(positions[0].quantity, decimal("0.001"));
}

#[test]
fn position_before_fill_emits_position_first() {
    let (mut exchange, _, mut private_rx) =
        PaperExchange::new(linear_config()).expect("valid config");
    exchange
        .inject_orderbook(linear_book("50000", "49900"))
        .unwrap();
    while private_rx.try_recv().is_ok() {}
    exchange.emit_position_before_fill();
    exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.001"),
            limit_price: Some(decimal("50000")),
            client_order_id: Some(ClientOrderId("ord-pos-first".into())),
            reduce_only: false,
        })
        .unwrap();
    let mut kinds = Vec::new();
    while let Ok(ev) = private_rx.try_recv() {
        match ev.payload {
            PrivateEvent::Position(_) => kinds.push("position"),
            PrivateEvent::Execution(e) if e.quantity > Decimal::ZERO => kinds.push("fill"),
            _ => {}
        }
    }
    let pos = kinds
        .iter()
        .position(|k| *k == "position")
        .expect("position");
    let fill = kinds.iter().position(|k| *k == "fill").expect("fill");
    assert!(pos < fill, "kinds={kinds:?}");
}

#[test]
fn emit_ws_position_does_not_change_rest_account() {
    let (mut exchange, _, mut private_rx) =
        PaperExchange::new(linear_config()).expect("valid config");
    let (_, rest_before) = exchange.get_snapshot_data();
    assert!(rest_before.is_empty());
    exchange.emit_ws_position(decimal("1"), decimal("50000"));
    let (_, rest_after) = exchange.get_snapshot_data();
    assert!(rest_after.is_empty());
    let ev = private_rx.try_recv().expect("ws position");
    match ev.payload {
        PrivateEvent::Position(p) => assert_eq!(p.quantity, decimal("1")),
        other => panic!("expected position, got {other:?}"),
    }
}

#[test]
fn drop_next_place_is_consumed_without_booking() {
    let (mut exchange, _, _) = PaperExchange::new(test_config()).expect("valid config");
    exchange.drop_next_place();
    assert!(exchange.take_drop_next_place());
    assert!(!exchange.take_drop_next_place());
}

#[test]
fn lab_fill_open_partial_then_cancel_remainder() {
    let (mut exchange, _, _) = PaperExchange::new(linear_config()).expect("valid config");
    exchange
        .inject_orderbook(linear_book("50000", "49900"))
        .unwrap();
    exchange.partial_next_fill(decimal("0.001"));
    let placed = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.002"),
            limit_price: Some(decimal("50000")),
            client_order_id: Some(ClientOrderId("race-1".into())),
            reduce_only: false,
        })
        .unwrap();
    assert_eq!(placed.status, OrderStatus::PartiallyFilled);
    let cancelled = exchange
        .cancel_order(CancelRequest::ByOrderId(placed.order_id))
        .unwrap();
    assert_eq!(cancelled.status, OrderStatus::Canceled);
    let (_, positions) = exchange.get_snapshot_data();
    assert_eq!(positions[0].quantity, decimal("0.001"));
    assert!(exchange.get_open_orders(None).is_empty());
}

#[test]
fn lab_fill_then_duplicate_keeps_working_remainder() {
    let (mut exchange, _, mut private_rx) =
        PaperExchange::new(linear_config()).expect("valid config");
    exchange
        .inject_orderbook(linear_book("50000", "49900"))
        .unwrap();
    exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.002"),
            limit_price: Some(decimal("49000")),
            client_order_id: Some(ClientOrderId("dup-partial".into())),
            reduce_only: false,
        })
        .unwrap();
    exchange.duplicate_next_fill();
    let filled = exchange.lab_fill_open(decimal("0.001"), None).unwrap();
    assert_eq!(filled.status, OrderStatus::PartiallyFilled);
    assert_eq!(filled.filled_qty, decimal("0.001"));
    let mut fill_ids = Vec::new();
    while let Ok(ev) = private_rx.try_recv() {
        if let PrivateEvent::Execution(e) = ev.payload {
            if e.quantity > Decimal::ZERO {
                fill_ids.push(e.trade_id);
            }
        }
    }
    assert_eq!(fill_ids.len(), 2);
    assert_eq!(fill_ids[0], fill_ids[1]);
    let open = exchange.get_open_orders(None);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].filled_qty, decimal("0.001"));
    assert_eq!(open[0].quantity - open[0].filled_qty, decimal("0.001"));
    let (_, positions) = exchange.get_snapshot_data();
    assert_eq!(positions[0].quantity, decimal("0.001"));
}

#[test]
fn lab_fill_open_targets_named_order_not_sibling() {
    let (mut exchange, _, _) = PaperExchange::new(linear_config()).expect("valid config");
    exchange
        .inject_orderbook(linear_book("50000", "49900"))
        .unwrap();
    let a = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.002"),
            limit_price: Some(decimal("48000")),
            client_order_id: Some(ClientOrderId("cid-a".into())),
            reduce_only: false,
        })
        .unwrap();
    let b = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: decimal("0.002"),
            limit_price: Some(decimal("47000")),
            client_order_id: Some(ClientOrderId("cid-b".into())),
            reduce_only: false,
        })
        .unwrap();
    let filled = exchange
        .lab_fill_open(decimal("0.002"), Some(&a.order_id.to_string()))
        .unwrap();
    assert_eq!(filled.order_id, a.order_id);
    assert_eq!(filled.status, OrderStatus::Filled);
    let open = exchange.get_open_orders(None);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].order_id, b.order_id);
}

#[test]
fn spot_sell_fill_consumes_base_inventory() {
    let (mut exchange, _, _) = PaperExchange::new(test_config()).expect("valid config");
    exchange
        .inject_orderbook(OrderbookSnapshot {
            instrument_id: InstrumentId::new("BTC-USDT"),
            bids: vec![BookLevel {
                price: decimal("50000"),
                quantity: decimal("2"),
            }],
            asks: vec![BookLevel {
                price: decimal("50100"),
                quantity: decimal("1"),
            }],
            sequence: 1,
            timestamp: Utc::now(),
        })
        .unwrap();
    let placed = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Sell,
            order_type: OrderType::Limit,
            quantity: decimal("0.001"),
            limit_price: Some(decimal("100000")),
            client_order_id: Some(ClientOrderId("spot-sell".into())),
            reduce_only: false,
        })
        .unwrap();
    assert_eq!(placed.status, OrderStatus::New);
    let filled = exchange
        .lab_fill_open(decimal("0.001"), Some(&placed.order_id.to_string()))
        .unwrap();
    assert_eq!(filled.status, OrderStatus::Filled);
    let (balances, _) = exchange.get_snapshot_data();
    let btc = balances.iter().find(|b| b.asset.0 == "BTC").unwrap();
    assert_eq!(btc.total, decimal("9.999"));
    assert_eq!(btc.locked, Decimal::ZERO);
}

// ---------- Atomic fills ----------

fn book(bid: &str, ask: &str, ask_qty: &str, seq: u64) -> OrderbookSnapshot {
    OrderbookSnapshot {
        instrument_id: InstrumentId::new("BTC-USDT"),
        bids: vec![BookLevel {
            price: decimal(bid),
            quantity: decimal("10"),
        }],
        asks: vec![BookLevel {
            price: decimal(ask),
            quantity: decimal(ask_qty),
        }],
        sequence: seq,
        timestamp: Utc::now(),
    }
}

fn buy_limit(qty: &str, price: &str) -> OrderRequest {
    OrderRequest {
        instrument_id: InstrumentId::new("BTC-USDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: decimal(qty),
        limit_price: Some(decimal(price)),
        client_order_id: None,
        reduce_only: false,
    }
}

#[test]
fn market_buy_that_would_overspend_does_not_fill_or_mutate_account() {
    let mut config = test_config();
    config.initial_balances[0].amount = decimal("100"); // 100 USDT only
    let (mut exchange, _, mut private_rx) = PaperExchange::new(config).expect("valid config");
    exchange
        .inject_orderbook(book("49900", "50000", "1", 1))
        .unwrap();

    // Market buys reserve nothing (price 0) and pass the "has some quote" check, but 0.01 BTC at
    // 50000 costs 500 USDT. Before the fix the order was marked Filled while the debit failed.
    let placed = exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Market,
            quantity: decimal("0.01"),
            limit_price: None,
            client_order_id: Some(ClientOrderId("overspend".into())),
            reduce_only: false,
        })
        .unwrap();

    assert_ne!(placed.status, OrderStatus::Filled);
    assert_eq!(placed.status, OrderStatus::Rejected);
    assert_eq!(placed.filled_qty, Decimal::ZERO);
    assert!(exchange.get_open_orders(None).is_empty());
    assert!(exchange.get_trades(None, None).is_empty());

    let (balances, _) = exchange.get_snapshot_data();
    let usdt = balances.iter().find(|b| b.asset.0 == "USDT").unwrap();
    let btc = balances.iter().find(|b| b.asset.0 == "BTC").unwrap();
    assert_eq!(usdt.total, decimal("100"));
    assert_eq!(usdt.available, decimal("100"));
    assert_eq!(btc.total, decimal("10"));

    let mut fills = 0;
    let mut rejects = 0;
    while let Ok(ev) = private_rx.try_recv() {
        if let PrivateEvent::Execution(e) = ev.payload {
            if e.quantity > Decimal::ZERO {
                fills += 1;
            }
            if e.reject_reason.is_some() {
                rejects += 1;
            }
        }
    }
    assert_eq!(fills, 0);
    assert_eq!(rejects, 1);
}

#[test]
fn unfundable_order_does_not_block_later_orders() {
    let mut config = test_config();
    config.initial_balances[0].amount = decimal("100");
    let (mut exchange, _, _) = PaperExchange::new(config).expect("valid config");
    exchange
        .place_order(OrderRequest {
            instrument_id: InstrumentId::new("BTC-USDT"),
            side: Side::Buy,
            order_type: OrderType::Market,
            quantity: decimal("0.01"),
            limit_price: None,
            client_order_id: Some(ClientOrderId("mkt".into())),
            reduce_only: false,
        })
        .unwrap();
    let limit = exchange.place_order(buy_limit("0.001", "40000")).unwrap(); // 40 USDT reserved
    assert_eq!(limit.status, OrderStatus::New);

    // Book crosses both: the unfunded market order is rejected, the funded limit still fills.
    exchange
        .inject_orderbook(book("49900", "39000", "1", 1))
        .unwrap();
    let mkt = exchange.get_order_by_client_id("mkt").unwrap();
    assert_eq!(mkt.status, OrderStatus::Rejected);
    assert!(exchange.get_open_orders(None).is_empty());
    assert_eq!(exchange.get_trades(None, None).len(), 1);
}

#[test]
fn account_apply_fill_is_atomic_on_insufficient_balance() {
    use crate::account::Account;
    let usdt = Asset("USDT".into());
    let btc = Asset("BTC".into());
    let mut account = Account::new(&[InitialBalance {
        asset: usdt.clone(),
        amount: decimal("60"),
    }]);
    account.reserve(&usdt, decimal("50")).unwrap(); // available 10, locked 50
    let before = account.get_balance(&usdt).cloned().unwrap();

    // Needs 100: release 50 -> available 60 < 100. Used to release first, then fail the debit.
    let err = account
        .apply_fill(
            Side::Buy,
            &usdt,
            &btc,
            decimal("100"),
            Decimal::ONE,
            Decimal::ZERO,
        )
        .unwrap_err();
    assert!(matches!(
        err,
        crate::error::ExchangeError::InsufficientBalance
    ));
    let after = account.get_balance(&usdt).unwrap();
    assert_eq!(after.available, before.available);
    assert_eq!(after.locked, before.locked);
    assert_eq!(after.total, before.total);
    assert!(account.get_balance(&btc).is_none());
}

#[test]
fn account_linear_fill_is_atomic_when_realized_loss_cannot_be_paid() {
    use crate::account::{Account, LinearFill};
    let usdt = Asset("USDT".into());
    let inst = InstrumentId::new("BTC-USDT");
    let mut account = Account::new(&[InitialBalance {
        asset: usdt.clone(),
        amount: decimal("10"),
    }]);
    account.update_position(Position {
        instrument_id: inst.clone(),
        side: Side::Buy,
        quantity: Decimal::ONE,
        entry_price: decimal("100"),
        unrealized_pnl: Some(Decimal::ZERO),
    });
    // Fee (1) is payable but the realized loss (50) is not: fee debit must be rolled back.
    let err = account
        .apply_linear_fill(LinearFill {
            quote_asset: &usdt,
            instrument_id: &inst,
            side: Side::Sell,
            price: decimal("50"),
            quantity: Decimal::ONE,
            fee: Decimal::ONE,
            reducing: true,
            leverage: 10,
            mark: decimal("50"),
        })
        .unwrap_err();
    assert!(matches!(
        err,
        crate::error::ExchangeError::InsufficientBalance
    ));
    assert_eq!(account.get_balance(&usdt).unwrap().total, decimal("10"));
    assert_eq!(account.get_position(&inst).unwrap().quantity, Decimal::ONE);
}

// ---------- Deterministic ordering / clock ----------

fn run_deterministic_session() -> (Vec<String>, Vec<String>, Vec<String>) {
    let (mut exchange, _, _) = PaperExchange::new(test_config()).expect("valid config");
    exchange.set_deterministic_ids(true);
    exchange.set_now_override(Some(
        chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
    ));
    exchange
        .inject_orderbook(book("49900", "50000", "1", 1))
        .unwrap();
    // Resting buys below the ask (prices are not in placement order on purpose).
    for p in [
        "40010", "40000", "40030", "40020", "40050", "40040", "40070", "40060",
    ] {
        exchange.place_order(buy_limit("0.001", p)).unwrap();
    }
    let open_ids: Vec<String> = exchange
        .get_open_orders(None)
        .iter()
        .map(|o| o.order_id.to_string())
        .collect();
    let canceled: Vec<String> = exchange
        .cancel_all_orders()
        .iter()
        .map(|o| o.order_id.to_string())
        .collect();
    let (balances, _) = exchange.get_snapshot_data();
    let assets = balances.iter().map(|b| b.asset.0.clone()).collect();
    (open_ids, canceled, assets)
}

#[test]
fn open_orders_and_cancel_all_follow_placement_order() {
    let (open_a, canceled_a, assets) = run_deterministic_session();
    assert_eq!(open_a.len(), 8);
    // Deterministic ids are the placement sequence, so placement order is directly checkable.
    let expected: Vec<String> = (1..=8).map(|n| OrderId::from_seq(n).to_string()).collect();
    assert_eq!(open_a, expected);
    assert_eq!(canceled_a, expected);
    // Balances iterate by asset key.
    assert_eq!(assets, vec!["BTC".to_string(), "USDT".to_string()]);

    // Whole run is reproducible.
    let (open_b, canceled_b, _) = run_deterministic_session();
    assert_eq!(open_a, open_b);
    assert_eq!(canceled_a, canceled_b);
}

#[test]
fn open_orders_exclude_filled_and_keep_placement_order() {
    let (mut exchange, _, _) = PaperExchange::new(test_config()).expect("valid config");
    exchange
        .inject_orderbook(book("49900", "50000", "1", 1))
        .unwrap();
    let a = exchange.place_order(buy_limit("0.001", "40000")).unwrap();
    let b = exchange.place_order(buy_limit("0.001", "41000")).unwrap();
    let c = exchange.place_order(buy_limit("0.001", "42000")).unwrap();
    exchange
        .cancel_order(CancelRequest::ByOrderId(b.order_id))
        .unwrap();
    // Ask 41500 crosses `c` (limit 42000) but not `a` (40000); `b` is already canceled.
    exchange
        .inject_orderbook(book("49900", "41500", "1", 2))
        .unwrap();
    let open: Vec<OrderId> = exchange
        .get_open_orders(None)
        .iter()
        .map(|o| o.order_id)
        .collect();
    assert_eq!(open, vec![a.order_id]);
    let trades = exchange.get_trades(None, None);
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].order_id, c.order_id);
}

#[test]
fn clock_override_stamps_orders() {
    let (mut exchange, _, _) = PaperExchange::new(test_config()).expect("valid config");
    let t = chrono::DateTime::parse_from_rfc3339("2030-05-06T07:08:09Z")
        .unwrap()
        .with_timezone(&Utc);
    exchange.set_now_override(Some(t));
    let order = exchange.place_order(buy_limit("0.001", "40000")).unwrap();
    assert_eq!(order.created_at, t);
    assert_eq!(order.updated_at, t);
}

// ---------- Trade history ----------

#[test]
fn trade_history_is_bounded_and_keeps_newest() {
    let mut config = test_config();
    config.max_trade_history = 2;
    let (mut exchange, _, _) = PaperExchange::new(config).expect("valid config");
    exchange.set_deterministic_ids(true);
    exchange
        .inject_orderbook(book("49900", "50000", "10", 1))
        .unwrap();
    for _ in 0..3 {
        let o = exchange.place_order(buy_limit("0.001", "50000")).unwrap();
        assert_eq!(o.status, OrderStatus::Filled);
    }
    let trades = exchange.get_trades(None, None);
    assert_eq!(trades.len(), 2);
    assert_eq!(trades[0].order_id, OrderId::from_seq(2));
    assert_eq!(trades[1].order_id, OrderId::from_seq(3));
    let last = exchange.get_trades(None, Some(1));
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].order_id, OrderId::from_seq(3));
}

// ---------- Unsupported fill-model config ----------

#[test]
fn new_rejects_trade_through_fill_model() {
    let mut config = test_config();
    config.fill_model.kind = crate::config::FillModelKind::TradeThrough;
    let err = PaperExchange::new(config).err().expect("must reject");
    assert!(err.to_string().contains("trade_through"), "{err}");
}

#[test]
fn new_rejects_partial_fills() {
    let mut config = test_config();
    config.fill_model.partial_fills = true;
    let err = PaperExchange::new(config).err().expect("must reject");
    assert!(err.to_string().contains("partial_fills"), "{err}");
}

#[test]
fn new_accepts_scripted_fill_model() {
    let mut config = test_config();
    config.fill_model.kind = crate::config::FillModelKind::Scripted;
    assert!(PaperExchange::new(config).is_ok());
}
