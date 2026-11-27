//! Scenario drivers: static startup JSON (lab) or timed overlay on baseline market engine.

use crate::market_runtime::MarketRuntime;
use crate::transport::Command;
use paper_exchange_core::types::*;
use paper_exchange_scenarios::{preset_by_name, ScenarioTimeline};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

fn decimal(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
}

#[derive(Clone, Debug)]
pub enum ScenarioLoad {
    /// Timed overlay effects on running market engine (baseline keeps emitting).
    Overlay(ScenarioTimeline),
    /// One-shot inject events at startup (legacy lab fixtures).
    Static(std::path::PathBuf),
}

pub fn resolve_scenario(spec: &str) -> Result<ScenarioLoad, String> {
    if let Some(timeline) = preset_by_name(spec) {
        return Ok(ScenarioLoad::Overlay(timeline));
    }
    let path = std::path::Path::new(spec);
    if path.exists() {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        if content.contains("\"at_ms\"") || content.contains("at_ms") {
            let timeline = ScenarioTimeline::from_json(&content).map_err(|e| e.to_string())?;
            return Ok(ScenarioLoad::Overlay(timeline));
        }
        return Ok(ScenarioLoad::Static(path.to_path_buf()));
    }
    Err(format!("unknown scenario preset or missing file: {spec}"))
}

pub fn spawn_overlay_timeline(timeline: ScenarioTimeline, runtime: Arc<Mutex<MarketRuntime>>) {
    let name = timeline.name.clone().unwrap_or_else(|| "overlay".into());
    let events = timeline.events.clone();
    tokio::spawn(async move {
        tracing::info!(
            "scenario overlay {:?}: {} timed effects",
            name,
            events.len()
        );
        let mut elapsed_ms: u64 = 0;
        for timed in events {
            if timed.at_ms > elapsed_ms {
                tokio::time::sleep(tokio::time::Duration::from_millis(timed.at_ms - elapsed_ms))
                    .await;
                elapsed_ms = timed.at_ms;
            }
            let mut rt = match runtime.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let mids = rt.mids();
            timeline.apply_effect(&timed.effect, &mut rt.overlay, &mids);
            tracing::info!(
                "scenario overlay {:?}: applied effect at {}ms",
                name,
                timed.at_ms
            );
        }
    });
}

/// Replay a static scenario file. Every event (orderbook, trade, place_order) is sent on the one
/// `command_tx` channel in file order, so the core executes "inject book" before a following
/// "place order" deterministically. Pass the core's **user** command channel (the one the core
/// treats as ordered/high priority), not the market-engine channel.
pub fn spawn_static_scenario(
    path: impl AsRef<Path>,
    interval_ms: u64,
    command_tx: mpsc::UnboundedSender<Command>,
) {
    let path = path.as_ref().to_path_buf();
    tokio::spawn(async move {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("scenario {}: {}", path.display(), e);
                return;
            }
        };
        let scenario: StaticScenario = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("scenario parse {}: {}", path.display(), e);
                return;
            }
        };
        tracing::info!(
            "static scenario {} ({} events, interval_ms={})",
            path.display(),
            scenario.events.len(),
            interval_ms
        );
        for (i, ev) in scenario.events.into_iter().enumerate() {
            if i > 0 && interval_ms > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
            }
            if let Err(e) = dispatch_static_event(ev, &command_tx) {
                tracing::warn!("scenario event {}: {}", i, e);
            }
        }
        tracing::info!("static scenario {} finished", path.display());
    });
}

#[derive(Debug, Deserialize)]
struct StaticScenarioEvent {
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
    limit_price: Option<String>,
    #[serde(default)]
    quantity: Option<String>,
    #[serde(default)]
    side: Option<String>,
    #[serde(default)]
    order_type: Option<String>,
    #[serde(default)]
    client_order_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StaticScenario {
    events: Vec<StaticScenarioEvent>,
}

fn dispatch_static_event(
    ev: StaticScenarioEvent,
    command_tx: &mpsc::UnboundedSender<Command>,
) -> Result<(), String> {
    match ev.t.as_str() {
        "orderbook" => {
            let instrument_id = InstrumentId::new(
                ev.instrument_id
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "BTC-USDT".to_string()),
            );
            let snapshot = OrderbookSnapshot {
                instrument_id,
                bids: levels_from_pairs(ev.bids),
                asks: levels_from_pairs(ev.asks),
                sequence: 0,
                timestamp: chrono::Utc::now(),
            };
            command_tx
                .send(Command::InjectOrderbook(snapshot))
                .map_err(|_| "core shut down".to_string())?;
        }
        "trade" => {
            let trade = PublicTrade {
                instrument_id: InstrumentId::new(
                    ev.instrument_id
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "BTC-USDT".to_string()),
                ),
                price: decimal(
                    ev.price
                        .as_deref()
                        .or(ev.limit_price.as_deref())
                        .unwrap_or("0"),
                ),
                quantity: decimal(ev.quantity.as_deref().unwrap_or("0")),
                side: parse_side(ev.side.as_deref()),
                timestamp: chrono::Utc::now(),
                trade_id: TradeId::new(),
            };
            command_tx
                .send(Command::InjectTrade(trade))
                .map_err(|_| "core shut down".to_string())?;
        }
        "place_order" => {
            let limit = ev.limit_price.or(ev.price).map(|p| decimal(&p));
            let order_type = match ev.order_type.as_deref() {
                Some("Market") | Some("market") => OrderType::Market,
                _ => OrderType::Limit,
            };
            let req = OrderRequest {
                instrument_id: InstrumentId::new(
                    ev.instrument_id
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "BTC-USDT".to_string()),
                ),
                side: parse_side(ev.side.as_deref()),
                order_type,
                quantity: decimal(ev.quantity.as_deref().unwrap_or("0")),
                limit_price: limit,
                client_order_id: ev.client_order_id.map(ClientOrderId),
                reduce_only: false,
            };
            command_tx
                .send(Command::PlaceOrder(req, None))
                .map_err(|_| "core shut down".to_string())?;
        }
        other => return Err(format!("unknown event type {:?}", other)),
    }
    Ok(())
}

fn levels_from_pairs(pairs: Option<Vec<Vec<String>>>) -> Vec<BookLevel> {
    pairs
        .unwrap_or_default()
        .into_iter()
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
        .collect()
}

fn parse_side(s: Option<&str>) -> Side {
    match s {
        Some("Sell") | Some("sell") => Side::Sell,
        _ => Side::Buy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(json: &str) -> StaticScenarioEvent {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn static_events_share_one_channel_in_file_order() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        dispatch_static_event(
            event(r#"{"t":"orderbook","bids":[["49900","1"]],"asks":[["50000","1"]]}"#),
            &tx,
        )
        .unwrap();
        dispatch_static_event(
            event(r#"{"t":"place_order","side":"Buy","limit_price":"50000","quantity":"0.1"}"#),
            &tx,
        )
        .unwrap();
        dispatch_static_event(
            event(r#"{"t":"trade","price":"50000","quantity":"0.1"}"#),
            &tx,
        )
        .unwrap();

        assert!(matches!(rx.try_recv(), Ok(Command::InjectOrderbook(_))));
        assert!(matches!(rx.try_recv(), Ok(Command::PlaceOrder(_, None))));
        assert!(matches!(rx.try_recv(), Ok(Command::InjectTrade(_))));
        assert!(rx.try_recv().is_err());
    }
}
