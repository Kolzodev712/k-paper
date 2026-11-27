//! Hot-path micro-benchmarks: orderbook injection and place/cancel/fill soak.
//!
//! Run with `cargo bench -p paper-exchange-core --bench inject_book`.

use chrono::Utc;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use paper_exchange_core::config::{Config, InitialBalance};
use paper_exchange_core::exchange::PaperExchange;
use paper_exchange_core::types::*;
use rust_decimal::Decimal;
use tokio::sync::mpsc::UnboundedReceiver;

type Market = UnboundedReceiver<StreamEvent<MarketDataEvent>>;
type Private = UnboundedReceiver<StreamEvent<PrivateEvent>>;

fn btc() -> InstrumentId {
    InstrumentId::new("BTC-USDT")
}

fn config() -> Config {
    Config {
        instruments: vec![Instrument {
            id: btc(),
            symbol: "BTC-USDT".to_string(),
            base_asset: Asset("BTC".to_string()),
            quote_asset: Asset("USDT".to_string()),
            tick_size: Decimal::new(1, 2),
            lot_size: Decimal::new(1, 3),
            min_notional: Decimal::new(10, 0),
            instrument_type: None,
        }],
        // Large enough that a soak never hits InsufficientBalance.
        initial_balances: vec![
            InitialBalance {
                asset: Asset("USDT".to_string()),
                amount: Decimal::new(1_000_000_000_000_000, 0),
            },
            InitialBalance {
                asset: Asset("BTC".to_string()),
                amount: Decimal::new(1_000_000_000, 0),
            },
        ],
        ..Config::default()
    }
}

/// `levels` bids below and asks above `mid`, 1.00 apart, tick-aligned.
fn snapshot(levels: i64, mid: i64, sequence: u64) -> OrderbookSnapshot {
    let level = |price: i64| BookLevel {
        price: Decimal::new(price, 0),
        quantity: Decimal::new(10, 0),
    };
    OrderbookSnapshot {
        instrument_id: btc(),
        bids: (0..levels).map(|i| level(mid - 1 - i)).collect(),
        asks: (0..levels).map(|i| level(mid + 1 + i)).collect(),
        sequence,
        timestamp: Utc::now(),
    }
}

fn new_exchange() -> (PaperExchange, Market, Private) {
    PaperExchange::new(config()).expect("valid bench config")
}

fn buy_limit(price: i64) -> OrderRequest {
    OrderRequest {
        instrument_id: btc(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: Decimal::new(1, 3),
        limit_price: Some(Decimal::new(price, 0)),
        client_order_id: None,
        reduce_only: false,
    }
}

/// Drop queued stream events so the unbounded channels do not grow across iterations
/// (the server's core loop drains them continuously).
fn drain(market: &mut Market, private: &mut Private) {
    while market.try_recv().is_ok() {}
    while private.try_recv().is_ok() {}
}

fn bench_inject_orderbook(c: &mut Criterion) {
    let mut group = c.benchmark_group("inject_orderbook");
    for levels in [5_i64, 50] {
        group.bench_function(format!("{levels}_levels"), |b| {
            let (mut exchange, mut market, mut private) = new_exchange();
            let mut seq = 0_u64;
            b.iter(|| {
                seq += 1;
                // Walk the mid so successive snapshots differ.
                let snap = snapshot(levels, 50_000 + (seq % 100) as i64, seq);
                exchange.inject_orderbook(black_box(snap)).unwrap();
                drain(&mut market, &mut private);
            });
        });
    }
    group.finish();
}

fn bench_place_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("place_order");

    // Resting order (below the best ask), then cancel so the open-order set stays bounded.
    group.bench_function("rest_then_cancel", |b| {
        let (mut exchange, mut market, mut private) = new_exchange();
        exchange.inject_orderbook(snapshot(5, 50_000, 1)).unwrap();
        b.iter(|| {
            let order = exchange.place_order(black_box(buy_limit(49_000))).unwrap();
            exchange
                .cancel_order(CancelRequest::ByOrderId(order.order_id))
                .unwrap();
            drain(&mut market, &mut private);
        });
    });

    // Marketable order that crosses the book and fills (trade history is bounded by config).
    // The book is re-injected per batch so liquidity is never exhausted.
    group.bench_function("cross_and_fill", |b| {
        b.iter_batched(
            || {
                let (mut exchange, market, private) = new_exchange();
                exchange.inject_orderbook(snapshot(5, 50_000, 1)).unwrap();
                (exchange, market, private)
            },
            |(mut exchange, mut market, mut private)| {
                let order = exchange.place_order(black_box(buy_limit(50_010))).unwrap();
                black_box(order.status);
                drain(&mut market, &mut private);
            },
            BatchSize::SmallInput,
        );
    });

    // Soak: many resting orders alive at once, then one injected book that sweeps matching.
    group.bench_function("soak_500_resting_then_inject", |b| {
        b.iter_batched(
            || {
                let (mut exchange, market, private) = new_exchange();
                exchange.inject_orderbook(snapshot(5, 50_000, 1)).unwrap();
                for i in 0..500_i64 {
                    exchange.place_order(buy_limit(48_000 + i % 100)).unwrap();
                }
                (exchange, market, private)
            },
            |(mut exchange, mut market, mut private)| {
                exchange
                    .inject_orderbook(black_box(snapshot(5, 50_001, 2)))
                    .unwrap();
                drain(&mut market, &mut private);
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_inject_orderbook, bench_place_order);
criterion_main!(benches);
