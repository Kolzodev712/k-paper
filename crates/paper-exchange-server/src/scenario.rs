//! Scripted scenario: inject orderbook/trades and optional orders at startup.

use crate::transport::Command;
use paper_exchange_core::types::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::path::Path;
use std::str::FromStr;
use tokio::sync::mpsc;

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
struct Scenario {
    events: Vec<ScenarioEvent>,
}

pub fn spawn_scenario(
    path: impl AsRef<Path>,
    interval_ms: u64,
    user_command_tx: mpsc::UnboundedSender<Command>,
    dummy_command_tx: mpsc::UnboundedSender<Command>,
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
        let scenario: Scenario = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("scenario parse {}: {}", path.display(), e);
                return;
            }
        };
        tracing::info!(
            "running scenario {} ({} events, interval_ms={})",
            path.display(),
            scenario.events.len(),
            interval_ms
        );
        for (i, ev) in scenario.events.into_iter().enumerate() {
            if i > 0 && interval_ms > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
            }
            if let Err(e) = dispatch_event(ev, &user_command_tx, &dummy_command_tx) {
                tracing::warn!("scenario event {}: {}", i, e);
            }
        }
        tracing::info!("scenario {} finished", path.display());
    });
}

fn dispatch_event(
    ev: ScenarioEvent,
    user_command_tx: &mpsc::UnboundedSender<Command>,
    dummy_command_tx: &mpsc::UnboundedSender<Command>,
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
            dummy_command_tx
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
            dummy_command_tx
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
            };
            user_command_tx
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
