//! Dummy orderbook generator: produces a stream of synthetic orderbook snapshots (random walk)
//! and public trades, injecting them into the exchange so connected trading bots receive
//! fake market data on the same stream.

use crate::transport::Command;
use paper_exchange_core::types::*;
use rand::{Rng, SeedableRng};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

fn decimal(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
}

fn tick_size_to_f64(tick: &Decimal) -> f64 {
    tick.to_string().parse().unwrap_or(0.01)
}

/// Configuration for the dummy orderbook generator.
#[derive(Clone, Debug)]
pub struct DummyObConfig {
    pub instrument_id: InstrumentId,
    /// Initial mid price (e.g. 50000 for BTC-USDT).
    pub initial_mid: f64,
    /// Max absolute change in mid per tick (random walk step).
    pub step: f64,
    /// Half-spread from mid to best bid/ask (e.g. 25 = best bid at mid-25, best ask at mid+25).
    pub spread_half: f64,
    /// Number of levels on each side (bids and asks).
    pub levels: usize,
    /// Tick size for price levels (e.g. 0.01).
    pub tick_size: Decimal,
    /// Base quantity per level (randomized slightly).
    pub base_qty_per_level: f64,
    /// Interval between snapshot updates in milliseconds.
    pub interval_ms: u64,
    /// Optional seed for deterministic stream (default: random).
    pub seed: Option<u64>,
    /// Probability per tick of emitting a synthetic public trade (0.0..=1.0). Same stream as OB.
    pub trade_probability: f64,
    /// Max quantity for synthetic trades (random up to this).
    pub trade_max_qty: f64,
}

impl Default for DummyObConfig {
    fn default() -> Self {
        Self {
            instrument_id: InstrumentId::new("BTC-USDT"),
            initial_mid: 50_000.0,
            step: 100.0,
            spread_half: 50.0,
            levels: 5,
            tick_size: decimal("0.01"),
            base_qty_per_level: 0.5,
            interval_ms: 500,
            seed: None,
            trade_probability: 0.25,
            trade_max_qty: 0.1,
        }
    }
}

/// Run the dummy OB generator in a dedicated thread. Sends `Command::InjectOrderbook`
/// each tick and, with configurable probability, `Command::InjectTrade`. Bots connected
/// to the server's WebSocket receive both on the same market data stream.
pub fn spawn_dummy_ob(config: DummyObConfig, command_tx: Sender<Command>) {
    thread::spawn(move || run_dummy_ob(config, command_tx));
}

fn run_dummy_ob(config: DummyObConfig, command_tx: Sender<Command>) {
    let mut rng = config
        .seed
        .map(rand::rngs::StdRng::seed_from_u64)
        .unwrap_or_else(rand::rngs::StdRng::from_entropy);

    let mut mid = config.initial_mid;
    let mut sequence: u64 = 0;
    let interval = Duration::from_millis(config.interval_ms);

    loop {
        mid += (rng.gen::<f64>() - 0.5) * 2.0 * config.step;
        mid = mid.max(100.0);

        let best_bid = mid - config.spread_half;
        let best_ask = mid + config.spread_half;

        let tick_f64 = tick_size_to_f64(&config.tick_size);
        let mut bids: Vec<BookLevel> = (0..config.levels)
            .map(|i| {
                let price = best_bid - (i as f64) * tick_f64;
                let price_dec = Decimal::try_from(price).unwrap_or(Decimal::ZERO);
                let qty = config.base_qty_per_level * (0.8 + rng.gen::<f64>() * 0.4);
                let qty_dec = Decimal::try_from(qty).unwrap_or(Decimal::ZERO);
                BookLevel {
                    price: price_dec,
                    quantity: qty_dec,
                }
            })
            .collect();
        bids.sort_by(|a, b| b.price.cmp(&a.price));

        let mut asks: Vec<BookLevel> = (0..config.levels)
            .map(|i| {
                let price = best_ask + (i as f64) * tick_f64;
                let price_dec = Decimal::try_from(price).unwrap_or(Decimal::ZERO);
                let qty = config.base_qty_per_level * (0.8 + rng.gen::<f64>() * 0.4);
                let qty_dec = Decimal::try_from(qty).unwrap_or(Decimal::ZERO);
                BookLevel {
                    price: price_dec,
                    quantity: qty_dec,
                }
            })
            .collect();
        asks.sort_by(|a, b| a.price.cmp(&b.price));

        sequence += 1;
        let snapshot = OrderbookSnapshot {
            instrument_id: config.instrument_id.clone(),
            bids,
            asks,
            sequence,
            timestamp: chrono::Utc::now(),
        };

        if command_tx.send(Command::InjectOrderbook(snapshot)).is_err() {
            break;
        }

        if config.trade_probability > 0.0 && rng.gen::<f64>() < config.trade_probability {
            let price = mid;
            let price_dec = Decimal::try_from(price).unwrap_or(Decimal::ZERO);
            let qty = config.trade_max_qty * (0.2 + rng.gen::<f64>() * 0.8);
            let qty_dec = Decimal::try_from(qty).unwrap_or(Decimal::ZERO);
            let side = if rng.gen::<bool>() {
                Side::Buy
            } else {
                Side::Sell
            };
            let trade = PublicTrade {
                instrument_id: config.instrument_id.clone(),
                price: price_dec,
                quantity: qty_dec,
                side,
                timestamp: chrono::Utc::now(),
                trade_id: TradeId::new(),
            };
            if command_tx.send(Command::InjectTrade(trade)).is_err() {
                break;
            }
        }

        thread::sleep(interval);
    }
    tracing::debug!("dummy OB generator stopped");
}
