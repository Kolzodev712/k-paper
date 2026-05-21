//! Multi-instrument baseline market engine: random-walk orderbooks and synthetic trades.

use crate::market_runtime::{InstrumentDynamics, MarketRuntime};
use crate::transport::Command;
use paper_exchange_core::types::*;
use rand::{Rng, SeedableRng};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

fn decimal(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
}

fn tick_size_to_f64(tick: &Decimal) -> f64 {
    tick.to_string().parse().unwrap_or(0.01)
}

/// Per-instrument baseline parameters.
#[derive(Clone, Debug)]
pub struct InstrumentMarketSpec {
    pub instrument_id: InstrumentId,
    pub initial_mid: f64,
    pub tick_size: Decimal,
}

/// Engine-wide defaults (from YAML `market` or `dummy_ob` section).
#[derive(Clone, Debug)]
pub struct MarketEngineConfig {
    pub instruments: Vec<InstrumentMarketSpec>,
    pub step: f64,
    pub spread_half: f64,
    pub levels: usize,
    pub base_qty_per_level: f64,
    pub interval_ms: u64,
    pub seed: Option<u64>,
    pub trade_probability: f64,
    pub trade_max_qty: f64,
}

impl MarketEngineConfig {
    pub fn with_seed(seed: Option<u64>) -> Self {
        Self {
            instruments: vec![InstrumentMarketSpec {
                instrument_id: InstrumentId::new("BTC-USDT"),
                initial_mid: 50_000.0,
                tick_size: decimal("0.01"),
            }],
            step: 100.0,
            spread_half: 50.0,
            levels: 5,
            base_qty_per_level: 0.5,
            interval_ms: 500,
            seed,
            trade_probability: 0.25,
            trade_max_qty: 0.1,
        }
    }
}

pub fn default_initial_mid(instrument_id: &str) -> f64 {
    match instrument_id {
        "BTC-USDT" => 50_000.0,
        "ETH-USDT" => 3_000.0,
        "SOL-USDT" => 150.0,
        "BNB-USDT" => 600.0,
        "XRP-USDT" => 0.55,
        _ => 1_000.0,
    }
}

pub fn spawn_market_engine(
    config: MarketEngineConfig,
    runtime: Arc<Mutex<MarketRuntime>>,
    command_tx: mpsc::UnboundedSender<Command>,
) {
    tokio::spawn(async move { run_market_engine(config, runtime, command_tx).await });
}

async fn run_market_engine(
    config: MarketEngineConfig,
    runtime: Arc<Mutex<MarketRuntime>>,
    command_tx: mpsc::UnboundedSender<Command>,
) {
    let mut rng = config
        .seed
        .map(rand::rngs::StdRng::seed_from_u64)
        .unwrap_or_else(rand::rngs::StdRng::from_entropy);

    {
        let mut rt = runtime.lock().expect("market runtime lock");
        for spec in &config.instruments {
            rt.instruments.insert(
                spec.instrument_id.clone(),
                InstrumentDynamics {
                    mid: spec.initial_mid,
                    sequence: 0,
                },
            );
        }
    }

    let mut interval =
        tokio::time::interval(tokio::time::Duration::from_millis(config.interval_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;

        let snapshots = {
            let mut rt = runtime.lock().expect("market runtime lock");
            rt.overlay.tick();
            build_snapshots(&config, &mut rt, &mut rng)
        };

        for (snapshot, trade) in snapshots {
            if command_tx.send(Command::InjectOrderbook(snapshot)).is_err() {
                tracing::debug!("market engine stopped");
                return;
            }
            if let Some(trade) = trade {
                if command_tx.send(Command::InjectTrade(trade)).is_err() {
                    return;
                }
            }
        }
    }
}

fn build_snapshots(
    config: &MarketEngineConfig,
    rt: &mut MarketRuntime,
    rng: &mut impl Rng,
) -> Vec<(OrderbookSnapshot, Option<PublicTrade>)> {
    let mut out = Vec::with_capacity(config.instruments.len());
    for spec in &config.instruments {
        let dynamics = match rt.instruments.get_mut(&spec.instrument_id) {
            Some(d) => d,
            None => continue,
        };

        dynamics.mid += (rng.gen::<f64>() - 0.5) * 2.0 * config.step;
        dynamics.mid = dynamics.mid.max(tick_size_to_f64(&spec.tick_size));

        let effective = dynamics.mid + rt.overlay.offset(&spec.instrument_id);
        let best_bid = effective - config.spread_half;
        let best_ask = effective + config.spread_half;
        let tick_f64 = tick_size_to_f64(&spec.tick_size);

        let mut bids: Vec<BookLevel> = (0..config.levels)
            .map(|i| {
                let price = best_bid - (i as f64) * tick_f64;
                let qty = config.base_qty_per_level * (0.8 + rng.gen::<f64>() * 0.4);
                BookLevel {
                    price: Decimal::try_from(price).unwrap_or(Decimal::ZERO),
                    quantity: Decimal::try_from(qty).unwrap_or(Decimal::ZERO),
                }
            })
            .collect();
        bids.sort_by(|a, b| b.price.cmp(&a.price));

        let mut asks: Vec<BookLevel> = (0..config.levels)
            .map(|i| {
                let price = best_ask + (i as f64) * tick_f64;
                let qty = config.base_qty_per_level * (0.8 + rng.gen::<f64>() * 0.4);
                BookLevel {
                    price: Decimal::try_from(price).unwrap_or(Decimal::ZERO),
                    quantity: Decimal::try_from(qty).unwrap_or(Decimal::ZERO),
                }
            })
            .collect();
        asks.sort_by(|a, b| a.price.cmp(&b.price));

        dynamics.sequence += 1;
        let snapshot = OrderbookSnapshot {
            instrument_id: spec.instrument_id.clone(),
            bids,
            asks,
            sequence: dynamics.sequence,
            timestamp: chrono::Utc::now(),
        };

        let trade = if config.trade_probability > 0.0 && rng.gen::<f64>() < config.trade_probability
        {
            let price_dec = Decimal::try_from(effective).unwrap_or(Decimal::ZERO);
            let qty = config.trade_max_qty * (0.2 + rng.gen::<f64>() * 0.8);
            Some(PublicTrade {
                instrument_id: spec.instrument_id.clone(),
                price: price_dec,
                quantity: Decimal::try_from(qty).unwrap_or(Decimal::ZERO),
                side: if rng.gen::<bool>() {
                    Side::Buy
                } else {
                    Side::Sell
                },
                timestamp: chrono::Utc::now(),
                trade_id: TradeId::new(),
            })
        } else {
            None
        };

        out.push((snapshot, trade));
    }
    out
}
