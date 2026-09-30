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
    let (mut exchange, _, _) = PaperExchange::new(config);

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
    let (mut exchange, _, mut private_rx) = PaperExchange::new(config);
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
    let (mut exchange, _, mut private_rx) = PaperExchange::new(config);
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
    let (mut exchange, _, _) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, mut private_rx) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, _) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, _) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, _) = PaperExchange::new(test_config());
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
    let (mut exchange, _, _) = PaperExchange::new(test_config());
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
    let (mut exchange, mut market_rx, _) = PaperExchange::new(test_config());
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
    let (mut exchange, _, _) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, mut private_rx) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, mut private_rx) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, _) = PaperExchange::new(test_config());
    exchange.drop_next_place();
    assert!(exchange.take_drop_next_place());
    assert!(!exchange.take_drop_next_place());
}

#[test]
fn lab_fill_open_partial_then_cancel_remainder() {
    let (mut exchange, _, _) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, mut private_rx) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, _) = PaperExchange::new(linear_config());
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
    let (mut exchange, _, _) = PaperExchange::new(test_config());
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
