//! Multi-instrument baseline market engine: random-walk orderbooks and synthetic trades.
//!
//! Step size and spread are expressed in **ticks of each instrument** so one config works for
//! both BTC (tick 0.01, mid 50 000) and XRP (tick 0.0001, mid 0.55). Every emitted price is an
//! exact multiple of the instrument's `tick_size`.

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
    /// Max random-walk move per engine interval, in ticks (>= 1).
    pub step_ticks: u32,
    /// Half the quoted spread, in ticks (>= 1): best bid/ask sit this far from the mid.
    pub spread_half_ticks: u32,
}

/// Engine-wide settings (from YAML `market` or `dummy_ob` section).
#[derive(Clone, Debug)]
pub struct MarketEngineConfig {
    pub instruments: Vec<InstrumentMarketSpec>,
    pub levels: usize,
    pub base_qty_per_level: f64,
    pub interval_ms: u64,
    pub seed: Option<u64>,
    pub trade_probability: f64,
    pub trade_max_qty: f64,
}

/// Tick size actually used for quoting: a non-positive configured tick falls back to 0.01.
fn quote_tick(tick: &Decimal) -> Decimal {
    if *tick > Decimal::ZERO {
        *tick
    } else {
        decimal("0.01")
    }
}

/// Default `(step_ticks, spread_half_ticks)` for an instrument: ~20 bps step and ~10 bps half
/// spread of `initial_mid`, rounded to whole ticks (min 1). For BTC at 50 000 / tick 0.01 this
/// is the historical 100 / 50 in price terms.
pub fn default_ticks(initial_mid: f64, tick_size: &Decimal) -> (u32, u32) {
    let tick = tick_size_to_f64(&quote_tick(tick_size));
    let ticks = |bps: f64| -> u32 {
        let n = (initial_mid * bps / 10_000.0 / tick).round();
        if n.is_finite() && n >= 1.0 {
            n.min(f64::from(u32::MAX)) as u32
        } else {
            1
        }
    };
    (ticks(20.0), ticks(10.0))
}

/// Round `price` to the nearest multiple of `tick` (exact Decimal arithmetic).
fn round_to_tick(price: f64, tick: Decimal) -> Decimal {
    let price = Decimal::try_from(price).unwrap_or(Decimal::ZERO);
    (price / tick).round() * tick
}

impl MarketEngineConfig {
    pub fn with_seed(seed: Option<u64>) -> Self {
        let tick_size = decimal("0.01");
        let initial_mid = 50_000.0;
        let (step_ticks, spread_half_ticks) = default_ticks(initial_mid, &tick_size);
        Self {
            instruments: vec![InstrumentMarketSpec {
                instrument_id: InstrumentId::new("BTC-USDT"),
                initial_mid,
                tick_size,
                step_ticks,
                spread_half_ticks,
            }],
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
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move { run_market_engine(config, runtime, command_tx).await })
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

        let tick = quote_tick(&spec.tick_size);
        let tick_f64 = tick_size_to_f64(&tick);
        let step = f64::from(spec.step_ticks.max(1)) * tick_f64;

        dynamics.mid += (rng.gen::<f64>() - 0.5) * 2.0 * step;
        dynamics.mid = dynamics.mid.max(tick_f64);

        // Quote on the tick grid: round the mid once, then offset by whole ticks.
        let effective = dynamics.mid + rt.overlay.offset(&spec.instrument_id);
        let mid_q = round_to_tick(effective, tick).max(tick);
        let half = tick * Decimal::from(spec.spread_half_ticks.max(1));
        let best_bid = (mid_q - half).max(tick);
        let best_ask = mid_q + half;

        let mut bids: Vec<BookLevel> = (0..config.levels)
            .filter_map(|i| {
                // Levels that would reach a non-positive price are dropped.
                let price = best_bid - tick * Decimal::from(i as u64);
                (price > Decimal::ZERO).then(|| BookLevel {
                    price,
                    quantity: level_qty(config, rng),
                })
            })
            .collect();
        bids.sort_by_key(|a| std::cmp::Reverse(a.price));

        let mut asks: Vec<BookLevel> = (0..config.levels)
            .map(|i| BookLevel {
                price: best_ask + tick * Decimal::from(i as u64),
                quantity: level_qty(config, rng),
            })
            .collect();
        asks.sort_by_key(|a| a.price);

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
            let price_dec = mid_q;
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

fn level_qty(config: &MarketEngineConfig, rng: &mut impl Rng) -> Decimal {
    let qty = config.base_qty_per_level * (0.8 + rng.gen::<f64>() * 0.4);
    Decimal::try_from(qty).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_exchange_scenarios::OverlayState;
    use std::collections::HashMap;

    fn spec(id: &str, mid: f64, tick: &str) -> InstrumentMarketSpec {
        let tick_size = decimal(tick);
        let (step_ticks, spread_half_ticks) = default_ticks(mid, &tick_size);
        InstrumentMarketSpec {
            instrument_id: InstrumentId::new(id),
            initial_mid: mid,
            tick_size,
            step_ticks,
            spread_half_ticks,
        }
    }

    fn engine(specs: Vec<InstrumentMarketSpec>) -> (MarketEngineConfig, MarketRuntime) {
        let mut cfg = MarketEngineConfig::with_seed(Some(7));
        cfg.trade_probability = 1.0;
        let rt = MarketRuntime {
            instruments: specs
                .iter()
                .map(|s| {
                    (
                        s.instrument_id.clone(),
                        InstrumentDynamics {
                            mid: s.initial_mid,
                            sequence: 0,
                        },
                    )
                })
                .collect::<HashMap<_, _>>(),
            overlay: OverlayState::default(),
        };
        cfg.instruments = specs;
        (cfg, rt)
    }

    #[test]
    fn default_ticks_match_historical_btc_prices() {
        // 20 bps of 50 000 = 100; 10 bps = 50; at tick 0.01.
        assert_eq!(default_ticks(50_000.0, &decimal("0.01")), (10_000, 5_000));
        // XRP: 20 bps of 0.55 = 0.0011 -> 11 ticks of 0.0001.
        assert_eq!(default_ticks(0.55, &decimal("0.0001")), (11, 6));
    }

    #[test]
    fn prices_are_tick_aligned_with_per_instrument_spread() {
        let specs = vec![
            spec("BTC-USDT", 50_000.0, "0.01"),
            spec("SOL-USDT", 150.0, "0.01"),
            spec("XRP-USDT", 0.55, "0.0001"),
        ];
        let (cfg, mut rt) = engine(specs.clone());
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        for _ in 0..200 {
            let out = build_snapshots(&cfg, &mut rt, &mut rng);
            assert_eq!(out.len(), specs.len());
            for ((snap, trade), s) in out.iter().zip(&specs) {
                let tick = s.tick_size;
                let bid = snap.bids.first().unwrap().price;
                let ask = snap.asks.first().unwrap().price;
                assert!(bid > Decimal::ZERO && bid < ask, "{bid} !< {ask}");
                for l in snap.bids.iter().chain(&snap.asks) {
                    assert_eq!(l.price % tick, Decimal::ZERO, "{} off-tick", l.price);
                }
                assert_eq!(ask - bid, tick * Decimal::from(2 * s.spread_half_ticks));
                let t = trade.as_ref().expect("trade_probability=1");
                assert_eq!(t.price % tick, Decimal::ZERO);
            }
        }
    }

    #[test]
    fn xrp_and_sol_stay_in_a_sane_band() {
        // Regression: absolute step=100 / spread_half=50 drove SOL/XRP negative or to nonsense.
        let specs = vec![
            spec("SOL-USDT", 150.0, "0.01"),
            spec("XRP-USDT", 0.55, "0.0001"),
        ];
        let (cfg, mut rt) = engine(specs);
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        for _ in 0..500 {
            build_snapshots(&cfg, &mut rt, &mut rng);
        }
        let sol = rt.mid(&InstrumentId::new("SOL-USDT"));
        let xrp = rt.mid(&InstrumentId::new("XRP-USDT"));
        assert!((100.0..200.0).contains(&sol), "SOL drifted to {sol}");
        assert!((0.3..0.9).contains(&xrp), "XRP drifted to {xrp}");
    }
}
